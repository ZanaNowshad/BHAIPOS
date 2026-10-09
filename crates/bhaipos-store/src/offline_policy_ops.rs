use super::*;

const OFFLINE_ACTIONS: &[&str] = &[
    "LOGIN",
    "SALE_CHECKOUT",
    "CASH_EVENT",
    "REFUND",
    "VOID",
    "PRICE_OVERRIDE",
    "CUSTOMER_CREDIT",
    "STOCK_ADJUSTMENT",
    "SUPPLIER_PAYMENT",
];

impl Store {
    pub fn configure_offline_policy(
        &mut self,
        request: ConfigureOfflinePolicyRequest,
    ) -> Result<OfflinePolicyResult, StoreError> {
        self.validate_local_session(request.context, request.user_id)?;
        if !self.user_has_permission(request.context, request.user_id, "offline_policy.manage")? {
            return Err(StoreError::Authorization("offline_policy.manage"));
        }
        if !(15..=10_080).contains(&request.offline_login_window_minutes)
            || !(15..=43_200).contains(&request.max_policy_staleness_minutes)
            || !(1..=365).contains(&request.authorization_valid_days)
        {
            return Err(StoreError::Validation(
                "offline policy windows are outside supported bounds".into(),
            ));
        }
        if request.rules.is_empty() {
            return Err(StoreError::Validation(
                "offline policy requires at least one action rule".into(),
            ));
        }
        let mut normalized_rules = Vec::with_capacity(request.rules.len());
        let mut actions = HashSet::new();
        for rule in &request.rules {
            let action = Self::normalize_offline_action(&rule.action_type)?;
            if !actions.insert(action.clone()) {
                return Err(StoreError::Validation(format!(
                    "duplicate offline action rule: {action}"
                )));
            }
            if !(0..=43_200).contains(&rule.max_offline_age_minutes) {
                return Err(StoreError::Validation(format!(
                    "offline age is outside supported bounds for {action}"
                )));
            }
            let constraints: serde_json::Value = serde_json::from_str(&rule.constraints_json)?;
            if !constraints.is_object() {
                return Err(StoreError::Validation(format!(
                    "offline constraints must be a JSON object for {action}"
                )));
            }
            normalized_rules.push((
                action,
                rule.decision.as_db().to_owned(),
                rule.max_offline_age_minutes,
                serde_json::to_string(&constraints)?,
            ));
        }
        normalized_rules.sort();
        let request_sha256 = sha256_hex(&serde_json::to_vec(&(
            "OFFLINE_POLICY_CONFIGURE:v1",
            request.context.tenant_id.to_string(),
            request.context.branch_id.to_string(),
            request.context.device_id.to_string(),
            request.user_id.to_string(),
            request.offline_login_window_minutes,
            request.max_policy_staleness_minutes,
            request.authorization_valid_days,
            &normalized_rules,
        ))?);
        const ACTION: &str = "OFFLINE_POLICY_CONFIGURE";
        if let Some(existing) = self.load_idempotent_result(
            request.context.tenant_id,
            request.operation_id,
            ACTION,
            &request_sha256,
        )? {
            return Ok(existing);
        }
        let valid_until = request
            .now
            .checked_add_signed(chrono::Duration::days(request.authorization_valid_days))
            .ok_or_else(|| StoreError::Validation("offline policy validity overflow".into()))?;
        let policy_id = Uuid::new_v4();
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let version: i64 = tx.query_row(
            "SELECT COALESCE(MAX(version),0)+1 FROM offline_policy_versions WHERE tenant_id=?1 AND branch_id=?2",
            params![request.context.tenant_id.to_string(),request.context.branch_id.to_string()],
            |row| row.get(0),
        )?;
        tx.execute(
            "UPDATE offline_policy_versions SET state='RETIRED',retired_at=?3 WHERE tenant_id=?1 AND branch_id=?2 AND state='ACTIVE'",
            params![request.context.tenant_id.to_string(),request.context.branch_id.to_string(),request.now.to_rfc3339()],
        )?;
        tx.execute(
            "INSERT INTO offline_policy_versions(id,tenant_id,branch_id,version,state,offline_login_window_minutes,max_policy_staleness_minutes,valid_from,valid_until,operation_id,created_by_user_id,created_on_device_id,created_at) VALUES(?1,?2,?3,?4,'ACTIVE',?5,?6,?7,?8,?9,?10,?11,?7)",
            params![policy_id.to_string(),request.context.tenant_id.to_string(),request.context.branch_id.to_string(),version,request.offline_login_window_minutes,request.max_policy_staleness_minutes,request.now.to_rfc3339(),valid_until.to_rfc3339(),request.operation_id.to_string(),request.user_id.to_string(),request.context.device_id.to_string()],
        )?;
        for (action, decision, max_age, constraints) in &normalized_rules {
            tx.execute(
                "INSERT INTO offline_policy_rules(policy_id,tenant_id,action_type,decision,max_offline_age_minutes,constraints_json) VALUES(?1,?2,?3,?4,?5,?6)",
                params![policy_id.to_string(),request.context.tenant_id.to_string(),action,decision,max_age,constraints],
            )?;
        }
        tx.execute(
            "INSERT INTO device_offline_policy_state(tenant_id,branch_id,device_id,policy_id,policy_version,state,synchronized_at,valid_until,updated_at) VALUES(?1,?2,?3,?4,?5,'ACTIVE',?6,?7,?6) ON CONFLICT(tenant_id,device_id) DO UPDATE SET branch_id=excluded.branch_id,policy_id=excluded.policy_id,policy_version=excluded.policy_version,state='ACTIVE',synchronized_at=excluded.synchronized_at,valid_until=excluded.valid_until,updated_at=excluded.updated_at",
            params![request.context.tenant_id.to_string(),request.context.branch_id.to_string(),request.context.device_id.to_string(),policy_id.to_string(),version,request.now.to_rfc3339(),valid_until.to_rfc3339()],
        )?;
        let result = OfflinePolicyResult {
            policy_id,
            version,
            state: "ACTIVE".into(),
            valid_until: valid_until.to_rfc3339(),
            rule_count: normalized_rules.len(),
        };
        Self::record_idempotent_result(
            &tx,
            request.context.tenant_id,
            request.operation_id,
            ACTION,
            &request_sha256,
            &result,
            request.now,
        )?;
        Self::append_audit(
            &tx,
            request.context.tenant_id,
            request.context.device_id,
            request.user_id,
            "OFFLINE_POLICY_ACTIVATED",
            "offline_policy",
            &policy_id.to_string(),
            &serde_json::to_string(&serde_json::json!({
                "version": version,
                "valid_until": result.valid_until,
                "rule_count": result.rule_count,
            }))?,
            request.now,
        )?;
        tx.commit()?;
        Ok(result)
    }

    pub fn offline_authorization(
        &self,
        context: LocalTerminalContext,
        user: UserId,
        action_type: &str,
        now: DateTime<Utc>,
    ) -> Result<OfflineAuthorizationResult, StoreError> {
        self.validate_local_session(context, user)?;
        let action = match Self::normalize_offline_action(action_type) {
            Ok(value) => value,
            Err(_) => {
                return Ok(OfflineAuthorizationResult {
                    policy_id: None,
                    policy_version: None,
                    action_type: action_type.trim().to_ascii_uppercase(),
                    decision: OfflinePolicyDecision::Deny,
                    reason: "unknown offline action fails closed".into(),
                    policy_valid_until: None,
                })
            }
        };
        type PolicyRow = (
            String,
            i64,
            String,
            String,
            i64,
            String,
            String,
            String,
            String,
            i64,
        );
        let row: Option<PolicyRow> = self.conn.query_row(
            "SELECT p.id,p.version,p.valid_from,p.valid_until,p.max_policy_staleness_minutes,d.state,d.synchronized_at,d.valid_until,r.decision,r.max_offline_age_minutes FROM offline_policy_versions p JOIN device_offline_policy_state d ON d.policy_id=p.id AND d.tenant_id=p.tenant_id AND d.branch_id=p.branch_id JOIN offline_policy_rules r ON r.policy_id=p.id AND r.tenant_id=p.tenant_id WHERE p.tenant_id=?1 AND p.branch_id=?2 AND p.state='ACTIVE' AND d.device_id=?3 AND r.action_type=?4",
            params![context.tenant_id.to_string(),context.branch_id.to_string(),context.device_id.to_string(),action],
            |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?,row.get(4)?,row.get(5)?,row.get(6)?,row.get(7)?,row.get(8)?,row.get(9)?)),
        ).optional()?;
        let Some((
            policy_id,
            version,
            valid_from,
            policy_valid_until,
            max_staleness,
            device_state,
            synchronized_at,
            device_valid_until,
            decision,
            action_max_age,
        )) = row
        else {
            return Ok(OfflineAuthorizationResult {
                policy_id: None,
                policy_version: None,
                action_type: action,
                decision: OfflinePolicyDecision::Deny,
                reason: "no active device-scoped offline policy rule".into(),
                policy_valid_until: None,
            });
        };
        let policy_uuid = Uuid::parse_str(&policy_id)
            .map_err(|_| StoreError::Validation("invalid offline policy identity".into()))?;
        let parse_time = |value: &str| {
            DateTime::parse_from_rfc3339(value)
                .map(|parsed| parsed.with_timezone(&Utc))
                .map_err(|_| StoreError::Validation("invalid offline policy timestamp".into()))
        };
        let valid_from_at = parse_time(&valid_from)?;
        let policy_expires_at = parse_time(&policy_valid_until)?;
        let device_expires_at = parse_time(&device_valid_until)?;
        let synchronized_at = parse_time(&synchronized_at)?;
        let denied = |reason: &str| OfflineAuthorizationResult {
            policy_id: Some(policy_uuid),
            policy_version: Some(version),
            action_type: action.clone(),
            decision: OfflinePolicyDecision::Deny,
            reason: reason.into(),
            policy_valid_until: Some(policy_valid_until.clone()),
        };
        if now < valid_from_at {
            return Ok(denied("offline policy is not effective yet"));
        }
        if now > policy_expires_at || now > device_expires_at {
            return Ok(denied("offline policy authorization has expired"));
        }
        if device_state != "ACTIVE" {
            return Ok(denied("device offline policy state is not active"));
        }
        let allowed_age = max_staleness.min(action_max_age);
        if allowed_age == 0
            || now.signed_duration_since(synchronized_at) > chrono::Duration::minutes(allowed_age)
        {
            return Ok(denied("offline policy evidence is stale for this action"));
        }
        let decision = match decision.as_str() {
            "ALLOW" => OfflinePolicyDecision::Allow,
            "REQUIRE_MANAGER_APPROVAL" => OfflinePolicyDecision::RequireManagerApproval,
            _ => OfflinePolicyDecision::Deny,
        };
        Ok(OfflineAuthorizationResult {
            policy_id: Some(policy_uuid),
            policy_version: Some(version),
            action_type: action,
            decision,
            reason: "active bounded offline policy rule".into(),
            policy_valid_until: Some(policy_valid_until),
        })
    }

    fn normalize_offline_action(value: &str) -> Result<String, StoreError> {
        let normalized = value.trim().to_ascii_uppercase();
        if OFFLINE_ACTIONS.contains(&normalized.as_str()) {
            Ok(normalized)
        } else {
            Err(StoreError::Validation("unknown offline action".into()))
        }
    }
}
