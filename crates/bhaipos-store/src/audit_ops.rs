use super::*;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditVerificationResult {
    pub verification_id: Uuid,
    pub chain_valid: bool,
    pub event_count: i64,
    pub verified_head_hash: Option<String>,
    pub alert_id: Option<Uuid>,
    pub verified_at: String,
}

impl Store {
    pub fn verify_local_audit_chain(
        &mut self,
        context: LocalTerminalContext,
        actor_user_id: UserId,
        operation_id: OperationId,
        now: DateTime<Utc>,
    ) -> Result<AuditVerificationResult, StoreError> {
        self.validate_local_session(context, actor_user_id)?;
        let digest = sha256_hex(&serde_json::to_vec(&(
            "AUDIT_VERIFY:v1",
            context.tenant_id,
            context.branch_id,
            context.device_id,
            actor_user_id,
        ))?);
        if let Some(result) =
            self.load_store_operation(context.tenant_id, operation_id, "AUDIT_VERIFY", &digest)?
        {
            return Ok(result);
        }
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        Self::assert_active_device_tx(
            &tx,
            context.tenant_id,
            context.branch_id,
            context.device_id,
        )?;
        Self::assert_permission(
            &tx,
            context.tenant_id,
            context.branch_id,
            actor_user_id,
            "audit.verify",
        )?;
        let chain_valid = Self::audit_chain_valid_on(&tx, context.tenant_id, context.device_id)?;
        let event_count: i64 = tx.query_row(
            "SELECT COUNT(*) FROM audit_events WHERE tenant_id=?1 AND device_id=?2",
            params![context.tenant_id.to_string(), context.device_id.to_string()],
            |row| row.get(0),
        )?;
        let verified_head_hash = if chain_valid && event_count > 0 {
            tx.query_row("SELECT a.event_hash FROM audit_events a WHERE a.tenant_id=?1 AND a.device_id=?2 AND NOT EXISTS(SELECT 1 FROM audit_events successor WHERE successor.tenant_id=a.tenant_id AND successor.device_id=a.device_id AND successor.previous_hash=a.event_hash) LIMIT 1",params![context.tenant_id.to_string(),context.device_id.to_string()],|row|row.get(0)).optional()?
        } else {
            None
        };
        let alert_id = if chain_valid {
            None
        } else {
            let existing: Option<String> = tx.query_row("SELECT id FROM operational_alerts WHERE tenant_id=?1 AND branch_id=?2 AND alert_type='SECURITY_EVENT' AND entity_type='device' AND entity_id=?3 AND status NOT IN ('RESOLVED','DISMISSED') LIMIT 1",params![context.tenant_id.to_string(),context.branch_id.to_string(),context.device_id.to_string()],|row|row.get(0)).optional()?;
            let id = existing.unwrap_or_else(|| Uuid::new_v4().to_string());
            if !tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM operational_alerts WHERE id=?1)",
                params![id],
                |row| row.get::<_, bool>(0),
            )? {
                tx.execute("INSERT INTO operational_alerts(id,tenant_id,branch_id,severity,alert_type,status,entity_type,entity_id,title,details_json,created_at) VALUES(?1,?2,?3,'CRITICAL','SECURITY_EVENT','NEW','device',?4,'Audit chain verification failed',?5,?6)",params![id,context.tenant_id.to_string(),context.branch_id.to_string(),context.device_id.to_string(),serde_json::to_string(&serde_json::json!({"event_count":event_count,"verification_operation_id":operation_id}))?,now.to_rfc3339()])?;
                tx.execute("INSERT INTO operational_alert_events(id,tenant_id,alert_id,branch_id,operation_id,event_type,previous_status,new_status,device_id,entered_by_user_id,note,created_at) VALUES(?1,?2,?3,?4,?5,'CREATED',NULL,'NEW',?6,?7,'Audit chain verification failed; preserve database and escalate',?8)",params![Uuid::new_v4().to_string(),context.tenant_id.to_string(),id,context.branch_id.to_string(),OperationId::new().to_string(),context.device_id.to_string(),actor_user_id.to_string(),now.to_rfc3339()])?;
            }
            Some(
                Uuid::parse_str(&id)
                    .map_err(|_| StoreError::Validation("invalid alert identity".into()))?,
            )
        };
        let verification_id = Uuid::new_v4();
        tx.execute("INSERT INTO audit_verification_runs(id,tenant_id,branch_id,device_id,actor_user_id,operation_id,chain_valid,event_count,verified_head_hash,alert_id,verified_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",params![verification_id.to_string(),context.tenant_id.to_string(),context.branch_id.to_string(),context.device_id.to_string(),actor_user_id.to_string(),operation_id.to_string(),chain_valid,event_count,verified_head_hash,alert_id.map(|id|id.to_string()),now.to_rfc3339()])?;
        let result = AuditVerificationResult {
            verification_id,
            chain_valid,
            event_count,
            verified_head_hash,
            alert_id,
            verified_at: now.to_rfc3339(),
        };
        if chain_valid {
            Self::append_audit(
                &tx,
                context.tenant_id,
                context.device_id,
                actor_user_id,
                "AUDIT_CHAIN_VERIFIED",
                "device",
                &context.device_id.to_string(),
                &serde_json::to_string(&result)?,
                now,
            )?;
        }
        Self::record_store_operation(
            &tx,
            context.tenant_id,
            operation_id,
            "AUDIT_VERIFY",
            &digest,
            &result,
            now,
        )?;
        tx.commit()?;
        Ok(result)
    }
}
