use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum UserSecurityAction {
    ResetPin,
    Unlock,
    Suspend,
    Reactivate,
}

impl UserSecurityAction {
    fn event_type(&self) -> &'static str {
        match self {
            Self::ResetPin => "PIN_RESET",
            Self::Unlock => "ACCOUNT_UNLOCKED",
            Self::Suspend => "ACCOUNT_SUSPENDED",
            Self::Reactivate => "ACCOUNT_REACTIVATED",
        }
    }
}

#[derive(Clone, Debug)]
pub struct UserSecurityRequest {
    pub context: LocalTerminalContext,
    pub actor_user_id: UserId,
    pub operation_id: OperationId,
    pub target_employee_no: String,
    pub action: UserSecurityAction,
    pub new_pin: Option<String>,
    pub reason: String,
    pub now: DateTime<Utc>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct UserSecurityResult {
    pub event_id: Uuid,
    pub target_user_id: UserId,
    pub employee_no: String,
    pub display_name: String,
    pub previous_status: String,
    pub new_status: String,
    pub event_type: String,
}

impl Store {
    pub fn administer_user_security(
        &mut self,
        req: UserSecurityRequest,
    ) -> Result<UserSecurityResult, StoreError> {
        self.validate_local_session(req.context, req.actor_user_id)?;
        let employee_no = req.target_employee_no.trim();
        let reason = req.reason.trim();
        if employee_no.is_empty() || reason.is_empty() {
            return Err(StoreError::Validation(
                "target employee number and reason are required".into(),
            ));
        }
        match (&req.action, &req.new_pin) {
            (UserSecurityAction::ResetPin, Some(pin)) => {
                bhaipos_core::validate_pin(pin)
                    .map_err(|error| StoreError::Validation(error.to_string()))?;
            }
            (UserSecurityAction::ResetPin, None) => {
                return Err(StoreError::Validation("new PIN is required".into()))
            }
            (_, Some(_)) => {
                return Err(StoreError::Validation(
                    "new PIN is only valid for a PIN reset".into(),
                ))
            }
            _ => {}
        };
        let digest = sha256_hex(&serde_json::to_vec(&(
            "USER_SECURITY:v1",
            req.context.tenant_id.to_string(),
            req.context.branch_id.to_string(),
            req.context.device_id.to_string(),
            req.actor_user_id.to_string(),
            employee_no,
            &req.action,
            reason,
        ))?);
        if let Some(existing) = self.load_store_operation(
            req.context.tenant_id,
            req.operation_id,
            "USER_SECURITY",
            &digest,
        )? {
            if req.action == UserSecurityAction::ResetPin {
                let verifier: String = self.conn.query_row(
                    "SELECT request_pin_hash FROM user_security_events WHERE tenant_id=?1 AND operation_id=?2",
                    params![req.context.tenant_id.to_string(), req.operation_id.to_string()],
                    |row| row.get(0),
                )?;
                if !verify_pin(&verifier, req.new_pin.as_deref().unwrap_or_default()) {
                    return Err(StoreError::Conflict(
                        "operation ID reused with a different action or payload".into(),
                    ));
                }
            }
            return Ok(existing);
        }
        let replacement_hash = req
            .new_pin
            .as_deref()
            .map(hash_pin)
            .transpose()
            .map_err(|error| StoreError::Validation(error.to_string()))?;
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        Self::assert_active_device_tx(
            &tx,
            req.context.tenant_id,
            req.context.branch_id,
            req.context.device_id,
        )?;
        Self::assert_permission(
            &tx,
            req.context.tenant_id,
            req.context.branch_id,
            req.actor_user_id,
            "employee.security.manage",
        )?;
        let targets = {
            let mut statement = tx.prepare("SELECT u.id,u.display_name,u.status FROM users u WHERE u.tenant_id=?1 AND u.employee_no=?2 AND EXISTS(SELECT 1 FROM user_roles ur JOIN roles r ON r.id=ur.role_id WHERE ur.user_id=u.id AND r.tenant_id=u.tenant_id AND (ur.branch_id IS NULL OR ur.branch_id=?3)) LIMIT 2")?;
            let rows = statement.query_map(
                params![
                    req.context.tenant_id.to_string(),
                    employee_no,
                    req.context.branch_id.to_string()
                ],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                    ))
                },
            )?;
            rows.collect::<Result<Vec<_>, _>>()?
        };
        if targets.len() != 1 {
            return Err(StoreError::Authorization(
                "target employee is unknown, ambiguous, or outside branch scope",
            ));
        }
        let (target_id, display_name, previous_status) = &targets[0];
        let target_user_id = UserId(
            Uuid::parse_str(target_id)
                .map_err(|_| StoreError::Validation("invalid user identity".into()))?,
        );
        let new_status = match req.action {
            UserSecurityAction::ResetPin => previous_status.clone(),
            UserSecurityAction::Unlock if previous_status == "LOCKED" => "ACTIVE".into(),
            UserSecurityAction::Suspend if target_user_id != req.actor_user_id => {
                "SUSPENDED".into()
            }
            UserSecurityAction::Reactivate if previous_status == "SUSPENDED" => "ACTIVE".into(),
            UserSecurityAction::Suspend => {
                return Err(StoreError::Authorization("cannot suspend current user"))
            }
            _ => {
                return Err(StoreError::Conflict(
                    "user security action is invalid for current status".into(),
                ))
            }
        };
        if let Some(ref pin_hash) = replacement_hash {
            tx.execute(
                "UPDATE users SET pin_hash=?2,failed_attempts=0 WHERE id=?1",
                params![target_id, pin_hash],
            )?;
        } else {
            tx.execute(
                "UPDATE users SET status=?2,failed_attempts=0,locked_until=NULL WHERE id=?1",
                params![target_id, new_status],
            )?;
        }
        let event_id = Uuid::new_v4();
        tx.execute("INSERT INTO user_security_events(id,tenant_id,branch_id,target_user_id,actor_user_id,device_id,operation_id,event_type,request_pin_hash,previous_status,new_status,reason,created_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)",params![event_id.to_string(),req.context.tenant_id.to_string(),req.context.branch_id.to_string(),target_id,req.actor_user_id.to_string(),req.context.device_id.to_string(),req.operation_id.to_string(),req.action.event_type(),replacement_hash.as_deref(),previous_status,new_status,reason,req.now.to_rfc3339()])?;
        let result = UserSecurityResult {
            event_id,
            target_user_id,
            employee_no: employee_no.into(),
            display_name: display_name.clone(),
            previous_status: previous_status.clone(),
            new_status: new_status.clone(),
            event_type: req.action.event_type().into(),
        };
        let payload = serde_json::to_string(&result)?;
        Self::append_audit(
            &tx,
            req.context.tenant_id,
            req.context.device_id,
            req.actor_user_id,
            "USER_SECURITY_CHANGED",
            "user",
            target_id,
            &payload,
            req.now,
        )?;
        tx.execute("INSERT INTO sync_queue(id,tenant_id,branch_id,device_id,operation_id,entity_type,entity_id,mutation_type,payload_json,state,created_at,updated_at) VALUES(?1,?2,?3,?4,?5,'user',?6,'SECURITY',?7,'PENDING',?8,?8)",params![Uuid::new_v4().to_string(),req.context.tenant_id.to_string(),req.context.branch_id.to_string(),req.context.device_id.to_string(),req.operation_id.to_string(),target_id,payload,req.now.to_rfc3339()])?;
        Self::record_store_operation(
            &tx,
            req.context.tenant_id,
            req.operation_id,
            "USER_SECURITY",
            &digest,
            &result,
            req.now,
        )?;
        tx.commit()?;
        Ok(result)
    }
}
