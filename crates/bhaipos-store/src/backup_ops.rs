use super::*;
use rusqlite::backup::Backup;
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::Read;
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
            Self::verify_backup_database(
                &partial_path,
                request.context,
                request.user_id,
                false,
            )?;
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
            return Err(StoreError::Conflict("backup hash verification failed".into()));
        }
        if actual_size != record.byte_size {
            return Err(StoreError::Conflict("backup size verification failed".into()));
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
            destination_directory: safety_directory,
            app_version: request.app_version.clone(),
            now: request.now,
        })?;
        let safety_record = self.backup_record(request.context.tenant_id, safety_result.backup_id)?;

        let source = Connection::open(&source_record.storage_path)?;
        {
            let backup = Backup::new(&source, &mut self.conn)?;
            backup.run_to_completion(128, Duration::from_millis(5), None)?;
        }
        drop(source);
        self.migrate()?;
        self.validate_local_session(request.context, request.user_id)?;
        self.assert_owner(request.context, request.user_id)?;

        let restore_id = request.operation_id.0;
        let result = RestoreResult {
            restore_id,
            backup_id: request.backup_id,
            pre_restore_backup_id: safety_result.backup_id,
            restored_sha256: preview.sha256,
            completed_at: request.now.to_rfc3339(),
        };
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
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
        Ok(result)
    }

    fn prepare_backup_directory(path: &Path) -> Result<PathBuf, StoreError> {
        if path.as_os_str().is_empty() {
            return Err(StoreError::Validation("backup directory is required".into()));
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
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY
                | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        let integrity: String =
            connection.query_row("PRAGMA integrity_check", [], |row| row.get(0))?;
        if integrity != "ok" {
            return Err(StoreError::Conflict(format!(
                "backup integrity check failed: {integrity}"
            )));
        }
        let foreign_key_errors: i64 = connection.query_row(
            "SELECT COUNT(*) FROM pragma_foreign_key_check",
            [],
            |row| row.get(0),
        )?;
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

    fn assert_owner(
        &self,
        context: LocalTerminalContext,
        user: UserId,
    ) -> Result<(), StoreError> {
        let owner: Option<i32> = self
            .conn
            .query_row(
                "SELECT 1 FROM user_roles ur JOIN roles role ON role.id=ur.role_id WHERE ur.user_id=?1 AND role.tenant_id=?2 AND lower(role.name)='owner' AND (ur.branch_id IS NULL OR ur.branch_id=?3) LIMIT 1",
                params![user.to_string(), context.tenant_id.to_string(), context.branch_id.to_string()],
                |row| row.get(0),
            )
            .optional()?;
        if owner.is_none() {
            return Err(StoreError::Authorization("owner role required for restore"));
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
