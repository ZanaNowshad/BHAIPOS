use super::*;
use rusqlite::backup::Backup;
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::{ErrorKind, Read};
use std::path::Path;
use std::time::Duration;

#[derive(Clone)]
struct BackupRecordEvidence {
    id: Uuid,
    tenant_id: TenantId,
    branch_id: BranchId,
    backup_type: String,
    storage_path: PathBuf,
    sha256: String,
    byte_size: u64,
    schema_version: String,
    app_version: String,
    origin_device_id: DeviceId,
    created_by_user_id: UserId,
    operation_id: OperationId,
    created_at: String,
    verified_at: String,
}

impl Store {
    pub fn configure_backup_schedule(
        &mut self,
        request: BackupScheduleRequest,
    ) -> Result<BackupScheduleResult, StoreError> {
        self.validate_local_session(request.context, request.user_id)?;
        self.assert_owner(request.context, request.user_id)?;
        if !(60..=10_080).contains(&request.interval_minutes) {
            return Err(StoreError::Validation(
                "backup interval must be between 60 and 10080 minutes".into(),
            ));
        }
        if !(1..=365).contains(&request.retention_count) {
            return Err(StoreError::Validation(
                "backup retention count must be between 1 and 365".into(),
            ));
        }
        if !(1..=90).contains(&request.authorization_valid_days) {
            return Err(StoreError::Validation(
                "scheduled backup authorization must be valid for 1 to 90 days".into(),
            ));
        }
        let digest = sha256_hex(&serde_json::to_vec(&serde_json::json!({
            "branch_id": request.context.branch_id,
            "device_id": request.context.device_id,
            "interval_minutes": request.interval_minutes,
            "retention_count": request.retention_count,
            "enabled": request.enabled,
            "first_run_at": request.first_run_at.to_rfc3339(),
            "authorization_valid_days": request.authorization_valid_days,
        }))?);
        if let Some(result) = self.load_backup_schedule_operation(
            request.context.tenant_id,
            request.operation_id,
            "CONFIGURE",
            &digest,
        )? {
            return Ok(result);
        }
        let expires_at = request
            .now
            .checked_add_signed(chrono::Duration::days(request.authorization_valid_days))
            .ok_or_else(|| StoreError::Validation("authorization expiry overflow".into()))?;
        let state = if request.enabled {
            "ACTIVE"
        } else {
            "DISABLED"
        };
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        Self::assert_permission(
            &tx,
            request.context.tenant_id,
            request.context.branch_id,
            request.user_id,
            "backup.schedule",
        )?;
        let existing: Option<(String, String, i64)> = tx
            .query_row(
                "SELECT id,state,version FROM backup_schedules WHERE tenant_id=?1 AND device_id=?2",
                params![
                    request.context.tenant_id.to_string(),
                    request.context.device_id.to_string()
                ],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()?;
        let (schedule_id, previous_state, version) = match existing {
            Some((id, previous_state, version)) => {
                tx.execute(
                    "UPDATE backup_schedules SET state=?2,interval_minutes=?3,retention_count=?4,next_run_at=?5,authorization_expires_at=?6,authorized_by_user_id=?7,version=version+1,updated_at=?8 WHERE id=?1",
                    params![id,state,request.interval_minutes,request.retention_count,request.first_run_at.to_rfc3339(),expires_at.to_rfc3339(),request.user_id.to_string(),request.now.to_rfc3339()],
                )?;
                (
                    Uuid::parse_str(&id).map_err(|_| {
                        StoreError::Validation("invalid backup schedule identity".into())
                    })?,
                    Some(previous_state),
                    version + 1,
                )
            }
            None => {
                let id = Uuid::new_v4();
                tx.execute(
                    "INSERT INTO backup_schedules(id,tenant_id,branch_id,device_id,state,interval_minutes,retention_count,next_run_at,authorization_expires_at,authorized_by_user_id,version,created_at,updated_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,1,?11,?11)",
                    params![id.to_string(),request.context.tenant_id.to_string(),request.context.branch_id.to_string(),request.context.device_id.to_string(),state,request.interval_minutes,request.retention_count,request.first_run_at.to_rfc3339(),expires_at.to_rfc3339(),request.user_id.to_string(),request.now.to_rfc3339()],
                )?;
                (id, None, 1)
            }
        };
        let result = BackupScheduleResult {
            schedule_id,
            state: state.into(),
            interval_minutes: request.interval_minutes,
            retention_count: request.retention_count,
            next_run_at: request.first_run_at.to_rfc3339(),
            authorization_expires_at: expires_at.to_rfc3339(),
            version,
        };
        Self::append_backup_schedule_event(
            &tx,
            request.context,
            request.user_id,
            request.operation_id,
            schedule_id,
            "CONFIGURED",
            previous_state.as_deref(),
            state,
            &serde_json::to_value(&result)?,
            request.now,
        )?;
        Self::record_backup_schedule_operation(
            &tx,
            request.context.tenant_id,
            request.operation_id,
            "CONFIGURE",
            &digest,
            &result,
            request.now,
        )?;
        Self::append_audit(
            &tx,
            request.context.tenant_id,
            request.context.device_id,
            request.user_id,
            "BACKUP_SCHEDULE_CONFIGURED",
            "backup_schedule",
            &schedule_id.to_string(),
            &serde_json::to_string(&result)?,
            request.now,
        )?;
        tx.commit()?;
        Ok(result)
    }

    pub fn enqueue_due_backup_jobs(
        &mut self,
        context: LocalTerminalContext,
        operation_id: OperationId,
        app_version: &str,
        now: DateTime<Utc>,
    ) -> Result<BackupScheduleTickResult, StoreError> {
        self.validate_local_device_context(context)?;
        let app_version = app_version.trim();
        if app_version.is_empty() || app_version.len() > 64 {
            return Err(StoreError::Validation(
                "application version must be present and at most 64 characters".into(),
            ));
        }
        let digest = sha256_hex(&serde_json::to_vec(&serde_json::json!({
            "branch_id": context.branch_id,
            "device_id": context.device_id,
            "app_version": app_version,
        }))?);
        if let Some(result) = self.load_backup_schedule_operation(
            context.tenant_id,
            operation_id,
            "ENQUEUE_DUE",
            &digest,
        )? {
            return Ok(result);
        }
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let schedules = {
            let mut statement = tx.prepare(
                "SELECT id,interval_minutes,retention_count,next_run_at,authorization_expires_at,authorized_by_user_id,state FROM backup_schedules WHERE tenant_id=?1 AND branch_id=?2 AND device_id=?3 AND state='ACTIVE' AND datetime(next_run_at)<=datetime(?4) ORDER BY next_run_at,id",
            )?;
            let rows = statement
                .query_map(
                    params![
                        context.tenant_id.to_string(),
                        context.branch_id.to_string(),
                        context.device_id.to_string(),
                        now.to_rfc3339()
                    ],
                    |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, i64>(1)?,
                            row.get::<_, i64>(2)?,
                            row.get::<_, String>(3)?,
                            row.get::<_, String>(4)?,
                            row.get::<_, String>(5)?,
                            row.get::<_, String>(6)?,
                        ))
                    },
                )?
                .collect::<Result<Vec<_>, _>>()?;
            rows
        };
        let mut result = BackupScheduleTickResult {
            enqueued: 0,
            requires_review: 0,
            job_ids: Vec::new(),
        };
        for (schedule, interval, retention, next_run, expires, authorized_user, state) in schedules
        {
            let schedule_id = Uuid::parse_str(&schedule)
                .map_err(|_| StoreError::Validation("invalid backup schedule identity".into()))?;
            let user = UserId(Uuid::parse_str(&authorized_user).map_err(|_| {
                StoreError::Validation("invalid schedule authorization user".into())
            })?);
            let expires_at = DateTime::parse_from_rfc3339(&expires)
                .map_err(|_| {
                    StoreError::Validation("invalid schedule authorization expiry".into())
                })?
                .with_timezone(&Utc);
            let permission_count: i64 = tx.query_row(
                "SELECT COUNT(DISTINCT rp.permission_code) FROM users u JOIN user_roles ur ON ur.user_id=u.id JOIN roles role ON role.id=ur.role_id JOIN role_permissions rp ON rp.role_id=role.id WHERE u.id=?1 AND u.tenant_id=?2 AND u.status='ACTIVE' AND role.tenant_id=u.tenant_id AND (ur.branch_id IS NULL OR ur.branch_id=?3) AND rp.permission_code IN ('backup.create','job.enqueue','job.execute')",
                params![user.to_string(),context.tenant_id.to_string(),context.branch_id.to_string()],
                |row| row.get(0),
            )?;
            if expires_at <= now || permission_count != 3 {
                tx.execute(
                    "UPDATE backup_schedules SET state='REQUIRES_REVIEW',version=version+1,updated_at=?2 WHERE id=?1 AND state='ACTIVE'",
                    params![schedule, now.to_rfc3339()],
                )?;
                Self::append_backup_schedule_event(
                    &tx,
                    context,
                    user,
                    operation_id,
                    schedule_id,
                    "AUTHORIZATION_REVIEW_REQUIRED",
                    Some(&state),
                    "REQUIRES_REVIEW",
                    &serde_json::json!({"authorization_expired":expires_at<=now,"permission_count":permission_count}),
                    now,
                )?;
                Self::append_audit(
                    &tx,
                    context.tenant_id,
                    context.device_id,
                    user,
                    "BACKUP_SCHEDULE_REQUIRES_REVIEW",
                    "backup_schedule",
                    &schedule,
                    &serde_json::json!({"authorization_expired":expires_at<=now,"permission_count":permission_count}).to_string(),
                    now,
                )?;
                result.requires_review += 1;
                continue;
            }
            let next = DateTime::parse_from_rfc3339(&next_run)
                .map_err(|_| StoreError::Validation("invalid next backup time".into()))?
                .with_timezone(&Utc);
            let overdue_minutes = now.signed_duration_since(next).num_minutes().max(0);
            let steps = overdue_minutes
                .checked_div(interval)
                .and_then(|value| value.checked_add(1))
                .ok_or_else(|| StoreError::Validation("backup schedule overflow".into()))?;
            let advance_minutes = interval
                .checked_mul(steps)
                .ok_or_else(|| StoreError::Validation("backup schedule overflow".into()))?;
            let advanced = next
                .checked_add_signed(chrono::Duration::minutes(advance_minutes))
                .ok_or_else(|| StoreError::Validation("backup schedule overflow".into()))?;
            let job_id = Uuid::new_v4();
            let payload = serde_json::json!({
                "backup_type":"SCHEDULED",
                "app_version":app_version,
                "schedule_id":schedule_id,
                "retention_count":retention,
            });
            let payload_json = serde_json::to_string(&payload)?;
            let request_digest = sha256_hex(&serde_json::to_vec(&serde_json::json!({
                "job_type":"BACKUP_CREATE",
                "payload":payload,
                "progress_total":1,
                "cancellable":true,
                "max_attempts":3,
                "branch_id":context.branch_id,
            }))?);
            tx.execute(
                "INSERT INTO background_jobs(id,tenant_id,branch_id,job_type,state,progress_current,progress_total,cancellable,payload_json,created_by_user_id,created_at,updated_at,origin_device_id,operation_id,request_sha256,attempts,max_attempts,not_before) VALUES(?1,?2,?3,'BACKUP_CREATE','QUEUED',0,1,1,?4,?5,?6,?6,?7,?1,?8,0,3,?6)",
                params![job_id.to_string(),context.tenant_id.to_string(),context.branch_id.to_string(),payload_json,user.to_string(),now.to_rfc3339(),context.device_id.to_string(),request_digest],
            )?;
            tx.execute(
                "INSERT INTO background_job_events(id,tenant_id,branch_id,job_id,operation_id,event_type,previous_state,new_state,device_id,user_id,evidence_json,created_at) VALUES(?1,?2,?3,?4,?4,'ENQUEUED',NULL,'QUEUED',?5,?6,?7,?8)",
                params![Uuid::new_v4().to_string(),context.tenant_id.to_string(),context.branch_id.to_string(),job_id.to_string(),context.device_id.to_string(),user.to_string(),serde_json::json!({"job_type":"BACKUP_CREATE","schedule_id":schedule_id}).to_string(),now.to_rfc3339()],
            )?;
            tx.execute(
                "UPDATE backup_schedules SET next_run_at=?2,last_enqueued_at=?3,version=version+1,updated_at=?3 WHERE id=?1 AND state='ACTIVE'",
                params![schedule, advanced.to_rfc3339(), now.to_rfc3339()],
            )?;
            Self::append_backup_schedule_event(
                &tx,
                context,
                user,
                operation_id,
                schedule_id,
                "JOB_ENQUEUED",
                Some(&state),
                &state,
                &serde_json::json!({"job_id":job_id,"scheduled_for":next_run,"next_run_at":advanced}),
                now,
            )?;
            Self::append_audit(
                &tx,
                context.tenant_id,
                context.device_id,
                user,
                "SCHEDULED_BACKUP_ENQUEUED",
                "background_job",
                &job_id.to_string(),
                &serde_json::json!({"schedule_id":schedule_id,"scheduled_for":next_run})
                    .to_string(),
                now,
            )?;
            result.enqueued += 1;
            result.job_ids.push(job_id);
        }
        Self::record_backup_schedule_operation(
            &tx,
            context.tenant_id,
            operation_id,
            "ENQUEUE_DUE",
            &digest,
            &result,
            now,
        )?;
        tx.commit()?;
        Ok(result)
    }

    pub fn record_scheduled_backup_output(
        &mut self,
        context: LocalTerminalContext,
        user: UserId,
        schedule_id: Uuid,
        job_id: Uuid,
        backup_id: Uuid,
        now: DateTime<Utc>,
    ) -> Result<(), StoreError> {
        self.validate_local_session(context, user)?;
        let existing: Option<(String, String, String, String)> = self
            .conn
            .query_row(
                "SELECT schedule_id,backup_id,device_id,user_id FROM scheduled_backup_outputs WHERE tenant_id=?1 AND job_id=?2",
                params![context.tenant_id.to_string(),job_id.to_string()],
                |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?)),
            )
            .optional()?;
        if let Some((stored_schedule, stored_backup, stored_device, stored_user)) = existing {
            if stored_schedule == schedule_id.to_string()
                && stored_backup == backup_id.to_string()
                && stored_device == context.device_id.to_string()
                && stored_user == user.to_string()
            {
                return Ok(());
            }
            return Err(StoreError::Conflict(
                "scheduled backup job was already linked to different evidence".into(),
            ));
        }
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        Self::assert_permission(
            &tx,
            context.tenant_id,
            context.branch_id,
            user,
            "backup.create",
        )?;
        tx.execute(
            "INSERT INTO scheduled_backup_outputs(tenant_id,branch_id,schedule_id,backup_id,job_id,device_id,user_id,created_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
            params![context.tenant_id.to_string(),context.branch_id.to_string(),schedule_id.to_string(),backup_id.to_string(),job_id.to_string(),context.device_id.to_string(),user.to_string(),now.to_rfc3339()],
        )?;
        Self::append_audit(
            &tx,
            context.tenant_id,
            context.device_id,
            user,
            "SCHEDULED_BACKUP_RECORDED",
            "backup",
            &backup_id.to_string(),
            &serde_json::json!({"schedule_id":schedule_id,"job_id":job_id}).to_string(),
            now,
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn prune_scheduled_backups(
        &mut self,
        context: LocalTerminalContext,
        user: UserId,
        operation_id: OperationId,
        schedule_id: Uuid,
        backup_directory: &Path,
        now: DateTime<Utc>,
    ) -> Result<BackupRetentionResult, StoreError> {
        self.validate_local_session(context, user)?;
        self.assert_owner(context, user)?;
        let trusted_directory = Self::prepare_backup_directory(backup_directory)?;
        let digest = sha256_hex(&serde_json::to_vec(&serde_json::json!({
            "branch_id":context.branch_id,
            "device_id":context.device_id,
            "schedule_id":schedule_id,
            "trusted_directory":trusted_directory,
        }))?);
        if let Some(result) =
            self.load_backup_retention_operation(context.tenant_id, operation_id, &digest)?
        {
            return Ok(result);
        }
        let existing: Option<(String, String)> = self
            .conn
            .query_row(
                "SELECT request_sha256,state FROM backup_retention_runs WHERE tenant_id=?1 AND operation_id=?2",
                params![context.tenant_id.to_string(),operation_id.to_string()],
                |row| Ok((row.get(0)?,row.get(1)?)),
            )
            .optional()?;
        match existing {
            Some((stored_digest, _)) if stored_digest != digest => {
                return Err(StoreError::Conflict(
                    "backup retention operation id was reused with a different request".into(),
                ));
            }
            Some((_, state)) if state != "RUNNING" => {
                return Err(StoreError::Conflict(
                    "backup retention completed without replay evidence".into(),
                ));
            }
            Some(_) => {}
            None => self.plan_backup_retention(
                context,
                user,
                operation_id,
                schedule_id,
                &trusted_directory,
                &digest,
                now,
            )?,
        }
        let pending = {
            let mut statement = self.conn.prepare(
                "SELECT backup_id,storage_path FROM backup_retention_items WHERE run_id=?1 AND tenant_id=?2 AND state='PENDING' ORDER BY backup_id",
            )?;
            let rows = statement
                .query_map(
                    params![operation_id.to_string(),context.tenant_id.to_string()],
                    |row| Ok((row.get::<_,String>(0)?,row.get::<_,String>(1)?)),
                )?
                .collect::<Result<Vec<_>, _>>()?;
            rows
        };
        for (backup_id_text, storage_path_text) in pending {
            let backup_id = Uuid::parse_str(&backup_id_text)
                .map_err(|_| StoreError::Validation("invalid retention backup identity".into()))?;
            let deletion = Self::delete_planned_backup_file(
                &trusted_directory,
                backup_id,
                Path::new(&storage_path_text),
            );
            let tx = self
                .conn
                .transaction_with_behavior(TransactionBehavior::Immediate)?;
            match deletion {
                Ok(()) => {
                    tx.execute(
                        "UPDATE backup_retention_items SET state='PRUNED',error_text=NULL,updated_at=?3 WHERE run_id=?1 AND backup_id=?2 AND state='PENDING'",
                        params![operation_id.to_string(),backup_id_text,now.to_rfc3339()],
                    )?;
                    tx.execute(
                        "UPDATE backup_records SET state='PRUNED',integrity_state='PRUNED' WHERE id=?1 AND tenant_id=?2 AND state='SUCCEEDED'",
                        params![backup_id_text,context.tenant_id.to_string()],
                    )?;
                    Self::append_backup_event(
                        &tx,
                        context,
                        user,
                        Some(backup_id),
                        None,
                        "BACKUP_PRUNED",
                        &serde_json::json!({"schedule_id":schedule_id,"retention_run_id":operation_id}),
                        now,
                    )?;
                }
                Err(error) => {
                    tx.execute(
                        "UPDATE backup_retention_items SET state='FAILED',error_text=?3,updated_at=?4 WHERE run_id=?1 AND backup_id=?2 AND state='PENDING'",
                        params![operation_id.to_string(),backup_id_text,error.to_string(),now.to_rfc3339()],
                    )?;
                }
            }
            tx.commit()?;
        }
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let (pruned, failed): (i64, i64) = tx.query_row(
            "SELECT COALESCE(SUM(CASE WHEN state='PRUNED' THEN 1 ELSE 0 END),0),COALESCE(SUM(CASE WHEN state='FAILED' THEN 1 ELSE 0 END),0) FROM backup_retention_items WHERE run_id=?1 AND tenant_id=?2",
            params![operation_id.to_string(),context.tenant_id.to_string()],
            |row| Ok((row.get(0)?,row.get(1)?)),
        )?;
        let (retained, protected): (i64, i64) = tx.query_row(
            "SELECT retained_count,protected_count FROM backup_retention_runs WHERE id=?1 AND tenant_id=?2 AND state='RUNNING'",
            params![operation_id.to_string(),context.tenant_id.to_string()],
            |row| Ok((row.get(0)?,row.get(1)?)),
        )?;
        let result = BackupRetentionResult {
            run_id: operation_id.0,
            pruned: usize::try_from(pruned)
                .map_err(|_| StoreError::Validation("invalid pruned count".into()))?,
            protected: usize::try_from(protected)
                .map_err(|_| StoreError::Validation("invalid protected count".into()))?,
            retained: usize::try_from(retained)
                .map_err(|_| StoreError::Validation("invalid retained count".into()))?,
            failed: usize::try_from(failed)
                .map_err(|_| StoreError::Validation("invalid failed count".into()))?,
        };
        let state = if failed == 0 { "SUCCEEDED" } else { "FAILED" };
        tx.execute(
            "UPDATE backup_retention_runs SET state=?2,completed_at=?3,result_json=?4 WHERE id=?1 AND state='RUNNING'",
            params![operation_id.to_string(),state,now.to_rfc3339(),serde_json::to_string(&result)?],
        )?;
        Self::record_backup_retention_operation(
            &tx,
            context.tenant_id,
            operation_id,
            &digest,
            &result,
            now,
        )?;
        Self::append_audit(
            &tx,
            context.tenant_id,
            context.device_id,
            user,
            "BACKUP_RETENTION_COMPLETED",
            "backup_schedule",
            &schedule_id.to_string(),
            &serde_json::to_string(&result)?,
            now,
        )?;
        tx.commit()?;
        Ok(result)
    }

    #[allow(clippy::too_many_arguments)]
    fn plan_backup_retention(
        &mut self,
        context: LocalTerminalContext,
        user: UserId,
        operation_id: OperationId,
        schedule_id: Uuid,
        trusted_directory: &Path,
        digest: &str,
        now: DateTime<Utc>,
    ) -> Result<(), StoreError> {
        let trusted_directory_text = trusted_directory
            .to_str()
            .ok_or_else(|| StoreError::Validation("backup path must be valid Unicode".into()))?;
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        Self::assert_permission(
            &tx,
            context.tenant_id,
            context.branch_id,
            user,
            "backup.retention",
        )?;
        let retention_count: i64 = tx
            .query_row(
                "SELECT retention_count FROM backup_schedules WHERE id=?1 AND tenant_id=?2 AND branch_id=?3 AND device_id=?4",
                params![
                    schedule_id.to_string(),
                    context.tenant_id.to_string(),
                    context.branch_id.to_string(),
                    context.device_id.to_string()
                ],
                |row| row.get(0),
            )
            .optional()?
            .ok_or(StoreError::NotFound("backup schedule"))?;
        let backups = {
            let mut statement = tx.prepare(
                "SELECT b.id,b.storage_path,EXISTS(SELECT 1 FROM restore_runs r WHERE r.tenant_id=b.tenant_id AND (r.backup_id=b.id OR r.pre_restore_backup_id=b.id)) FROM scheduled_backup_outputs output JOIN backup_records b ON b.id=output.backup_id AND b.tenant_id=output.tenant_id WHERE output.tenant_id=?1 AND output.branch_id=?2 AND output.schedule_id=?3 AND output.device_id=?4 AND b.state='SUCCEEDED' AND b.integrity_state='VERIFIED' ORDER BY b.created_at DESC,b.id DESC",
            )?;
            let rows = statement
                .query_map(
                    params![
                        context.tenant_id.to_string(),
                        context.branch_id.to_string(),
                        schedule_id.to_string(),
                        context.device_id.to_string()
                    ],
                    |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, i64>(2)? != 0,
                        ))
                    },
                )?
                .collect::<Result<Vec<_>, _>>()?;
            rows
        };
        let retention_count_usize = usize::try_from(retention_count)
            .map_err(|_| StoreError::Validation("invalid backup retention count".into()))?;
        let retained = backups.len().min(retention_count_usize);
        let mut protected = 0_usize;
        let mut candidates = Vec::new();
        for (backup_id, storage_path, restore_referenced) in backups.into_iter().skip(retained) {
            if restore_referenced {
                protected = protected
                    .checked_add(1)
                    .ok_or_else(|| StoreError::Validation("protected backup count overflow".into()))?;
            } else {
                candidates.push((backup_id, storage_path));
            }
        }
        tx.execute(
            "INSERT INTO backup_retention_runs(id,tenant_id,branch_id,schedule_id,operation_id,request_sha256,trusted_directory,retention_count,retained_count,protected_count,state,device_id,user_id,created_at) VALUES(?1,?2,?3,?4,?1,?5,?6,?7,?8,?9,'RUNNING',?10,?11,?12)",
            params![
                operation_id.to_string(),
                context.tenant_id.to_string(),
                context.branch_id.to_string(),
                schedule_id.to_string(),
                digest,
                trusted_directory_text,
                retention_count,
                i64::try_from(retained)
                    .map_err(|_| StoreError::Validation("retained backup count overflow".into()))?,
                i64::try_from(protected)
                    .map_err(|_| StoreError::Validation("protected backup count overflow".into()))?,
                context.device_id.to_string(),
                user.to_string(),
                now.to_rfc3339(),
            ],
        )?;
        for (backup_id, storage_path) in &candidates {
            tx.execute(
                "INSERT INTO backup_retention_items(run_id,tenant_id,backup_id,storage_path,state,updated_at) VALUES(?1,?2,?3,?4,'PENDING',?5)",
                params![
                    operation_id.to_string(),
                    context.tenant_id.to_string(),
                    backup_id,
                    storage_path,
                    now.to_rfc3339()
                ],
            )?;
        }
        Self::append_audit(
            &tx,
            context.tenant_id,
            context.device_id,
            user,
            "BACKUP_RETENTION_PLANNED",
            "backup_schedule",
            &schedule_id.to_string(),
            &serde_json::json!({
                "run_id":operation_id,
                "retained":retained,
                "protected":protected,
                "candidates":candidates.len(),
            })
            .to_string(),
            now,
        )?;
        tx.commit()?;
        Ok(())
    }

    fn delete_planned_backup_file(
        trusted_directory: &Path,
        backup_id: Uuid,
        stored_path: &Path,
    ) -> Result<(), StoreError> {
        let expected_name = format!("bhaipos-{backup_id}.sqlite3");
        if stored_path.file_name().and_then(|name| name.to_str()) != Some(expected_name.as_str()) {
            return Err(StoreError::Validation(
                "scheduled backup path does not match its immutable identity".into(),
            ));
        }
        let parent = stored_path.parent().ok_or_else(|| {
            StoreError::Validation("scheduled backup path has no parent directory".into())
        })?;
        let canonical_parent = fs::canonicalize(parent)?;
        if canonical_parent != trusted_directory {
            return Err(StoreError::Validation(
                "scheduled backup path escapes the trusted backup directory".into(),
            ));
        }
        let metadata = match fs::symlink_metadata(stored_path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error.into()),
        };
        if metadata.file_type().is_symlink() || !metadata.file_type().is_file() {
            return Err(StoreError::Validation(
                "scheduled backup target must be a regular file".into(),
            ));
        }
        fs::remove_file(stored_path)?;
        Ok(())
    }

    pub fn create_verified_backup(
        &mut self,
        request: BackupCreateRequest,
    ) -> Result<BackupResult, StoreError> {
        self.validate_local_session(request.context, request.user_id)?;
        let backup_type = Self::normalize_backup_type(&request.backup_type)?;
        let app_version = request.app_version.trim();
        if app_version.is_empty() || app_version.len() > 64 {
            return Err(StoreError::Validation(
                "application version must be present and at most 64 characters".into(),
            ));
        }
        let destination_directory = Self::prepare_backup_directory(&request.destination_directory)?;
        let digest = sha256_hex(&serde_json::to_vec(&serde_json::json!({
            "backup_type": backup_type,
            "destination_directory": destination_directory,
            "app_version": app_version,
            "branch_id": request.context.branch_id,
        }))?);
        if let Some(result) = self.load_backup_operation(
            request.context.tenant_id,
            request.operation_id,
            "CREATE",
            &digest,
        )? {
            return Ok(result);
        }
        {
            let tx = self
                .conn
                .transaction_with_behavior(TransactionBehavior::Immediate)?;
            Self::assert_permission(
                &tx,
                request.context.tenant_id,
                request.context.branch_id,
                request.user_id,
                "backup.create",
            )?;
            tx.commit()?;
        }

        let backup_id = request.operation_id.0;
        let final_path = destination_directory.join(format!("bhaipos-{backup_id}.sqlite3"));
        let partial_path = destination_directory.join(format!(".{backup_id}.partial"));
        if partial_path.exists() {
            fs::remove_file(&partial_path)?;
        }
        let copy_result = (|| -> Result<(), StoreError> {
            let mut destination = Connection::open(&partial_path)?;
            {
                let backup = Backup::new(&self.conn, &mut destination)?;
                backup.run_to_completion(128, Duration::from_millis(5), None)?;
            }
            drop(destination);
            Self::verify_backup_database(&partial_path, request.context, request.user_id, false)?;
            if final_path.exists() {
                fs::remove_file(&final_path)?;
            }
            fs::rename(&partial_path, &final_path)?;
            Ok(())
        })();
        if copy_result.is_err() && partial_path.exists() {
            let _ = fs::remove_file(&partial_path);
        }
        copy_result?;
        let (sha256, byte_size) = Self::hash_file(&final_path)?;
        let path_text = final_path
            .to_str()
            .ok_or_else(|| StoreError::Validation("backup path must be valid Unicode".into()))?;
        let result = BackupResult {
            backup_id,
            storage_path: final_path.clone(),
            sha256: sha256.clone(),
            byte_size,
            schema_version: LATEST_SCHEMA.into(),
            integrity_state: "VERIFIED".into(),
        };
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        Self::assert_permission(
            &tx,
            request.context.tenant_id,
            request.context.branch_id,
            request.user_id,
            "backup.create",
        )?;
        tx.execute(
            "INSERT INTO backup_records(id,tenant_id,branch_id,backup_type,storage_path,sha256,byte_size,schema_version,app_version,state,integrity_state,created_at,verified_at,origin_device_id,created_by_user_id,operation_id) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,'SUCCEEDED','VERIFIED',?10,?10,?11,?12,?13)",
            params![
                backup_id.to_string(),
                request.context.tenant_id.to_string(),
                request.context.branch_id.to_string(),
                backup_type,
                path_text,
                sha256,
                i64::try_from(byte_size).map_err(|_| StoreError::Validation("backup is too large".into()))?,
                LATEST_SCHEMA,
                app_version,
                request.now.to_rfc3339(),
                request.context.device_id.to_string(),
                request.user_id.to_string(),
                request.operation_id.to_string(),
            ],
        )?;
        Self::append_backup_event(
            &tx,
            request.context,
            request.user_id,
            Some(backup_id),
            None,
            "BACKUP_VERIFIED",
            &serde_json::json!({"sha256":result.sha256,"byte_size":result.byte_size,"schema_version":LATEST_SCHEMA}),
            request.now,
        )?;
        Self::record_backup_operation(
            &tx,
            request.context.tenant_id,
            request.operation_id,
            "CREATE",
            &digest,
            &result,
            request.now,
        )?;
        Self::append_audit(
            &tx,
            request.context.tenant_id,
            request.context.device_id,
            request.user_id,
            "BACKUP_VERIFIED",
            "backup",
            &backup_id.to_string(),
            &serde_json::to_string(&result)?,
            request.now,
        )?;
        tx.commit()?;
        Ok(result)
    }

    pub fn preview_restore(
        &self,
        context: LocalTerminalContext,
        user: UserId,
        backup_id: Uuid,
    ) -> Result<RestorePreview, StoreError> {
        self.validate_local_session(context, user)?;
        if !self.user_has_permission(context, user, "backup.restore")? {
            return Err(StoreError::Authorization("backup.restore"));
        }
        self.assert_owner(context, user)?;
        let record = self.backup_record(context.tenant_id, backup_id)?;
        let (actual_hash, actual_size) = Self::hash_file(&record.storage_path)?;
        if actual_hash != record.sha256 {
            return Err(StoreError::Conflict(
                "backup hash verification failed".into(),
            ));
        }
        if actual_size != record.byte_size {
            return Err(StoreError::Conflict(
                "backup size verification failed".into(),
            ));
        }
        Self::verify_backup_database(&record.storage_path, context, user, true)?;
        Ok(RestorePreview {
            backup_id,
            sha256: actual_hash,
            byte_size: actual_size,
            schema_version: record.schema_version.clone(),
            integrity_state: "VERIFIED".into(),
            compatible: record.schema_version == LATEST_SCHEMA,
        })
    }

    pub fn restore_verified_backup(
        &mut self,
        request: RestoreBackupRequest,
    ) -> Result<RestoreResult, StoreError> {
        self.validate_local_session(request.context, request.user_id)?;
        let safety_directory = Self::prepare_backup_directory(&request.safety_backup_directory)?;
        let digest = sha256_hex(&serde_json::to_vec(&serde_json::json!({
            "backup_id": request.backup_id,
            "expected_sha256": request.expected_sha256,
            "safety_backup_directory": safety_directory,
            "app_version": request.app_version.trim(),
            "branch_id": request.context.branch_id,
        }))?);
        if let Some(result) = self.load_backup_operation(
            request.context.tenant_id,
            request.operation_id,
            "RESTORE",
            &digest,
        )? {
            return Ok(result);
        }
        if !self.user_has_permission(request.context, request.user_id, "backup.restore")? {
            return Err(StoreError::Authorization("backup.restore"));
        }
        self.assert_owner(request.context, request.user_id)?;
        let preview = self.preview_restore(request.context, request.user_id, request.backup_id)?;
        if !preview.compatible {
            return Err(StoreError::Conflict(format!(
                "backup schema {} is incompatible with {}",
                preview.schema_version, LATEST_SCHEMA
            )));
        }
        if preview.sha256 != request.expected_sha256 {
            return Err(StoreError::Conflict(
                "backup changed after restore preview".into(),
            ));
        }
        let source_record = self.backup_record(request.context.tenant_id, request.backup_id)?;
        let safety_operation = OperationId::new();
        let safety_result = self.create_verified_backup(BackupCreateRequest {
            context: request.context,
            user_id: request.user_id,
            operation_id: safety_operation,
            backup_type: "PRE_RESTORE".into(),
            destination_directory: safety_directory.clone(),
            app_version: request.app_version.clone(),
            now: request.now,
        })?;
        let safety_record =
            self.backup_record(request.context.tenant_id, safety_result.backup_id)?;

        let restore_id = request.operation_id.0;
        let result = RestoreResult {
            restore_id,
            backup_id: request.backup_id,
            pre_restore_backup_id: safety_result.backup_id,
            restored_sha256: preview.sha256.clone(),
            completed_at: request.now.to_rfc3339(),
        };

        // Build a complete, verified restore image before touching the live
        // database. If the process stops during the final SQLite backup, the
        // destination transaction leaves either the old image or this image,
        // which already contains its idempotency and audit evidence.
        let staged_path = safety_directory.join(format!(".{restore_id}.restore-stage"));
        if staged_path.exists() {
            fs::remove_file(&staged_path)?;
        }
        let stage_result = (|| -> Result<(), StoreError> {
            let source = Connection::open(&source_record.storage_path)?;
            let mut staged = Connection::open(&staged_path)?;
            {
                let backup = Backup::new(&source, &mut staged)?;
                backup.run_to_completion(128, Duration::from_millis(5), None)?;
            }
            drop(source);
            {
                let tx = staged.transaction_with_behavior(TransactionBehavior::Immediate)?;
                Self::insert_backup_record_evidence(&tx, &source_record)?;
                Self::insert_backup_record_evidence(&tx, &safety_record)?;
                tx.execute(
                    "INSERT INTO restore_runs(id,tenant_id,backup_id,pre_restore_backup_id,state,compatibility_json,authorized_by_user_id,started_at,completed_at,result_json,branch_id,origin_device_id,operation_id) VALUES(?1,?2,?3,?4,'SUCCEEDED',?5,?6,?7,?7,?8,?9,?10,?11)",
                    params![
                        restore_id.to_string(),
                        request.context.tenant_id.to_string(),
                        request.backup_id.to_string(),
                        safety_result.backup_id.to_string(),
                        serde_json::json!({"compatible":true,"schema_version":preview.schema_version,"sha256":preview.sha256}).to_string(),
                        request.user_id.to_string(),
                        request.now.to_rfc3339(),
                        serde_json::to_string(&result)?,
                        request.context.branch_id.to_string(),
                        request.context.device_id.to_string(),
                        request.operation_id.to_string(),
                    ],
                )?;
                Self::append_backup_event(
                    &tx,
                    request.context,
                    request.user_id,
                    Some(request.backup_id),
                    Some(restore_id),
                    "RESTORE_SUCCEEDED",
                    &serde_json::json!({"pre_restore_backup_id":safety_result.backup_id,"restored_sha256":result.restored_sha256}),
                    request.now,
                )?;
                Self::record_backup_operation(
                    &tx,
                    request.context.tenant_id,
                    request.operation_id,
                    "RESTORE",
                    &digest,
                    &result,
                    request.now,
                )?;
                Self::append_audit(
                    &tx,
                    request.context.tenant_id,
                    request.context.device_id,
                    request.user_id,
                    "BACKUP_RESTORED",
                    "restore_run",
                    &restore_id.to_string(),
                    &serde_json::to_string(&result)?,
                    request.now,
                )?;
                tx.commit()?;
            }
            drop(staged);
            Self::verify_backup_database(&staged_path, request.context, request.user_id, true)?;
            let staged_source = Connection::open(&staged_path)?;
            {
                let backup = Backup::new(&staged_source, &mut self.conn)?;
                backup.run_to_completion(128, Duration::from_millis(5), None)?;
            }
            drop(staged_source);
            Ok(())
        })();
        if stage_result.is_err() && staged_path.exists() {
            let _ = fs::remove_file(&staged_path);
        }
        stage_result?;
        let _ = fs::remove_file(&staged_path);
        self.migrate()?;
        self.validate_local_session(request.context, request.user_id)?;
        self.assert_owner(request.context, request.user_id)?;
        Ok(result)
    }

    fn prepare_backup_directory(path: &Path) -> Result<PathBuf, StoreError> {
        if path.as_os_str().is_empty() {
            return Err(StoreError::Validation(
                "backup directory is required".into(),
            ));
        }
        fs::create_dir_all(path)?;
        Ok(fs::canonicalize(path)?)
    }

    fn normalize_backup_type(value: &str) -> Result<String, StoreError> {
        let normalized = value.trim().to_ascii_uppercase();
        if normalized.is_empty()
            || normalized.len() > 32
            || !normalized
                .chars()
                .all(|character| character.is_ascii_uppercase() || character == '_')
        {
            return Err(StoreError::Validation(
                "backup type must use uppercase letters or underscores".into(),
            ));
        }
        Ok(normalized)
    }

    fn hash_file(path: &Path) -> Result<(String, u64), StoreError> {
        let mut file = File::open(path)?;
        let mut hasher = Sha256::new();
        let mut size = 0_u64;
        let mut buffer = [0_u8; 64 * 1024];
        loop {
            let read = file.read(&mut buffer)?;
            if read == 0 {
                break;
            }
            hasher.update(&buffer[..read]);
            size = size
                .checked_add(read as u64)
                .ok_or_else(|| StoreError::Validation("backup size overflow".into()))?;
        }
        Ok((hex::encode(hasher.finalize()), size))
    }

    fn verify_backup_database(
        path: &Path,
        context: LocalTerminalContext,
        user: UserId,
        require_restore_authority: bool,
    ) -> Result<(), StoreError> {
        let connection = Connection::open_with_flags(
            path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        let integrity: String =
            connection.query_row("PRAGMA integrity_check", [], |row| row.get(0))?;
        if integrity != "ok" {
            return Err(StoreError::Conflict(format!(
                "backup integrity check failed: {integrity}"
            )));
        }
        let foreign_key_errors: i64 =
            connection.query_row("SELECT COUNT(*) FROM pragma_foreign_key_check", [], |row| {
                row.get(0)
            })?;
        if foreign_key_errors != 0 {
            return Err(StoreError::Conflict(
                "backup foreign-key check failed".into(),
            ));
        }
        let scoped: Option<i32> = connection
            .query_row(
                "SELECT 1 FROM local_terminal_binding binding JOIN users user ON user.id=?5 AND user.tenant_id=binding.tenant_id AND user.status='ACTIVE' JOIN devices device ON device.id=binding.device_id AND device.tenant_id=binding.tenant_id AND device.branch_id=binding.branch_id WHERE binding.singleton=1 AND binding.tenant_id=?1 AND binding.branch_id=?2 AND binding.device_id=?3 AND binding.register_id=?4",
                params![
                    context.tenant_id.to_string(),
                    context.branch_id.to_string(),
                    context.device_id.to_string(),
                    context.register_id.to_string(),
                    user.to_string(),
                ],
                |row| row.get(0),
            )
            .optional()?;
        if scoped.is_none() {
            return Err(StoreError::Conflict(
                "backup tenant or terminal identity is incompatible".into(),
            ));
        }
        if require_restore_authority {
            let owner: Option<i32> = connection
                .query_row(
                    "SELECT 1 FROM user_roles ur JOIN roles role ON role.id=ur.role_id WHERE ur.user_id=?1 AND role.tenant_id=?2 AND lower(role.name)='owner' AND (ur.branch_id IS NULL OR ur.branch_id=?3) LIMIT 1",
                    params![user.to_string(), context.tenant_id.to_string(), context.branch_id.to_string()],
                    |row| row.get(0),
                )
                .optional()?;
            if owner.is_none() {
                return Err(StoreError::Conflict(
                    "backup would remove the authorizing owner identity".into(),
                ));
            }
        }
        Ok(())
    }

    fn assert_owner(&self, context: LocalTerminalContext, user: UserId) -> Result<(), StoreError> {
        let owner: Option<i32> = self
            .conn
            .query_row(
                "SELECT 1 FROM user_roles ur JOIN roles role ON role.id=ur.role_id WHERE ur.user_id=?1 AND role.tenant_id=?2 AND lower(role.name)='owner' AND (ur.branch_id IS NULL OR ur.branch_id=?3) LIMIT 1",
                params![user.to_string(), context.tenant_id.to_string(), context.branch_id.to_string()],
                |row| row.get(0),
            )
            .optional()?;
        if owner.is_none() {
            return Err(StoreError::Authorization("owner role required"));
        }
        Ok(())
    }

    fn backup_record(
        &self,
        tenant: TenantId,
        backup_id: Uuid,
    ) -> Result<BackupRecordEvidence, StoreError> {
        self.conn
            .query_row(
                "SELECT id,tenant_id,branch_id,backup_type,storage_path,sha256,byte_size,schema_version,app_version,origin_device_id,created_by_user_id,operation_id,created_at,verified_at FROM backup_records WHERE id=?1 AND tenant_id=?2 AND state='SUCCEEDED' AND integrity_state='VERIFIED'",
                params![backup_id.to_string(), tenant.to_string()],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?, row.get::<_, String>(4)?, row.get::<_, String>(5)?,
                        row.get::<_, i64>(6)?, row.get::<_, String>(7)?, row.get::<_, String>(8)?,
                        row.get::<_, String>(9)?, row.get::<_, String>(10)?, row.get::<_, String>(11)?,
                        row.get::<_, String>(12)?, row.get::<_, String>(13)?,
                    ))
                },
            )
            .optional()?
            .ok_or(StoreError::NotFound("verified backup"))
            .and_then(|row| {
                Ok(BackupRecordEvidence {
                    id: Uuid::parse_str(&row.0).map_err(|_| StoreError::Validation("invalid backup id".into()))?,
                    tenant_id: TenantId(Uuid::parse_str(&row.1).map_err(|_| StoreError::Validation("invalid backup tenant".into()))?),
                    branch_id: BranchId(Uuid::parse_str(&row.2).map_err(|_| StoreError::Validation("invalid backup branch".into()))?),
                    backup_type: row.3,
                    storage_path: PathBuf::from(row.4),
                    sha256: row.5,
                    byte_size: u64::try_from(row.6).map_err(|_| StoreError::Validation("invalid backup size".into()))?,
                    schema_version: row.7,
                    app_version: row.8,
                    origin_device_id: DeviceId(Uuid::parse_str(&row.9).map_err(|_| StoreError::Validation("invalid backup device".into()))?),
                    created_by_user_id: UserId(Uuid::parse_str(&row.10).map_err(|_| StoreError::Validation("invalid backup user".into()))?),
                    operation_id: OperationId(Uuid::parse_str(&row.11).map_err(|_| StoreError::Validation("invalid backup operation".into()))?),
                    created_at: row.12,
                    verified_at: row.13,
                })
            })
    }

    fn insert_backup_record_evidence(
        tx: &Transaction<'_>,
        record: &BackupRecordEvidence,
    ) -> Result<(), StoreError> {
        tx.execute(
            "INSERT OR IGNORE INTO backup_records(id,tenant_id,branch_id,backup_type,storage_path,sha256,byte_size,schema_version,app_version,state,integrity_state,created_at,verified_at,origin_device_id,created_by_user_id,operation_id) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,'SUCCEEDED','VERIFIED',?10,?11,?12,?13,?14)",
            params![record.id.to_string(),record.tenant_id.to_string(),record.branch_id.to_string(),record.backup_type,record.storage_path.to_string_lossy(),record.sha256,i64::try_from(record.byte_size).map_err(|_|StoreError::Validation("backup is too large".into()))?,record.schema_version,record.app_version,record.created_at,record.verified_at,record.origin_device_id.to_string(),record.created_by_user_id.to_string(),record.operation_id.to_string()],
        )?;
        Ok(())
    }

    fn load_backup_operation<T: DeserializeOwned>(
        &self,
        tenant: TenantId,
        operation: OperationId,
        action: &str,
        digest: &str,
    ) -> Result<Option<T>, StoreError> {
        let row: Option<(String, String, String)> = self
            .conn
            .query_row(
                "SELECT action,request_sha256,result_json FROM backup_operation_results WHERE tenant_id=?1 AND operation_id=?2",
                params![tenant.to_string(), operation.to_string()],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()?;
        match row {
            None => Ok(None),
            Some((stored_action, stored_digest, result))
                if stored_action == action && stored_digest == digest =>
            {
                Ok(Some(serde_json::from_str(&result)?))
            }
            Some(_) => Err(StoreError::Conflict(
                "backup operation id was reused with a different request".into(),
            )),
        }
    }

    fn record_backup_operation<T: Serialize>(
        tx: &Transaction<'_>,
        tenant: TenantId,
        operation: OperationId,
        action: &str,
        digest: &str,
        result: &T,
        now: DateTime<Utc>,
    ) -> Result<(), StoreError> {
        tx.execute(
            "INSERT INTO backup_operation_results(tenant_id,operation_id,action,request_sha256,result_json,committed_at) VALUES(?1,?2,?3,?4,?5,?6)",
            params![tenant.to_string(), operation.to_string(), action, digest, serde_json::to_string(result)?, now.to_rfc3339()],
        )?;
        Ok(())
    }

    fn load_backup_schedule_operation<T: DeserializeOwned>(
        &self,
        tenant: TenantId,
        operation: OperationId,
        action: &str,
        digest: &str,
    ) -> Result<Option<T>, StoreError> {
        let row: Option<(String, String, String)> = self
            .conn
            .query_row(
                "SELECT action,request_sha256,result_json FROM backup_schedule_operation_results WHERE tenant_id=?1 AND operation_id=?2",
                params![tenant.to_string(), operation.to_string()],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()?;
        match row {
            None => Ok(None),
            Some((stored_action, stored_digest, result))
                if stored_action == action && stored_digest == digest =>
            {
                Ok(Some(serde_json::from_str(&result)?))
            }
            Some(_) => Err(StoreError::Conflict(
                "backup schedule operation id was reused with a different request".into(),
            )),
        }
    }

    fn record_backup_schedule_operation<T: Serialize>(
        tx: &Transaction<'_>,
        tenant: TenantId,
        operation: OperationId,
        action: &str,
        digest: &str,
        result: &T,
        now: DateTime<Utc>,
    ) -> Result<(), StoreError> {
        tx.execute(
            "INSERT INTO backup_schedule_operation_results(tenant_id,operation_id,action,request_sha256,result_json,committed_at) VALUES(?1,?2,?3,?4,?5,?6)",
            params![tenant.to_string(),operation.to_string(),action,digest,serde_json::to_string(result)?,now.to_rfc3339()],
        )?;
        Ok(())
    }

    fn load_backup_retention_operation(
        &self,
        tenant: TenantId,
        operation: OperationId,
        digest: &str,
    ) -> Result<Option<BackupRetentionResult>, StoreError> {
        let row: Option<(String, String)> = self
            .conn
            .query_row(
                "SELECT request_sha256,result_json FROM backup_retention_operation_results WHERE tenant_id=?1 AND operation_id=?2",
                params![tenant.to_string(),operation.to_string()],
                |row| Ok((row.get(0)?,row.get(1)?)),
            )
            .optional()?;
        match row {
            None => Ok(None),
            Some((stored_digest, result)) if stored_digest == digest => {
                Ok(Some(serde_json::from_str(&result)?))
            }
            Some(_) => Err(StoreError::Conflict(
                "backup retention operation id was reused with a different request".into(),
            )),
        }
    }

    fn record_backup_retention_operation(
        tx: &Transaction<'_>,
        tenant: TenantId,
        operation: OperationId,
        digest: &str,
        result: &BackupRetentionResult,
        now: DateTime<Utc>,
    ) -> Result<(), StoreError> {
        tx.execute(
            "INSERT INTO backup_retention_operation_results(tenant_id,operation_id,request_sha256,result_json,committed_at) VALUES(?1,?2,?3,?4,?5)",
            params![tenant.to_string(),operation.to_string(),digest,serde_json::to_string(result)?,now.to_rfc3339()],
        )?;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn append_backup_schedule_event(
        tx: &Transaction<'_>,
        context: LocalTerminalContext,
        user: UserId,
        operation: OperationId,
        schedule_id: Uuid,
        event_type: &str,
        previous_state: Option<&str>,
        new_state: &str,
        evidence: &serde_json::Value,
        now: DateTime<Utc>,
    ) -> Result<(), StoreError> {
        tx.execute(
            "INSERT INTO backup_schedule_events(id,tenant_id,branch_id,schedule_id,operation_id,event_type,previous_state,new_state,device_id,user_id,evidence_json,created_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",
            params![Uuid::new_v4().to_string(),context.tenant_id.to_string(),context.branch_id.to_string(),schedule_id.to_string(),operation.to_string(),event_type,previous_state,new_state,context.device_id.to_string(),user.to_string(),evidence.to_string(),now.to_rfc3339()],
        )?;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn append_backup_event(
        tx: &Transaction<'_>,
        context: LocalTerminalContext,
        user: UserId,
        backup_id: Option<Uuid>,
        restore_run_id: Option<Uuid>,
        event_type: &str,
        evidence: &serde_json::Value,
        now: DateTime<Utc>,
    ) -> Result<(), StoreError> {
        tx.execute(
            "INSERT INTO backup_events(id,tenant_id,branch_id,backup_id,restore_run_id,event_type,device_id,user_id,evidence_json,created_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
            params![Uuid::new_v4().to_string(),context.tenant_id.to_string(),context.branch_id.to_string(),backup_id.map(|value|value.to_string()),restore_run_id.map(|value|value.to_string()),event_type,context.device_id.to_string(),user.to_string(),evidence.to_string(),now.to_rfc3339()],
        )?;
        Ok(())
    }
}
