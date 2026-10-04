#![allow(
    clippy::too_many_arguments,
    reason = "alert commands retain trusted scope, retry identity, evidence, and event time"
)]

use super::*;

impl Store {
    #[allow(
        clippy::too_many_arguments,
        reason = "automatic alert evidence retains authoritative scope, actor, source entity, and event time"
    )]
    pub(super) fn append_automatic_operational_alert(
        tx: &Transaction<'_>,
        tenant_id: TenantId,
        branch_id: BranchId,
        device_id: DeviceId,
        user_id: UserId,
        severity: &str,
        alert_type: &str,
        title: &str,
        entity_type: &str,
        entity_id: &str,
        details: &serde_json::Value,
        now: DateTime<Utc>,
    ) -> Result<Option<Uuid>, StoreError> {
        let existing: Option<String> = tx
            .query_row(
                "SELECT id FROM operational_alerts WHERE tenant_id=?1 AND branch_id=?2 AND alert_type=?3 AND entity_type=?4 AND entity_id=?5 AND status NOT IN ('RESOLVED','DISMISSED') LIMIT 1",
                params![tenant_id.to_string(), branch_id.to_string(), alert_type, entity_type, entity_id],
                |row| row.get(0),
            )
            .optional()?;
        if existing.is_some() {
            return Ok(None);
        }
        let details_json = serde_json::to_string(details)?;
        let alert_id = Uuid::new_v4();
        let event_operation_id = OperationId::new();
        tx.execute(
            "INSERT INTO operational_alerts(id,tenant_id,branch_id,severity,alert_type,status,entity_type,entity_id,title,details_json,created_at) VALUES(?1,?2,?3,?4,?5,'NEW',?6,?7,?8,?9,?10)",
            params![alert_id.to_string(), tenant_id.to_string(), branch_id.to_string(), severity, alert_type, entity_type, entity_id, title, details_json, now.to_rfc3339()],
        )?;
        tx.execute(
            "INSERT INTO operational_alert_events(id,tenant_id,alert_id,branch_id,operation_id,event_type,previous_status,new_status,device_id,entered_by_user_id,note,created_at) VALUES(?1,?2,?3,?4,?5,'CREATED',NULL,'NEW',?6,?7,'Automatically produced from committed domain evidence',?8)",
            params![Uuid::new_v4().to_string(), tenant_id.to_string(), alert_id.to_string(), branch_id.to_string(), event_operation_id.to_string(), device_id.to_string(), user_id.to_string(), now.to_rfc3339()],
        )?;
        let audit_payload = serde_json::json!({
            "alert_id": alert_id,
            "severity": severity,
            "alert_type": alert_type,
            "entity_type": entity_type,
            "entity_id": entity_id,
            "automatic": true
        });
        Self::append_audit(
            tx,
            tenant_id,
            device_id,
            user_id,
            "OPERATIONAL_ALERT_AUTOMATICALLY_CREATED",
            "operational_alert",
            &alert_id.to_string(),
            &serde_json::to_string(&audit_payload)?,
            now,
        )?;
        Ok(Some(alert_id))
    }

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

    pub fn evaluate_operational_alerts(
        &mut self,
        context: LocalTerminalContext,
        user: UserId,
        operation_id: OperationId,
        policy: OperationalAlertEvaluationPolicy,
        now: DateTime<Utc>,
    ) -> Result<OperationalAlertEvaluationResult, StoreError> {
        self.validate_local_session(context, user)?;
        if !(1..=525_600).contains(&policy.sync_delay_minutes)
            || !(1..=525_600).contains(&policy.terminal_offline_minutes)
            || !(1..=3_650).contains(&policy.expiry_warning_days)
            || !(1..=100).contains(&policy.authentication_failure_threshold)
        {
            return Err(StoreError::Validation(
                "invalid operational alert evaluation policy".into(),
            ));
        }
        let digest = sha256_hex(&serde_json::to_vec(&policy)?);
        if let Some(result) = self.load_alert_operation(
            operation_id,
            "EVALUATE",
            &digest,
            context.tenant_id,
        )? {
            return Ok(result);
        }
        let sync_cutoff = now
            .checked_sub_signed(chrono::Duration::minutes(policy.sync_delay_minutes))
            .ok_or_else(|| StoreError::Validation("sync delay cutoff overflow".into()))?;
        let terminal_cutoff = now
            .checked_sub_signed(chrono::Duration::minutes(
                policy.terminal_offline_minutes,
            ))
            .ok_or_else(|| StoreError::Validation("terminal cutoff overflow".into()))?;
        let today = now.date_naive();
        let expiry_horizon = today
            .checked_add_days(chrono::Days::new(policy.expiry_warning_days as u64))
            .ok_or_else(|| StoreError::Validation("expiry horizon overflow".into()))?;
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

        let low_stock = {
            let mut statement = tx.prepare(
                "SELECT p.id,p.name,COALESCE(SUM(sl.quantity_milli),0),rp.reorder_point_milli FROM reorder_policies rp JOIN products p ON p.id=rp.product_id AND p.tenant_id=rp.tenant_id LEFT JOIN stock_levels sl ON sl.tenant_id=rp.tenant_id AND sl.branch_id=rp.branch_id AND sl.product_id=rp.product_id WHERE rp.tenant_id=?1 AND rp.branch_id=?2 AND rp.active=1 AND rp.reorder_point_milli IS NOT NULL AND p.status='ACTIVE' GROUP BY p.id,p.name,rp.reorder_point_milli HAVING COALESCE(SUM(sl.quantity_milli),0)<=rp.reorder_point_milli",
            )?;
            statement
                .query_map(
                    params![context.tenant_id.to_string(), context.branch_id.to_string()],
                    |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, i64>(2)?,
                            row.get::<_, i64>(3)?,
                        ))
                    },
                )?
                .collect::<Result<Vec<_>, _>>()?
        };
        let expiring_lots = {
            let mut statement = tx.prepare(
                "SELECT il.id,p.name,il.expires_on,SUM(lb.quantity_milli) FROM inventory_lots il JOIN products p ON p.id=il.product_id AND p.tenant_id=il.tenant_id JOIN lot_balances lb ON lb.lot_id=il.id AND lb.tenant_id=il.tenant_id WHERE il.tenant_id=?1 AND lb.branch_id=?2 AND il.status='ACTIVE' AND il.expires_on IS NOT NULL AND date(il.expires_on)<=date(?3) GROUP BY il.id,p.name,il.expires_on HAVING SUM(lb.quantity_milli)>0",
            )?;
            statement
                .query_map(
                    params![
                        context.tenant_id.to_string(),
                        context.branch_id.to_string(),
                        expiry_horizon.to_string()
                    ],
                    |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, String>(2)?,
                            row.get::<_, i64>(3)?,
                        ))
                    },
                )?
                .collect::<Result<Vec<_>, _>>()?
        };
        let delayed_sync = {
            let mut statement = tx.prepare(
                "SELECT device_id,MIN(updated_at),COUNT(*) FROM sync_queue WHERE tenant_id=?1 AND branch_id=?2 AND state IN ('PENDING','SENDING','RETRYING') AND datetime(updated_at)<datetime(?3) GROUP BY device_id",
            )?;
            statement
                .query_map(
                    params![
                        context.tenant_id.to_string(),
                        context.branch_id.to_string(),
                        sync_cutoff.to_rfc3339()
                    ],
                    |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, i64>(2)?,
                        ))
                    },
                )?
                .collect::<Result<Vec<_>, _>>()?
        };
        let offline_terminals = {
            let mut statement = tx.prepare(
                "SELECT id,label,last_heartbeat_at,created_at FROM devices WHERE tenant_id=?1 AND branch_id=?2 AND status='ACTIVE' AND id!=?3 AND datetime(COALESCE(last_heartbeat_at,created_at))<datetime(?4)",
            )?;
            statement
                .query_map(
                    params![
                        context.tenant_id.to_string(),
                        context.branch_id.to_string(),
                        context.device_id.to_string(),
                        terminal_cutoff.to_rfc3339()
                    ],
                    |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, Option<String>>(2)?,
                            row.get::<_, String>(3)?,
                        ))
                    },
                )?
                .collect::<Result<Vec<_>, _>>()?
        };
        let overdue_invoices = {
            let mut statement = tx.prepare(
                "SELECT si.id,si.invoice_number,s.name,si.due_date,si.total_fils-si.amount_paid_fils FROM supplier_invoices si JOIN suppliers s ON s.id=si.supplier_id AND s.tenant_id=si.tenant_id WHERE si.tenant_id=?1 AND si.branch_id=?2 AND si.status IN ('OPEN','PARTIALLY_PAID') AND si.due_date IS NOT NULL AND date(si.due_date)<date(?3) AND si.total_fils>si.amount_paid_fils",
            )?;
            statement
                .query_map(
                    params![
                        context.tenant_id.to_string(),
                        context.branch_id.to_string(),
                        today.to_string()
                    ],
                    |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, String>(2)?,
                            row.get::<_, String>(3)?,
                            row.get::<_, i64>(4)?,
                        ))
                    },
                )?
                .collect::<Result<Vec<_>, _>>()?
        };
        let failed_backups = {
            let mut statement = tx.prepare(
                "SELECT id,backup_type,state,integrity_state,created_at FROM backup_records WHERE tenant_id=?1 AND (branch_id=?2 OR branch_id IS NULL) AND (state='FAILED' OR integrity_state IN ('FAILED','CORRUPT'))",
            )?;
            statement
                .query_map(
                    params![context.tenant_id.to_string(), context.branch_id.to_string()],
                    |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, String>(2)?,
                            row.get::<_, Option<String>>(3)?,
                            row.get::<_, String>(4)?,
                        ))
                    },
                )?
                .collect::<Result<Vec<_>, _>>()?
        };
        let authentication_risks = {
            let mut statement = tx.prepare(
                "SELECT u.id,u.display_name,u.failed_attempts,u.status FROM users u WHERE u.tenant_id=?1 AND u.failed_attempts>=?2 AND u.status IN ('ACTIVE','LOCKED') AND EXISTS (SELECT 1 FROM user_roles ur JOIN roles r ON r.id=ur.role_id WHERE ur.user_id=u.id AND r.tenant_id=u.tenant_id AND (ur.branch_id IS NULL OR ur.branch_id=?3))",
            )?;
            statement
                .query_map(
                    params![
                        context.tenant_id.to_string(),
                        policy.authentication_failure_threshold,
                        context.branch_id.to_string()
                    ],
                    |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, i64>(2)?,
                            row.get::<_, String>(3)?,
                        ))
                    },
                )?
                .collect::<Result<Vec<_>, _>>()?
        };

        let mut created_alerts = 0usize;
        let mut existing_alerts = 0usize;
        let mut produce = |severity: &str,
                           alert_type: &str,
                           title: String,
                           entity_type: &str,
                           entity_id: &str,
                           details: serde_json::Value|
         -> Result<(), StoreError> {
            if Self::append_automatic_operational_alert(
                &tx,
                context.tenant_id,
                context.branch_id,
                context.device_id,
                user,
                severity,
                alert_type,
                &title,
                entity_type,
                entity_id,
                &details,
                now,
            )?
            .is_some()
            {
                created_alerts += 1;
            } else {
                existing_alerts += 1;
            }
            Ok(())
        };
        for (product_id, name, quantity_milli, reorder_point_milli) in low_stock {
            produce(
                if quantity_milli < 0 { "HIGH" } else { "MEDIUM" },
                "LOW_STOCK",
                format!("Low stock: {name}"),
                "product",
                &product_id,
                serde_json::json!({
                    "quantity_milli": quantity_milli,
                    "reorder_point_milli": reorder_point_milli
                }),
            )?;
        }
        for (lot_id, name, expires_on, quantity_milli) in expiring_lots {
            let expiry = chrono::NaiveDate::parse_from_str(&expires_on, "%Y-%m-%d")
                .map_err(|_| StoreError::Validation("invalid inventory lot expiry".into()))?;
            let days_remaining = expiry.signed_duration_since(today).num_days();
            produce(
                if days_remaining < 0 {
                    "HIGH"
                } else if days_remaining <= 3 {
                    "MEDIUM"
                } else {
                    "LOW"
                },
                "EXPIRY",
                format!("Expiring stock: {name}"),
                "inventory_lot",
                &lot_id,
                serde_json::json!({
                    "expires_on": expires_on,
                    "days_remaining": days_remaining,
                    "quantity_milli": quantity_milli
                }),
            )?;
        }
        for (device_id, oldest_update, pending_count) in delayed_sync {
            produce(
                "HIGH",
                "SYNC_DELAY",
                "Terminal synchronization is delayed".into(),
                "device",
                &device_id,
                serde_json::json!({
                    "oldest_pending_update": oldest_update,
                    "pending_count": pending_count,
                    "threshold_minutes": policy.sync_delay_minutes
                }),
            )?;
        }
        for (device_id, label, last_heartbeat_at, created_at) in offline_terminals {
            produce(
                "HIGH",
                "TERMINAL_OFFLINE",
                format!("Terminal offline: {label}"),
                "device",
                &device_id,
                serde_json::json!({
                    "last_heartbeat_at": last_heartbeat_at,
                    "device_created_at": created_at,
                    "threshold_minutes": policy.terminal_offline_minutes
                }),
            )?;
        }
        for (invoice_id, invoice_number, supplier_name, due_date, balance_fils) in overdue_invoices {
            produce(
                "HIGH",
                "OVERDUE_SUPPLIER_INVOICE",
                format!("Supplier invoice overdue: {supplier_name}"),
                "supplier_invoice",
                &invoice_id,
                serde_json::json!({
                    "invoice_number": invoice_number,
                    "due_date": due_date,
                    "balance_fils": balance_fils
                }),
            )?;
        }
        for (backup_id, backup_type, state, integrity_state, created_at) in failed_backups {
            produce(
                "CRITICAL",
                "BACKUP_FAILURE",
                "Backup failed or failed integrity verification".into(),
                "backup",
                &backup_id,
                serde_json::json!({
                    "backup_type": backup_type,
                    "state": state,
                    "integrity_state": integrity_state,
                    "created_at": created_at
                }),
            )?;
        }
        for (user_id, display_name, failed_attempts, status) in authentication_risks {
            produce(
                "CRITICAL",
                "SECURITY_EVENT",
                format!("Repeated authentication failures: {display_name}"),
                "user",
                &user_id,
                serde_json::json!({
                    "failed_attempts": failed_attempts,
                    "account_status": status,
                    "threshold": policy.authentication_failure_threshold
                }),
            )?;
        }
        drop(produce);
        let result = OperationalAlertEvaluationResult {
            operation_id,
            created_alerts,
            existing_alerts,
            evaluated_at: now.to_rfc3339(),
        };
        Self::record_alert_operation(
            &tx,
            context.tenant_id,
            operation_id,
            "EVALUATE",
            &digest,
            &result,
            now,
        )?;
        Self::append_audit(
            &tx,
            context.tenant_id,
            context.device_id,
            user,
            "OPERATIONAL_ALERTS_EVALUATED",
            "branch",
            &context.branch_id.to_string(),
            &serde_json::to_string(&result)?,
            now,
        )?;
        tx.commit()?;
        Ok(result)
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
