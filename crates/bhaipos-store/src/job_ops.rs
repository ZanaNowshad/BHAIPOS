#![allow(
    clippy::too_many_arguments,
    reason = "job worker mutations bind explicit authority, operation, lease, progress, and event time"
)]

use super::*;

type ClaimableBackgroundJobRow = (String, String, String, i64, i64, i64, Option<i64>, i32);

impl Store {
    pub fn enqueue_background_job(
        &mut self,
        request: BackgroundJobEnqueueRequest,
    ) -> Result<BackgroundJobResult, StoreError> {
        self.validate_local_session(request.context, request.user_id)?;
        let job_type = request.job_type.trim();
        if job_type.is_empty()
            || job_type.len() > 64
            || !job_type
                .chars()
                .all(|value| value.is_ascii_uppercase() || value.is_ascii_digit() || value == '_')
        {
            return Err(StoreError::Validation(
                "job type must use uppercase letters, digits, or underscores".into(),
            ));
        }
        if request.progress_total.is_some_and(|value| value < 0) {
            return Err(StoreError::Validation(
                "job progress total cannot be negative".into(),
            ));
        }
        if !(1..=10).contains(&request.max_attempts) {
            return Err(StoreError::Validation(
                "job max attempts must be between 1 and 10".into(),
            ));
        }
        let payload: serde_json::Value = serde_json::from_str(&request.payload_json)?;
        let normalized_payload = serde_json::to_string(&payload)?;
        let digest = sha256_hex(&serde_json::to_vec(&serde_json::json!({
            "job_type": job_type,
            "payload": payload,
            "progress_total": request.progress_total,
            "cancellable": request.cancellable,
            "max_attempts": request.max_attempts,
            "not_before": request.not_before.map(|value| value.to_rfc3339()),
            "branch_id": request.context.branch_id
        }))?);
        if let Some(result) = self.load_background_job_operation(
            request.context.tenant_id,
            request.operation_id,
            "ENQUEUE",
            &digest,
        )? {
            return Ok(result);
        }
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        Self::assert_permission(
            &tx,
            request.context.tenant_id,
            request.context.branch_id,
            request.user_id,
            "job.enqueue",
        )?;
        let job_id = Uuid::new_v4();
        tx.execute(
            "INSERT INTO background_jobs(id,tenant_id,branch_id,job_type,state,progress_current,progress_total,cancellable,payload_json,created_by_user_id,created_at,updated_at,origin_device_id,operation_id,request_sha256,attempts,max_attempts,not_before) VALUES(?1,?2,?3,?4,'QUEUED',0,?5,?6,?7,?8,?9,?9,?10,?11,?12,0,?13,?14)",
            params![
                job_id.to_string(),
                request.context.tenant_id.to_string(),
                request.context.branch_id.to_string(),
                job_type,
                request.progress_total,
                if request.cancellable { 1 } else { 0 },
                normalized_payload,
                request.user_id.to_string(),
                request.now.to_rfc3339(),
                request.context.device_id.to_string(),
                request.operation_id.to_string(),
                digest,
                request.max_attempts,
                request.not_before.map(|value| value.to_rfc3339()),
            ],
        )?;
        Self::append_background_job_event(
            &tx,
            request.context,
            request.user_id,
            request.operation_id,
            job_id,
            "ENQUEUED",
            None,
            "QUEUED",
            &serde_json::json!({"job_type":job_type}),
            request.now,
        )?;
        let result = Self::background_job_result_tx(
            &tx,
            request.context.tenant_id,
            request.context.branch_id,
            job_id,
        )?;
        Self::record_background_job_operation(
            &tx,
            request.context.tenant_id,
            request.operation_id,
            "ENQUEUE",
            &digest,
            &result,
            request.now,
        )?;
        Self::append_audit(
            &tx,
            request.context.tenant_id,
            request.context.device_id,
            request.user_id,
            "BACKGROUND_JOB_ENQUEUED",
            "background_job",
            &job_id.to_string(),
            &serde_json::to_string(&result)?,
            request.now,
        )?;
        tx.commit()?;
        Ok(result)
    }

    pub fn claim_next_background_job(
        &mut self,
        context: LocalTerminalContext,
        user: UserId,
        operation_id: OperationId,
        lease_seconds: i64,
        now: DateTime<Utc>,
    ) -> Result<Option<BackgroundJobLease>, StoreError> {
        self.validate_local_session(context, user)?;
        Self::validate_background_job_lease_seconds(lease_seconds)?;
        let digest = sha256_hex(&serde_json::to_vec(&serde_json::json!({
            "branch_id": context.branch_id,
            "device_id": context.device_id,
            "lease_seconds": lease_seconds
        }))?);
        if let Some(result) =
            self.load_background_job_operation(context.tenant_id, operation_id, "CLAIM", &digest)?
        {
            return Ok(result);
        }
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        Self::assert_permission(
            &tx,
            context.tenant_id,
            context.branch_id,
            user,
            "job.execute",
        )?;
        let row: Option<ClaimableBackgroundJobRow> = tx
            .query_row(
                "SELECT id,job_type,payload_json,attempts,max_attempts,progress_current,progress_total,cancellable FROM background_jobs WHERE tenant_id=?1 AND branch_id=?2 AND state='QUEUED' AND datetime(COALESCE(retry_after,not_before,created_at))<=datetime(?3) ORDER BY datetime(COALESCE(retry_after,not_before,created_at)),created_at,id LIMIT 1",
                params![context.tenant_id.to_string(), context.branch_id.to_string(), now.to_rfc3339()],
                |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?,row.get(4)?,row.get(5)?,row.get(6)?,row.get(7)?)),
            )
            .optional()?;
        let result = if let Some((
            job_id,
            job_type,
            payload_json,
            attempts,
            max_attempts,
            progress_current,
            progress_total,
            _cancellable,
        )) = row
        {
            let lease_token = Uuid::new_v4();
            let lease_expires_at = now
                .checked_add_signed(chrono::Duration::seconds(lease_seconds))
                .ok_or_else(|| StoreError::Validation("job lease expiry overflow".into()))?;
            let changed = tx.execute(
                "UPDATE background_jobs SET state='RUNNING',attempts=attempts+1,lease_token=?2,lease_owner_device_id=?3,lease_expires_at=?4,retry_after=NULL,error_text=NULL,started_at=COALESCE(started_at,?5),updated_at=?5 WHERE id=?1 AND tenant_id=?6 AND branch_id=?7 AND state='QUEUED'",
                params![job_id,lease_token.to_string(),context.device_id.to_string(),lease_expires_at.to_rfc3339(),now.to_rfc3339(),context.tenant_id.to_string(),context.branch_id.to_string()],
            )?;
            if changed != 1 {
                return Err(StoreError::Conflict("background job lease lost".into()));
            }
            let parsed_job_id = Uuid::parse_str(&job_id)
                .map_err(|_| StoreError::Validation("invalid background job identity".into()))?;
            let lease = BackgroundJobLease {
                job_id: parsed_job_id,
                job_type,
                payload_json,
                lease_token,
                lease_expires_at: lease_expires_at.to_rfc3339(),
                attempt: attempts + 1,
                max_attempts,
                progress_current,
                progress_total,
                cancel_requested: false,
            };
            Self::append_background_job_event(
                &tx,
                context,
                user,
                operation_id,
                parsed_job_id,
                "CLAIMED",
                Some("QUEUED"),
                "RUNNING",
                &serde_json::json!({"attempt":lease.attempt,"lease_expires_at":lease.lease_expires_at}),
                now,
            )?;
            Some(lease)
        } else {
            None
        };
        Self::record_background_job_operation(
            &tx,
            context.tenant_id,
            operation_id,
            "CLAIM",
            &digest,
            &result,
            now,
        )?;
        tx.commit()?;
        Ok(result)
    }

    pub fn heartbeat_background_job(
        &mut self,
        context: LocalTerminalContext,
        user: UserId,
        operation_id: OperationId,
        job_id: Uuid,
        lease_token: Uuid,
        progress_current: i64,
        progress_total: Option<i64>,
        extend_seconds: i64,
        now: DateTime<Utc>,
    ) -> Result<BackgroundJobProgressResult, StoreError> {
        self.validate_local_session(context, user)?;
        Self::validate_background_job_lease_seconds(extend_seconds)?;
        if progress_current < 0 || progress_total.is_some_and(|total| total < progress_current) {
            return Err(StoreError::Validation("invalid job progress".into()));
        }
        let digest = sha256_hex(&serde_json::to_vec(&(
            job_id,
            lease_token,
            progress_current,
            progress_total,
            extend_seconds,
        ))?);
        if let Some(result) = self.load_background_job_operation(
            context.tenant_id,
            operation_id,
            "HEARTBEAT",
            &digest,
        )? {
            return Ok(result);
        }
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        Self::assert_permission(
            &tx,
            context.tenant_id,
            context.branch_id,
            user,
            "job.execute",
        )?;
        let current: Option<(i64, Option<i64>, i32)> = tx.query_row(
            "SELECT progress_current,progress_total,cancel_requested_at IS NOT NULL FROM background_jobs WHERE id=?1 AND tenant_id=?2 AND branch_id=?3 AND state='RUNNING' AND lease_token=?4 AND lease_owner_device_id=?5 AND datetime(lease_expires_at)>=datetime(?6)",
            params![job_id.to_string(),context.tenant_id.to_string(),context.branch_id.to_string(),lease_token.to_string(),context.device_id.to_string(),now.to_rfc3339()],
            |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?)),
        ).optional()?;
        let Some((stored_progress, stored_total, cancel_requested)) = current else {
            return Err(StoreError::Conflict(
                "background job lease is stale or mismatched".into(),
            ));
        };
        if progress_current < stored_progress {
            return Err(StoreError::Conflict(
                "background job progress cannot move backward".into(),
            ));
        }
        if stored_total.is_some() && progress_total.is_some() && stored_total != progress_total {
            return Err(StoreError::Conflict(
                "background job progress total is immutable once set".into(),
            ));
        }
        let effective_total = stored_total.or(progress_total);
        if effective_total.is_some_and(|total| progress_current > total) {
            return Err(StoreError::Validation("job progress exceeds total".into()));
        }
        let lease_expires_at = now
            .checked_add_signed(chrono::Duration::seconds(extend_seconds))
            .ok_or_else(|| StoreError::Validation("job lease expiry overflow".into()))?;
        tx.execute(
            "UPDATE background_jobs SET progress_current=?2,progress_total=?3,lease_expires_at=?4,updated_at=?5 WHERE id=?1 AND state='RUNNING' AND lease_token=?6",
            params![job_id.to_string(),progress_current,effective_total,lease_expires_at.to_rfc3339(),now.to_rfc3339(),lease_token.to_string()],
        )?;
        let result = BackgroundJobProgressResult {
            state: "RUNNING".into(),
            progress_current,
            progress_total: effective_total,
            cancel_requested: cancel_requested != 0,
            lease_expires_at: lease_expires_at.to_rfc3339(),
        };
        Self::append_background_job_event(
            &tx,
            context,
            user,
            operation_id,
            job_id,
            "PROGRESS",
            Some("RUNNING"),
            "RUNNING",
            &serde_json::json!({"progress_current":progress_current,"progress_total":effective_total,"cancel_requested":result.cancel_requested}),
            now,
        )?;
        Self::record_background_job_operation(
            &tx,
            context.tenant_id,
            operation_id,
            "HEARTBEAT",
            &digest,
            &result,
            now,
        )?;
        tx.commit()?;
        Ok(result)
    }

    pub fn request_background_job_cancellation(
        &mut self,
        context: LocalTerminalContext,
        user: UserId,
        operation_id: OperationId,
        job_id: Uuid,
        reason: &str,
        now: DateTime<Utc>,
    ) -> Result<BackgroundJobResult, StoreError> {
        self.validate_local_session(context, user)?;
        let reason = reason.trim();
        if reason.is_empty() {
            return Err(StoreError::Validation(
                "job cancellation reason is required".into(),
            ));
        }
        let digest = sha256_hex(&serde_json::to_vec(&(job_id, reason))?);
        if let Some(result) =
            self.load_background_job_operation(context.tenant_id, operation_id, "CANCEL", &digest)?
        {
            return Ok(result);
        }
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        Self::assert_permission(
            &tx,
            context.tenant_id,
            context.branch_id,
            user,
            "job.manage",
        )?;
        let state: Option<(String, i32, i32)> = tx.query_row(
            "SELECT state,cancellable,cancel_requested_at IS NOT NULL FROM background_jobs WHERE id=?1 AND tenant_id=?2 AND branch_id=?3",
            params![job_id.to_string(),context.tenant_id.to_string(),context.branch_id.to_string()],
            |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?)),
        ).optional()?;
        let Some((previous_state, cancellable, already_requested)) = state else {
            return Err(StoreError::NotFound("background job"));
        };
        if cancellable == 0 {
            return Err(StoreError::Conflict(
                "background job is not cancellable".into(),
            ));
        }
        if already_requested != 0 || !matches!(previous_state.as_str(), "QUEUED" | "RUNNING") {
            return Err(StoreError::Conflict(
                "background job cannot be cancelled in its current state".into(),
            ));
        }
        let (new_state, event_type) = if previous_state == "QUEUED" {
            ("CANCELLED", "CANCELLED")
        } else {
            ("RUNNING", "CANCEL_REQUESTED")
        };
        tx.execute(
            "UPDATE background_jobs SET state=?2,cancel_requested_at=?3,cancel_requested_by_user_id=?4,cancel_reason=?5,completed_at=CASE WHEN ?2='CANCELLED' THEN ?3 ELSE NULL END,updated_at=?3 WHERE id=?1",
            params![job_id.to_string(),new_state,now.to_rfc3339(),user.to_string(),reason],
        )?;
        Self::append_background_job_event(
            &tx,
            context,
            user,
            operation_id,
            job_id,
            event_type,
            Some(&previous_state),
            new_state,
            &serde_json::json!({"reason":reason}),
            now,
        )?;
        let result =
            Self::background_job_result_tx(&tx, context.tenant_id, context.branch_id, job_id)?;
        Self::record_background_job_operation(
            &tx,
            context.tenant_id,
            operation_id,
            "CANCEL",
            &digest,
            &result,
            now,
        )?;
        Self::append_audit(
            &tx,
            context.tenant_id,
            context.device_id,
            user,
            "BACKGROUND_JOB_CANCELLATION_REQUESTED",
            "background_job",
            &job_id.to_string(),
            &serde_json::to_string(&result)?,
            now,
        )?;
        tx.commit()?;
        Ok(result)
    }

    pub fn finish_background_job(
        &mut self,
        context: LocalTerminalContext,
        user: UserId,
        operation_id: OperationId,
        job_id: Uuid,
        lease_token: Uuid,
        outcome: BackgroundJobFinishOutcome,
        now: DateTime<Utc>,
    ) -> Result<BackgroundJobResult, StoreError> {
        self.validate_local_session(context, user)?;
        let normalized_outcome = Self::normalize_background_job_outcome(outcome)?;
        let digest = sha256_hex(&serde_json::to_vec(&(
            job_id,
            lease_token,
            &normalized_outcome,
        ))?);
        if let Some(result) =
            self.load_background_job_operation(context.tenant_id, operation_id, "FINISH", &digest)?
        {
            return Ok(result);
        }
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        Self::assert_permission(
            &tx,
            context.tenant_id,
            context.branch_id,
            user,
            "job.execute",
        )?;
        let held: Option<(i64,i64,i32)> = tx.query_row(
            "SELECT attempts,max_attempts,cancel_requested_at IS NOT NULL FROM background_jobs WHERE id=?1 AND tenant_id=?2 AND branch_id=?3 AND state='RUNNING' AND lease_token=?4 AND lease_owner_device_id=?5 AND datetime(lease_expires_at)>=datetime(?6)",
            params![job_id.to_string(),context.tenant_id.to_string(),context.branch_id.to_string(),lease_token.to_string(),context.device_id.to_string(),now.to_rfc3339()],
            |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?)),
        ).optional()?;
        let Some((attempts, max_attempts, cancel_requested)) = held else {
            return Err(StoreError::Conflict(
                "background job lease is stale or mismatched".into(),
            ));
        };
        if cancel_requested != 0
            && !matches!(
                &normalized_outcome,
                BackgroundJobFinishOutcome::Cancelled { .. }
            )
        {
            return Err(StoreError::Conflict(
                "cancelled background job must acknowledge cancellation".into(),
            ));
        }
        let (state, event_type, result_json, error, retry_after, completed_at) =
            match &normalized_outcome {
                BackgroundJobFinishOutcome::Succeeded { result_json } => {
                    if cancel_requested != 0 {
                        return Err(StoreError::Conflict(
                            "cancelled background job cannot be marked successful".into(),
                        ));
                    }
                    (
                        "SUCCEEDED",
                        "SUCCEEDED",
                        Some(result_json.clone()),
                        None,
                        None,
                        Some(now.to_rfc3339()),
                    )
                }
                BackgroundJobFinishOutcome::Failed { error, retryable }
                    if *retryable && attempts < max_attempts =>
                {
                    let exponent = u32::try_from(attempts.clamp(1, 8)).unwrap_or(8);
                    let delay_seconds = (1_i64 << exponent).min(300);
                    (
                        "QUEUED",
                        "RETRY_SCHEDULED",
                        None,
                        Some(error.clone()),
                        Some((now + chrono::Duration::seconds(delay_seconds)).to_rfc3339()),
                        None,
                    )
                }
                BackgroundJobFinishOutcome::Failed { error, .. } => (
                    "FAILED",
                    "FAILED",
                    None,
                    Some(error.clone()),
                    None,
                    Some(now.to_rfc3339()),
                ),
                BackgroundJobFinishOutcome::RequiresReview { error } => (
                    "REQUIRES_REVIEW",
                    "REQUIRES_REVIEW",
                    None,
                    Some(error.clone()),
                    None,
                    Some(now.to_rfc3339()),
                ),
                BackgroundJobFinishOutcome::Cancelled { result_json } => {
                    if cancel_requested == 0 {
                        return Err(StoreError::Conflict(
                            "background job cancellation was not requested".into(),
                        ));
                    }
                    (
                        "CANCELLED",
                        "CANCELLED",
                        result_json.clone(),
                        None,
                        None,
                        Some(now.to_rfc3339()),
                    )
                }
            };
        tx.execute(
            "UPDATE background_jobs SET state=?2,result_json=?3,error_text=?4,retry_after=?5,lease_token=NULL,lease_owner_device_id=NULL,lease_expires_at=NULL,completed_at=?6,updated_at=?7 WHERE id=?1 AND state='RUNNING' AND lease_token=?8",
            params![job_id.to_string(),state,result_json,error,retry_after,completed_at,now.to_rfc3339(),lease_token.to_string()],
        )?;
        Self::append_background_job_event(
            &tx,
            context,
            user,
            operation_id,
            job_id,
            event_type,
            Some("RUNNING"),
            state,
            &serde_json::json!({"outcome":normalized_outcome,"attempt":attempts,"retry_after":retry_after}),
            now,
        )?;
        let result =
            Self::background_job_result_tx(&tx, context.tenant_id, context.branch_id, job_id)?;
        Self::record_background_job_operation(
            &tx,
            context.tenant_id,
            operation_id,
            "FINISH",
            &digest,
            &result,
            now,
        )?;
        Self::append_audit(
            &tx,
            context.tenant_id,
            context.device_id,
            user,
            "BACKGROUND_JOB_FINISHED",
            "background_job",
            &job_id.to_string(),
            &serde_json::to_string(&result)?,
            now,
        )?;
        tx.commit()?;
        Ok(result)
    }

    pub fn recover_expired_background_jobs(
        &mut self,
        context: LocalTerminalContext,
        user: UserId,
        operation_id: OperationId,
        now: DateTime<Utc>,
    ) -> Result<BackgroundJobRecoveryResult, StoreError> {
        self.validate_local_session(context, user)?;
        let digest = sha256_hex(&serde_json::to_vec(&(
            context.branch_id,
            context.device_id,
        ))?);
        if let Some(result) =
            self.load_background_job_operation(context.tenant_id, operation_id, "RECOVER", &digest)?
        {
            return Ok(result);
        }
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        Self::assert_permission(
            &tx,
            context.tenant_id,
            context.branch_id,
            user,
            "job.manage",
        )?;
        let expired = {
            let mut statement = tx.prepare(
                "SELECT id,attempts,max_attempts,cancel_requested_at IS NOT NULL FROM background_jobs WHERE tenant_id=?1 AND branch_id=?2 AND state='RUNNING' AND datetime(lease_expires_at)<datetime(?3) ORDER BY lease_expires_at,id",
            )?;
            let rows = statement
                .query_map(
                    params![
                        context.tenant_id.to_string(),
                        context.branch_id.to_string(),
                        now.to_rfc3339()
                    ],
                    |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, i64>(1)?,
                            row.get::<_, i64>(2)?,
                            row.get::<_, i32>(3)?,
                        ))
                    },
                )?
                .collect::<Result<Vec<_>, _>>()?;
            rows
        };
        let mut result = BackgroundJobRecoveryResult {
            requeued: 0,
            failed: 0,
            cancelled: 0,
        };
        for (job_id, attempts, max_attempts, cancel_requested) in expired {
            let parsed = Uuid::parse_str(&job_id)
                .map_err(|_| StoreError::Validation("invalid background job identity".into()))?;
            let (state, event_type, error, retry_after, completed_at) = if cancel_requested != 0 {
                result.cancelled += 1;
                (
                    "CANCELLED",
                    "LEASE_EXPIRED_CANCELLED",
                    None,
                    None,
                    Some(now.to_rfc3339()),
                )
            } else if attempts < max_attempts {
                result.requeued += 1;
                (
                    "QUEUED",
                    "LEASE_EXPIRED_REQUEUED",
                    Some("Recovered after expired worker lease"),
                    Some(now.to_rfc3339()),
                    None,
                )
            } else {
                result.failed += 1;
                (
                    "FAILED",
                    "LEASE_EXPIRED_FAILED",
                    Some("Worker lease expired after maximum attempts"),
                    None,
                    Some(now.to_rfc3339()),
                )
            };
            tx.execute(
                "UPDATE background_jobs SET state=?2,error_text=?3,retry_after=?4,lease_token=NULL,lease_owner_device_id=NULL,lease_expires_at=NULL,completed_at=?5,updated_at=?6 WHERE id=?1 AND state='RUNNING'",
                params![job_id,state,error,retry_after,completed_at,now.to_rfc3339()],
            )?;
            Self::append_background_job_event(
                &tx,
                context,
                user,
                operation_id,
                parsed,
                event_type,
                Some("RUNNING"),
                state,
                &serde_json::json!({"attempt":attempts,"max_attempts":max_attempts}),
                now,
            )?;
        }
        Self::record_background_job_operation(
            &tx,
            context.tenant_id,
            operation_id,
            "RECOVER",
            &digest,
            &result,
            now,
        )?;
        Self::append_audit(
            &tx,
            context.tenant_id,
            context.device_id,
            user,
            "BACKGROUND_JOBS_RECOVERED",
            "branch",
            &context.branch_id.to_string(),
            &serde_json::to_string(&result)?,
            now,
        )?;
        tx.commit()?;
        Ok(result)
    }

    pub fn background_jobs(
        &self,
        context: LocalTerminalContext,
        user: UserId,
        limit: usize,
    ) -> Result<Vec<BackgroundJobSummary>, StoreError> {
        self.validate_local_session(context, user)?;
        Self::assert_permission_conn(
            &self.conn,
            context.tenant_id,
            context.branch_id,
            user,
            "job.view",
        )?;
        if limit == 0 || limit > 500 {
            return Err(StoreError::Validation(
                "job list limit must be between 1 and 500".into(),
            ));
        }
        let mut statement = self.conn.prepare(
            "SELECT id,job_type,state,progress_current,progress_total,attempts,max_attempts,cancellable,cancel_requested_at IS NOT NULL,retry_after,error_text,created_at,updated_at FROM background_jobs WHERE tenant_id=?1 AND branch_id=?2 ORDER BY updated_at DESC,id LIMIT ?3",
        )?;
        let rows = statement
            .query_map(
                params![
                    context.tenant_id.to_string(),
                    context.branch_id.to_string(),
                    limit as i64
                ],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, i64>(3)?,
                        row.get::<_, Option<i64>>(4)?,
                        row.get::<_, i64>(5)?,
                        row.get::<_, i64>(6)?,
                        row.get::<_, i32>(7)?,
                        row.get::<_, i32>(8)?,
                        row.get::<_, Option<String>>(9)?,
                        row.get::<_, Option<String>>(10)?,
                        row.get::<_, String>(11)?,
                        row.get::<_, String>(12)?,
                    ))
                },
            )?
            .collect::<Result<Vec<_>, _>>()?;
        rows.into_iter()
            .map(
                |(
                    id,
                    job_type,
                    state,
                    progress_current,
                    progress_total,
                    attempts,
                    max_attempts,
                    cancellable,
                    cancel_requested,
                    retry_after,
                    error,
                    created_at,
                    updated_at,
                )| {
                    Ok(BackgroundJobSummary {
                        job_id: Uuid::parse_str(&id).map_err(|_| {
                            StoreError::Validation("invalid background job identity".into())
                        })?,
                        job_type,
                        state,
                        progress_current,
                        progress_total,
                        attempts,
                        max_attempts,
                        cancellable: cancellable != 0,
                        cancel_requested: cancel_requested != 0,
                        retry_after,
                        error,
                        created_at,
                        updated_at,
                    })
                },
            )
            .collect()
    }

    fn normalize_background_job_outcome(
        outcome: BackgroundJobFinishOutcome,
    ) -> Result<BackgroundJobFinishOutcome, StoreError> {
        match outcome {
            BackgroundJobFinishOutcome::Succeeded { result_json } => {
                let value: serde_json::Value = serde_json::from_str(&result_json)?;
                Ok(BackgroundJobFinishOutcome::Succeeded {
                    result_json: serde_json::to_string(&value)?,
                })
            }
            BackgroundJobFinishOutcome::Failed { error, retryable } => {
                let error = error.trim();
                if error.is_empty() {
                    return Err(StoreError::Validation(
                        "job failure reason is required".into(),
                    ));
                }
                Ok(BackgroundJobFinishOutcome::Failed {
                    error: error.into(),
                    retryable,
                })
            }
            BackgroundJobFinishOutcome::RequiresReview { error } => {
                let error = error.trim();
                if error.is_empty() {
                    return Err(StoreError::Validation(
                        "job review reason is required".into(),
                    ));
                }
                Ok(BackgroundJobFinishOutcome::RequiresReview {
                    error: error.into(),
                })
            }
            BackgroundJobFinishOutcome::Cancelled { result_json } => {
                let result_json = result_json
                    .map(|json| {
                        serde_json::from_str::<serde_json::Value>(&json)
                            .and_then(|value| serde_json::to_string(&value))
                    })
                    .transpose()?;
                Ok(BackgroundJobFinishOutcome::Cancelled { result_json })
            }
        }
    }

    fn validate_background_job_lease_seconds(value: i64) -> Result<(), StoreError> {
        if !(30..=3_600).contains(&value) {
            return Err(StoreError::Validation(
                "job lease must be between 30 and 3600 seconds".into(),
            ));
        }
        Ok(())
    }

    fn background_job_result_tx(
        tx: &Transaction<'_>,
        tenant: TenantId,
        branch: BranchId,
        job_id: Uuid,
    ) -> Result<BackgroundJobResult, StoreError> {
        tx.query_row(
            "SELECT state,progress_current,progress_total,attempts,max_attempts,cancel_requested_at IS NOT NULL,retry_after,error_text,updated_at FROM background_jobs WHERE id=?1 AND tenant_id=?2 AND branch_id=?3",
            params![job_id.to_string(),tenant.to_string(),branch.to_string()],
            |row| Ok(BackgroundJobResult { job_id,state:row.get(0)?,progress_current:row.get(1)?,progress_total:row.get(2)?,attempts:row.get(3)?,max_attempts:row.get(4)?,cancel_requested:row.get::<_,i32>(5)?!=0,retry_after:row.get(6)?,error:row.get(7)?,updated_at:row.get(8)? }),
        ).optional()?.ok_or(StoreError::NotFound("background job"))
    }

    fn load_background_job_operation<T: DeserializeOwned>(
        &self,
        tenant: TenantId,
        operation_id: OperationId,
        action: &str,
        digest: &str,
    ) -> Result<Option<T>, StoreError> {
        let stored: Option<(String,String,String)> = self.conn.query_row(
            "SELECT action,request_sha256,result_json FROM background_job_operation_results WHERE tenant_id=?1 AND operation_id=?2",
            params![tenant.to_string(),operation_id.to_string()],
            |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?)),
        ).optional()?;
        match stored {
            None => Ok(None),
            Some((stored_action, stored_digest, json))
                if stored_action == action && stored_digest == digest =>
            {
                Ok(Some(serde_json::from_str(&json)?))
            }
            Some(_) => Err(StoreError::Conflict(
                "background job operation ID reused with different action or payload".into(),
            )),
        }
    }

    fn record_background_job_operation<T: Serialize>(
        tx: &Transaction<'_>,
        tenant: TenantId,
        operation_id: OperationId,
        action: &str,
        digest: &str,
        result: &T,
        now: DateTime<Utc>,
    ) -> Result<(), StoreError> {
        tx.execute(
            "INSERT INTO background_job_operation_results(tenant_id,operation_id,action,request_sha256,result_json,committed_at) VALUES(?1,?2,?3,?4,?5,?6)",
            params![tenant.to_string(),operation_id.to_string(),action,digest,serde_json::to_string(result)?,now.to_rfc3339()],
        )?;
        Ok(())
    }

    fn append_background_job_event(
        tx: &Transaction<'_>,
        context: LocalTerminalContext,
        user: UserId,
        operation_id: OperationId,
        job_id: Uuid,
        event_type: &str,
        previous_state: Option<&str>,
        new_state: &str,
        evidence: &serde_json::Value,
        now: DateTime<Utc>,
    ) -> Result<(), StoreError> {
        tx.execute(
            "INSERT INTO background_job_events(id,tenant_id,branch_id,job_id,operation_id,event_type,previous_state,new_state,device_id,user_id,evidence_json,created_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",
            params![Uuid::new_v4().to_string(),context.tenant_id.to_string(),context.branch_id.to_string(),job_id.to_string(),operation_id.to_string(),event_type,previous_state,new_state,context.device_id.to_string(),user.to_string(),serde_json::to_string(evidence)?,now.to_rfc3339()],
        )?;
        Ok(())
    }
}
