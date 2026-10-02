#![allow(
    clippy::too_many_arguments,
    reason = "alert commands retain trusted scope, retry identity, evidence, and event time"
)]

use super::*;

impl Store {
    pub fn create_operational_alert(
        &mut self,
        context: LocalTerminalContext,
        user: UserId,
        operation_id: OperationId,
        alert_id: Uuid,
        severity: &str,
        alert_type: &str,
        title: &str,
        entity_type: Option<&str>,
        entity_id: Option<&str>,
        details_json: &str,
        now: DateTime<Utc>,
    ) -> Result<OperationalAlertResult, StoreError> {
        self.validate_local_session(context, user)?;
        if !matches!(severity, "LOW" | "MEDIUM" | "HIGH" | "CRITICAL") {
            return Err(StoreError::Validation("invalid alert severity".into()));
        }
        if !matches!(
            alert_type,
            "BACKUP_FAILURE"
                | "TERMINAL_OFFLINE"
                | "SYNC_DELAY"
                | "CASH_VARIANCE"
                | "NEGATIVE_STOCK"
                | "EXPIRY"
                | "LOW_STOCK"
                | "OVERDUE_SUPPLIER_INVOICE"
                | "OVERDUE_CUSTOMER_CREDIT"
                | "UNKNOWN_BARCODE"
                | "SETTLEMENT_DISCREPANCY"
                | "SECURITY_EVENT"
                | "OTHER"
        ) {
            return Err(StoreError::Validation("invalid alert type".into()));
        }
        let title = title.trim();
        if title.is_empty() {
            return Err(StoreError::Validation("alert title is required".into()));
        }
        let entity_type = entity_type.map(str::trim).filter(|value| !value.is_empty());
        let entity_id = entity_id.map(str::trim).filter(|value| !value.is_empty());
        if entity_type.is_some() != entity_id.is_some() {
            return Err(StoreError::Validation(
                "alert entity type and ID must be supplied together".into(),
            ));
        }
        let details: serde_json::Value = serde_json::from_str(details_json)?;
        if !details.is_object() {
            return Err(StoreError::Validation(
                "alert details must be a JSON object".into(),
            ));
        }
        let details_json = serde_json::to_string(&details)?;
        let digest = sha256_hex(&serde_json::to_vec(&(
            alert_id,
            severity,
            alert_type,
            title,
            entity_type,
            entity_id,
            &details_json,
            context.branch_id,
        ))?);
        if let Some(result) =
            self.load_alert_operation(operation_id, "CREATE", &digest, context.tenant_id)?
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
            "alert.create",
        )?;
        tx.execute(
            "INSERT INTO operational_alerts(id,tenant_id,branch_id,severity,alert_type,status,entity_type,entity_id,title,details_json,created_at) VALUES(?1,?2,?3,?4,?5,'NEW',?6,?7,?8,?9,?10)",
            params![alert_id.to_string(), context.tenant_id.to_string(), context.branch_id.to_string(), severity, alert_type, entity_type, entity_id, title, details_json, now.to_rfc3339()],
        )?;
        Self::append_operational_alert_event(
            &tx,
            context,
            user,
            operation_id,
            alert_id,
            "CREATED",
            None,
            "NEW",
            None,
            None,
            now,
        )?;
        let result = OperationalAlertResult {
            alert_id,
            status: "NEW".into(),
            severity: severity.into(),
            assigned_user_id: None,
        };
        Self::record_alert_operation(
            &tx,
            context.tenant_id,
            operation_id,
            "CREATE",
            &digest,
            &result,
            now,
        )?;
        Self::append_audit(
            &tx,
            context.tenant_id,
            context.device_id,
            user,
            "OPERATIONAL_ALERT_CREATED",
            "operational_alert",
            &alert_id.to_string(),
            &serde_json::to_string(&result)?,
            now,
        )?;
        tx.commit()?;
        Ok(result)
    }

    pub fn transition_operational_alert(
        &mut self,
        context: LocalTerminalContext,
        user: UserId,
        operation_id: OperationId,
        alert_id: Uuid,
        new_status: &str,
        assigned_user_id: Option<UserId>,
        note: Option<&str>,
        now: DateTime<Utc>,
    ) -> Result<OperationalAlertResult, StoreError> {
        self.validate_local_session(context, user)?;
        if !matches!(
            new_status,
            "ACKNOWLEDGED" | "IN_PROGRESS" | "RESOLVED" | "DISMISSED"
        ) {
            return Err(StoreError::Validation("invalid alert status".into()));
        }
        let note = note.map(str::trim).filter(|value| !value.is_empty());
        if matches!(new_status, "RESOLVED" | "DISMISSED") && note.is_none() {
            return Err(StoreError::Validation(
                "closing an alert requires a note".into(),
            ));
        }
        let digest = sha256_hex(&serde_json::to_vec(&(
            alert_id,
            new_status,
            assigned_user_id,
            note,
        ))?);
        if let Some(result) =
            self.load_alert_operation(operation_id, "TRANSITION", &digest, context.tenant_id)?
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
            "alert.manage",
        )?;
        let (previous_status, severity, current_assignee): (String, String, Option<String>) = tx
            .query_row(
                "SELECT status,severity,assigned_user_id FROM operational_alerts WHERE id=?1 AND tenant_id=?2 AND branch_id=?3",
                params![alert_id.to_string(), context.tenant_id.to_string(), context.branch_id.to_string()],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()?
            .ok_or(StoreError::NotFound("operational alert"))?;
        let legal = matches!(
            (previous_status.as_str(), new_status),
            ("NEW", "ACKNOWLEDGED")
                | ("ACKNOWLEDGED", "IN_PROGRESS")
                | ("ACKNOWLEDGED", "DISMISSED")
                | ("IN_PROGRESS", "RESOLVED")
                | ("IN_PROGRESS", "DISMISSED")
        );
        if !legal {
            return Err(StoreError::Conflict(format!(
                "illegal alert transition {previous_status} -> {new_status}"
            )));
        }
        let assigned_user_id = assigned_user_id.or_else(|| {
            current_assignee
                .and_then(|value| Uuid::parse_str(&value).ok())
                .map(UserId)
        });
        Self::append_operational_alert_event(
            &tx,
            context,
            user,
            operation_id,
            alert_id,
            new_status,
            Some(&previous_status),
            new_status,
            assigned_user_id,
            note,
            now,
        )?;
        let resolved_at = matches!(new_status, "RESOLVED" | "DISMISSED").then(|| now.to_rfc3339());
        tx.execute(
            "UPDATE operational_alerts SET status=?1,assigned_user_id=?2,resolved_at=?3 WHERE id=?4 AND tenant_id=?5 AND branch_id=?6",
            params![new_status, assigned_user_id.map(|value| value.to_string()), resolved_at, alert_id.to_string(), context.tenant_id.to_string(), context.branch_id.to_string()],
        )?;
        let result = OperationalAlertResult {
            alert_id,
            status: new_status.into(),
            severity,
            assigned_user_id,
        };
        Self::record_alert_operation(
            &tx,
            context.tenant_id,
            operation_id,
            "TRANSITION",
            &digest,
            &result,
            now,
        )?;
        Self::append_audit(
            &tx,
            context.tenant_id,
            context.device_id,
            user,
            "OPERATIONAL_ALERT_TRANSITIONED",
            "operational_alert",
            &alert_id.to_string(),
            &serde_json::to_string(&result)?,
            now,
        )?;
        tx.commit()?;
        Ok(result)
    }

    pub fn active_operational_alerts(
        &self,
        context: LocalTerminalContext,
        user: UserId,
        limit: usize,
    ) -> Result<Vec<OperationalAlertSummary>, StoreError> {
        self.validate_local_session(context, user)?;
        Self::assert_permission_conn(
            &self.conn,
            context.tenant_id,
            context.branch_id,
            user,
            "alert.view",
        )?;
        let limit = limit.clamp(1, 500) as i64;
        let mut statement = self.conn.prepare(
            "SELECT id,severity,alert_type,status,title,assigned_user_id,created_at FROM operational_alerts WHERE tenant_id=?1 AND branch_id=?2 AND status NOT IN ('RESOLVED','DISMISSED') ORDER BY CASE severity WHEN 'CRITICAL' THEN 0 WHEN 'HIGH' THEN 1 WHEN 'MEDIUM' THEN 2 ELSE 3 END,created_at,id LIMIT ?3",
        )?;
        let rows = statement
            .query_map(
                params![
                    context.tenant_id.to_string(),
                    context.branch_id.to_string(),
                    limit
                ],
                |row| {
                    let id: String = row.get(0)?;
                    let assignee: Option<String> = row.get(5)?;
                    Ok((
                        id,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        assignee,
                        row.get(6)?,
                    ))
                },
            )?
            .collect::<Result<Vec<_>, _>>()?;
        rows.into_iter()
            .map(
                |(id, severity, alert_type, status, title, assignee, created_at)| {
                    Ok(OperationalAlertSummary {
                        alert_id: Uuid::parse_str(&id)
                            .map_err(|_| StoreError::Validation("invalid alert identity".into()))?,
                        severity,
                        alert_type,
                        status,
                        title,
                        assigned_user_id: assignee
                            .map(|value| {
                                Uuid::parse_str(&value).map(UserId).map_err(|_| {
                                    StoreError::Validation("invalid alert assignee".into())
                                })
                            })
                            .transpose()?,
                        created_at,
                    })
                },
            )
            .collect()
    }

    fn load_alert_operation<T: DeserializeOwned>(
        &self,
        operation_id: OperationId,
        action: &str,
        digest: &str,
        tenant: TenantId,
    ) -> Result<Option<T>, StoreError> {
        let stored: Option<(String, String, String)> = self.conn.query_row(
            "SELECT action,request_sha256,result_json FROM alert_operation_results WHERE tenant_id=?1 AND operation_id=?2",
            params![tenant.to_string(), operation_id.to_string()],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        ).optional()?;
        match stored {
            None => Ok(None),
            Some((stored_action, stored_digest, json))
                if stored_action == action && stored_digest == digest =>
            {
                Ok(Some(serde_json::from_str(&json)?))
            }
            Some(_) => Err(StoreError::Conflict(
                "operation ID reused with different action or payload".into(),
            )),
        }
    }

    fn record_alert_operation<T: Serialize>(
        tx: &Transaction<'_>,
        tenant: TenantId,
        operation_id: OperationId,
        action: &str,
        digest: &str,
        result: &T,
        now: DateTime<Utc>,
    ) -> Result<(), StoreError> {
        tx.execute(
            "INSERT INTO alert_operation_results(tenant_id,operation_id,action,request_sha256,result_json,committed_at) VALUES(?1,?2,?3,?4,?5,?6)",
            params![tenant.to_string(), operation_id.to_string(), action, digest, serde_json::to_string(result)?, now.to_rfc3339()],
        )?;
        Ok(())
    }

    fn append_operational_alert_event(
        tx: &Transaction<'_>,
        context: LocalTerminalContext,
        user: UserId,
        operation_id: OperationId,
        alert_id: Uuid,
        event_type: &str,
        previous_status: Option<&str>,
        new_status: &str,
        assigned_user_id: Option<UserId>,
        note: Option<&str>,
        now: DateTime<Utc>,
    ) -> Result<(), StoreError> {
        tx.execute(
            "INSERT INTO operational_alert_events(id,tenant_id,alert_id,branch_id,operation_id,event_type,previous_status,new_status,assigned_user_id,device_id,entered_by_user_id,note,created_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)",
            params![Uuid::new_v4().to_string(), context.tenant_id.to_string(), alert_id.to_string(), context.branch_id.to_string(), operation_id.to_string(), event_type, previous_status, new_status, assigned_user_id.map(|value| value.to_string()), context.device_id.to_string(), user.to_string(), note, now.to_rfc3339()],
        )?;
        Ok(())
    }
}
