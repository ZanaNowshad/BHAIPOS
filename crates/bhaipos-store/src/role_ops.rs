use super::*;

const RESERVED_ROLE_NAMES: &[&str] = &[
    "owner",
    "administrator",
    "admin",
    "manager",
    "cashier",
    "accountant",
    "inventory staff",
    "delivery staff",
];

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoleAdministrationResult {
    pub event_id: Uuid,
    pub role_id: Uuid,
    pub role_name: String,
    pub version: i64,
    pub permission_codes: Vec<String>,
    pub target_user_id: Option<UserId>,
    pub assigned: Option<bool>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoleAdministrationCatalog {
    pub permissions: Vec<(String, String)>,
    pub custom_roles: Vec<(Uuid, String, i64, Vec<String>)>,
}

#[derive(Clone, Debug)]
pub struct ConfigureCustomRoleRequest {
    pub context: LocalTerminalContext,
    pub actor_user_id: UserId,
    pub operation_id: OperationId,
    pub role_name: String,
    pub permission_codes: Vec<String>,
    pub reason: String,
    pub now: DateTime<Utc>,
}

#[derive(Clone, Debug)]
pub struct AssignCustomRoleRequest {
    pub context: LocalTerminalContext,
    pub actor_user_id: UserId,
    pub operation_id: OperationId,
    pub target_employee_no: String,
    pub role_name: String,
    pub assigned: bool,
    pub reason: String,
    pub now: DateTime<Utc>,
}

impl Store {
    pub fn role_administration_catalog(
        &self,
        context: LocalTerminalContext,
        actor_user_id: UserId,
    ) -> Result<RoleAdministrationCatalog, StoreError> {
        self.validate_local_session(context, actor_user_id)?;
        if !self.user_has_permission(context, actor_user_id, "role.manage")? {
            return Err(StoreError::Authorization("role.manage"));
        }
        let permissions = self
            .conn
            .prepare("SELECT code,description FROM permissions ORDER BY code")?
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect::<Result<Vec<_>, _>>()?;
        let mut statement = self.conn.prepare("SELECT r.id,r.name,p.version FROM roles r JOIN custom_role_profiles p ON p.role_id=r.id AND p.tenant_id=r.tenant_id WHERE r.tenant_id=?1 ORDER BY lower(r.name),r.id")?;
        let rows = statement.query_map(params![context.tenant_id.to_string()], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
            ))
        })?;
        let mut custom_roles = Vec::new();
        for row in rows {
            let (id, name, version) = row?;
            let role_id = Uuid::parse_str(&id)
                .map_err(|_| StoreError::Validation("invalid role identity".into()))?;
            let permission_codes = self
                .conn
                .prepare("SELECT permission_code FROM role_permissions WHERE role_id=?1 ORDER BY permission_code")?
                .query_map(params![id], |permission| permission.get(0))?
                .collect::<Result<Vec<String>, _>>()?;
            custom_roles.push((role_id, name, version, permission_codes));
        }
        Ok(RoleAdministrationCatalog {
            permissions,
            custom_roles,
        })
    }

    pub fn configure_custom_role(
        &mut self,
        req: ConfigureCustomRoleRequest,
    ) -> Result<RoleAdministrationResult, StoreError> {
        self.validate_local_session(req.context, req.actor_user_id)?;
        let role_name = req.role_name.trim();
        let reason = req.reason.trim();
        if role_name.len() < 2 || reason.len() < 3 {
            return Err(StoreError::Validation(
                "role name and a meaningful reason are required".into(),
            ));
        }
        if RESERVED_ROLE_NAMES.contains(&role_name.to_ascii_lowercase().as_str()) {
            return Err(StoreError::Authorization(
                "built-in roles cannot be changed through custom role administration",
            ));
        }
        let mut permission_codes = req
            .permission_codes
            .iter()
            .map(|code| code.trim().to_owned())
            .filter(|code| !code.is_empty())
            .collect::<Vec<_>>();
        permission_codes.sort();
        permission_codes.dedup();
        let digest = sha256_hex(&serde_json::to_vec(&(
            "ROLE_CONFIGURE:v1",
            req.context.tenant_id,
            req.context.branch_id,
            req.context.device_id,
            req.actor_user_id,
            role_name,
            &permission_codes,
            reason,
        ))?);
        if let Some(result) = self.load_store_operation(
            req.context.tenant_id,
            req.operation_id,
            "ROLE_CONFIGURE",
            &digest,
        )? {
            return Ok(result);
        }
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
            "role.manage",
        )?;
        for code in &permission_codes {
            let exists: Option<i32> = tx
                .query_row(
                    "SELECT 1 FROM permissions WHERE code=?1",
                    params![code],
                    |row| row.get(0),
                )
                .optional()?;
            if exists.is_none() {
                return Err(StoreError::Validation(format!(
                    "unknown permission: {code}"
                )));
            }
        }
        let existing: Option<(String, i64)> = tx
            .query_row(
                "SELECT r.id,p.version FROM roles r JOIN custom_role_profiles p ON p.role_id=r.id AND p.tenant_id=r.tenant_id WHERE r.tenant_id=?1 AND lower(r.name)=lower(?2)",
                params![req.context.tenant_id.to_string(), role_name],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        let (role_id, version, event_type, previous_permissions) = if let Some((id, version)) =
            existing
        {
            let previous = tx
                .prepare("SELECT permission_code FROM role_permissions WHERE role_id=?1 ORDER BY permission_code")?
                .query_map(params![id], |row| row.get(0))?
                .collect::<Result<Vec<String>, _>>()?;
            tx.execute(
                "UPDATE custom_role_profiles SET version=version+1,updated_at=?2 WHERE role_id=?1",
                params![id, req.now.to_rfc3339()],
            )?;
            (id, version + 1, "PERMISSIONS_REPLACED", previous)
        } else {
            let conflicting: Option<i32> = tx
                .query_row(
                    "SELECT 1 FROM roles WHERE tenant_id=?1 AND lower(name)=lower(?2)",
                    params![req.context.tenant_id.to_string(), role_name],
                    |row| row.get(0),
                )
                .optional()?;
            if conflicting.is_some() {
                return Err(StoreError::Authorization(
                    "built-in roles cannot be converted to custom roles",
                ));
            }
            let id = Uuid::new_v4().to_string();
            tx.execute(
                "INSERT INTO roles(id,tenant_id,name) VALUES(?1,?2,?3)",
                params![id, req.context.tenant_id.to_string(), role_name],
            )?;
            tx.execute("INSERT INTO custom_role_profiles(role_id,tenant_id,version,created_by_user_id,created_on_device_id,created_at,updated_at) VALUES(?1,?2,1,?3,?4,?5,?5)",params![id,req.context.tenant_id.to_string(),req.actor_user_id.to_string(),req.context.device_id.to_string(),req.now.to_rfc3339()])?;
            (id, 1, "ROLE_CREATED", Vec::new())
        };
        tx.execute(
            "DELETE FROM role_permissions WHERE role_id=?1",
            params![role_id],
        )?;
        for code in &permission_codes {
            tx.execute(
                "INSERT INTO role_permissions(role_id,permission_code) VALUES(?1,?2)",
                params![role_id, code],
            )?;
        }
        let event_id = Uuid::new_v4();
        let previous_json = serde_json::to_string(&previous_permissions)?;
        let new_json = serde_json::to_string(&permission_codes)?;
        tx.execute("INSERT INTO role_administration_events(id,tenant_id,branch_id,role_id,target_user_id,actor_user_id,device_id,operation_id,event_type,previous_json,new_json,reason,created_at) VALUES(?1,?2,?3,?4,NULL,?5,?6,?7,?8,?9,?10,?11,?12)",params![event_id.to_string(),req.context.tenant_id.to_string(),req.context.branch_id.to_string(),role_id,req.actor_user_id.to_string(),req.context.device_id.to_string(),req.operation_id.to_string(),event_type,previous_json,new_json,reason,req.now.to_rfc3339()])?;
        let role_uuid = Uuid::parse_str(&role_id)
            .map_err(|_| StoreError::Validation("invalid role identity".into()))?;
        let result = RoleAdministrationResult {
            event_id,
            role_id: role_uuid,
            role_name: role_name.into(),
            version,
            permission_codes,
            target_user_id: None,
            assigned: None,
        };
        let payload = serde_json::to_string(&result)?;
        Self::append_audit(
            &tx,
            req.context.tenant_id,
            req.context.device_id,
            req.actor_user_id,
            "CUSTOM_ROLE_CONFIGURED",
            "role",
            &role_id,
            &payload,
            req.now,
        )?;
        tx.execute("INSERT INTO sync_queue(id,tenant_id,branch_id,device_id,operation_id,entity_type,entity_id,mutation_type,payload_json,state,created_at,updated_at) VALUES(?1,?2,?3,?4,?5,'role',?6,'CONFIGURE',?7,'PENDING',?8,?8)",params![Uuid::new_v4().to_string(),req.context.tenant_id.to_string(),req.context.branch_id.to_string(),req.context.device_id.to_string(),req.operation_id.to_string(),role_id,payload,req.now.to_rfc3339()])?;
        Self::record_store_operation(
            &tx,
            req.context.tenant_id,
            req.operation_id,
            "ROLE_CONFIGURE",
            &digest,
            &result,
            req.now,
        )?;
        tx.commit()?;
        Ok(result)
    }

    pub fn assign_custom_role(
        &mut self,
        req: AssignCustomRoleRequest,
    ) -> Result<RoleAdministrationResult, StoreError> {
        self.validate_local_session(req.context, req.actor_user_id)?;
        let employee_no = req.target_employee_no.trim();
        let role_name = req.role_name.trim();
        let reason = req.reason.trim();
        if employee_no.is_empty() || role_name.is_empty() || reason.len() < 3 {
            return Err(StoreError::Validation(
                "employee number, custom role, and a meaningful reason are required".into(),
            ));
        }
        let digest = sha256_hex(&serde_json::to_vec(&(
            "ROLE_ASSIGNMENT:v1",
            req.context.tenant_id,
            req.context.branch_id,
            req.context.device_id,
            req.actor_user_id,
            employee_no,
            role_name,
            req.assigned,
            reason,
        ))?);
        if let Some(result) = self.load_store_operation(
            req.context.tenant_id,
            req.operation_id,
            "ROLE_ASSIGNMENT",
            &digest,
        )? {
            return Ok(result);
        }
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
            "role.manage",
        )?;
        let role: Option<(String, i64)> = tx
            .query_row(
                "SELECT r.id,p.version FROM roles r JOIN custom_role_profiles p ON p.role_id=r.id AND p.tenant_id=r.tenant_id WHERE r.tenant_id=?1 AND lower(r.name)=lower(?2)",
                params![req.context.tenant_id.to_string(), role_name],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        let (role_id, version) = role.ok_or(StoreError::NotFound("custom role"))?;
        let targets = {
            let mut statement = tx.prepare(
                "SELECT id FROM users WHERE tenant_id=?1 AND employee_no=?2 AND status='ACTIVE' LIMIT 2",
            )?;
            let rows = statement.query_map(
                params![req.context.tenant_id.to_string(), employee_no],
                |row| row.get::<_, String>(0),
            )?;
            rows.collect::<Result<Vec<_>, _>>()?
        };
        if targets.len() != 1 {
            return Err(StoreError::Authorization(
                "target employee is unknown, ambiguous, or inactive",
            ));
        }
        let target_id = &targets[0];
        let target_user_id = UserId(
            Uuid::parse_str(target_id)
                .map_err(|_| StoreError::Validation("invalid user identity".into()))?,
        );
        let was_assigned: bool = tx
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM user_roles WHERE user_id=?1 AND role_id=?2 AND branch_id=?3)",
                params![target_id, role_id, req.context.branch_id.to_string()],
                |row| row.get(0),
            )?;
        if was_assigned == req.assigned {
            return Err(StoreError::Conflict(
                "custom role assignment already has the requested state".into(),
            ));
        }
        if !req.assigned && target_user_id == req.actor_user_id {
            let role_grants_management: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM role_permissions WHERE role_id=?1 AND permission_code='role.manage')",
                params![role_id],
                |row| row.get(0),
            )?;
            if role_grants_management {
                let alternate: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM user_roles ur JOIN roles r ON r.id=ur.role_id AND r.tenant_id=?2 JOIN role_permissions rp ON rp.role_id=r.id AND rp.permission_code='role.manage' WHERE ur.user_id=?1 AND ur.role_id<>?3 AND (ur.branch_id IS NULL OR ur.branch_id=?4))",params![target_id,req.context.tenant_id.to_string(),role_id,req.context.branch_id.to_string()],|row|row.get(0))?;
                if !alternate {
                    return Err(StoreError::Authorization(
                        "cannot remove the current administrator's last role-management grant",
                    ));
                }
            }
        }
        if req.assigned {
            tx.execute(
                "INSERT INTO user_roles(user_id,role_id,branch_id) VALUES(?1,?2,?3)",
                params![target_id, role_id, req.context.branch_id.to_string()],
            )?;
        } else {
            tx.execute(
                "DELETE FROM user_roles WHERE user_id=?1 AND role_id=?2 AND branch_id=?3",
                params![target_id, role_id, req.context.branch_id.to_string()],
            )?;
        }
        let permission_codes = tx
            .prepare("SELECT permission_code FROM role_permissions WHERE role_id=?1 ORDER BY permission_code")?
            .query_map(params![role_id], |row| row.get(0))?
            .collect::<Result<Vec<String>, _>>()?;
        let event_id = Uuid::new_v4();
        let previous_json = serde_json::to_string(&serde_json::json!({
            "assigned": was_assigned,
            "branch_id": req.context.branch_id,
        }))?;
        let new_json = serde_json::to_string(&serde_json::json!({
            "assigned": req.assigned,
            "branch_id": req.context.branch_id,
        }))?;
        tx.execute("INSERT INTO role_administration_events(id,tenant_id,branch_id,role_id,target_user_id,actor_user_id,device_id,operation_id,event_type,previous_json,new_json,reason,created_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)",params![event_id.to_string(),req.context.tenant_id.to_string(),req.context.branch_id.to_string(),role_id,target_id,req.actor_user_id.to_string(),req.context.device_id.to_string(),req.operation_id.to_string(),if req.assigned{"ROLE_ASSIGNED"}else{"ROLE_UNASSIGNED"},previous_json,new_json,reason,req.now.to_rfc3339()])?;
        let role_uuid = Uuid::parse_str(&role_id)
            .map_err(|_| StoreError::Validation("invalid role identity".into()))?;
        let result = RoleAdministrationResult {
            event_id,
            role_id: role_uuid,
            role_name: role_name.into(),
            version,
            permission_codes,
            target_user_id: Some(target_user_id),
            assigned: Some(req.assigned),
        };
        let payload = serde_json::to_string(&result)?;
        Self::append_audit(
            &tx,
            req.context.tenant_id,
            req.context.device_id,
            req.actor_user_id,
            if req.assigned {
                "CUSTOM_ROLE_ASSIGNED"
            } else {
                "CUSTOM_ROLE_UNASSIGNED"
            },
            "user",
            target_id,
            &payload,
            req.now,
        )?;
        tx.execute("INSERT INTO sync_queue(id,tenant_id,branch_id,device_id,operation_id,entity_type,entity_id,mutation_type,payload_json,state,created_at,updated_at) VALUES(?1,?2,?3,?4,?5,'user',?6,'ROLE_ASSIGNMENT',?7,'PENDING',?8,?8)",params![Uuid::new_v4().to_string(),req.context.tenant_id.to_string(),req.context.branch_id.to_string(),req.context.device_id.to_string(),req.operation_id.to_string(),target_id,payload,req.now.to_rfc3339()])?;
        Self::record_store_operation(
            &tx,
            req.context.tenant_id,
            req.operation_id,
            "ROLE_ASSIGNMENT",
            &digest,
            &result,
            req.now,
        )?;
        tx.commit()?;
        Ok(result)
    }
}
