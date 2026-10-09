use super::*;

impl Store {
    pub fn operational_diagnostics(
        &self,
        context: LocalTerminalContext,
        user: UserId,
    ) -> Result<OperationalDiagnostics, StoreError> {
        self.validate_local_session(context, user)?;
        if !self.user_has_permission(context, user, "diagnostics.view")? {
            return Err(StoreError::Authorization("diagnostics.view"));
        }
        let database_integrity: String = self
            .conn
            .query_row("PRAGMA quick_check", [], |row| row.get(0))?;
        let foreign_key_violations: i64 =
            self.conn
                .query_row("SELECT COUNT(*) FROM pragma_foreign_key_check", [], |row| {
                    row.get(0)
                })?;
        let (device_status, device_app_version, last_heartbeat_at): (
            String,
            Option<String>,
            Option<String>,
        ) = self.conn.query_row(
            "SELECT status,app_version,last_heartbeat_at FROM devices WHERE id=?1 AND tenant_id=?2 AND branch_id=?3",
            params![context.device_id.to_string(),context.tenant_id.to_string(),context.branch_id.to_string()],
            |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?)),
        )?;
        let pending_sync_mutations: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM sync_queue WHERE tenant_id=?1 AND branch_id=?2 AND device_id=?3 AND state IN ('PENDING','SENDING','RETRYING')",
            params![context.tenant_id.to_string(),context.branch_id.to_string(),context.device_id.to_string()],
            |row| row.get(0),
        )?;
        let sync_requires_review: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM sync_queue WHERE tenant_id=?1 AND branch_id=?2 AND device_id=?3 AND state='REQUIRES_REVIEW'",
            params![context.tenant_id.to_string(),context.branch_id.to_string(),context.device_id.to_string()],
            |row| row.get(0),
        )?;
        let last_sync_at: Option<String> = self
            .conn
            .query_row(
                "SELECT updated_at FROM device_sync_checkpoints WHERE tenant_id=?1 AND branch_id=?2 AND device_id=?3",
                params![context.tenant_id.to_string(),context.branch_id.to_string(),context.device_id.to_string()],
                |row| row.get(0),
            )
            .optional()?;
        let pending_background_jobs: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM background_jobs WHERE tenant_id=?1 AND branch_id=?2 AND origin_device_id=?3 AND state IN ('QUEUED','RUNNING')",
            params![context.tenant_id.to_string(),context.branch_id.to_string(),context.device_id.to_string()],
            |row| row.get(0),
        )?;
        let background_jobs_requires_review: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM background_jobs WHERE tenant_id=?1 AND branch_id=?2 AND origin_device_id=?3 AND state='REQUIRES_REVIEW'",
            params![context.tenant_id.to_string(),context.branch_id.to_string(),context.device_id.to_string()],
            |row| row.get(0),
        )?;
        let failed_print_jobs: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM print_jobs WHERE tenant_id=?1 AND branch_id=?2 AND device_id=?3 AND state='FAILED'",
            params![context.tenant_id.to_string(),context.branch_id.to_string(),context.device_id.to_string()],
            |row| row.get(0),
        )?;
        let printer_configured = self.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM printer_profiles WHERE tenant_id=?1 AND branch_id=?2 AND device_id=?3 AND active=1 AND is_default=1)",
            params![context.tenant_id.to_string(),context.branch_id.to_string(),context.device_id.to_string()],
            |row| row.get::<_,i64>(0),
        )? != 0;
        let latest_backup = self
            .conn
            .query_row(
                "SELECT id,backup_type,state,integrity_state,created_at FROM backup_records WHERE tenant_id=?1 AND branch_id=?2 AND origin_device_id=?3 AND state='SUCCEEDED' AND integrity_state='VERIFIED' ORDER BY created_at DESC,id DESC LIMIT 1",
                params![context.tenant_id.to_string(),context.branch_id.to_string(),context.device_id.to_string()],
                |row| Ok(DiagnosticBackupSummary { backup_id: Uuid::parse_str(&row.get::<_,String>(0)?).map_err(|_| rusqlite::Error::InvalidQuery)?, backup_type: row.get(1)?, state: row.get(2)?, integrity_state: row.get(3)?, created_at: row.get(4)? }),
            )
            .optional()?;
        let backup_schedule = self
            .conn
            .query_row(
                "SELECT state,next_run_at,authorization_expires_at,retention_count FROM backup_schedules WHERE tenant_id=?1 AND branch_id=?2 AND device_id=?3",
                params![context.tenant_id.to_string(),context.branch_id.to_string(),context.device_id.to_string()],
                |row| Ok(DiagnosticBackupScheduleSummary { state: row.get(0)?, next_run_at: row.get(1)?, authorization_expires_at: row.get(2)?, retention_count: row.get(3)? }),
            )
            .optional()?;
        Ok(OperationalDiagnostics {
            tenant_id: context.tenant_id,
            branch_id: context.branch_id,
            device_id: context.device_id,
            register_id: context.register_id,
            schema_version: LATEST_SCHEMA.into(),
            database_integrity,
            foreign_key_violations,
            device_status,
            device_app_version,
            last_heartbeat_at,
            pending_sync_mutations,
            sync_requires_review,
            last_sync_at,
            pending_background_jobs,
            background_jobs_requires_review,
            failed_print_jobs,
            printer_configured,
            latest_backup,
            backup_schedule,
        })
    }

    pub fn capture_diagnostic_snapshot(
        &mut self,
        request: DiagnosticCaptureRequest,
    ) -> Result<DiagnosticSnapshotResult, StoreError> {
        self.validate_local_session(request.context, request.user_id)?;
        if !self.user_has_permission(request.context, request.user_id, "diagnostics.export")? {
            return Err(StoreError::Authorization("diagnostics.export"));
        }
        let app_version = Self::diagnostic_label(&request.app_version, "application version")?;
        let build_sha = request
            .build_sha
            .as_deref()
            .map(|value| Self::diagnostic_label(value, "build SHA"))
            .transpose()?;
        let hub_mode = Self::diagnostic_label(&request.hub_mode, "hub mode")?;
        let whatsapp_status = Self::diagnostic_label(&request.whatsapp_status, "WhatsApp status")?;
        let ocr_status = Self::diagnostic_label(&request.ocr_status, "OCR status")?;
        let printer_state = Self::diagnostic_label(&request.printer_state, "printer state")?;
        let printer_detail = Self::diagnostic_label(&request.printer_detail, "printer detail")?;
        let normalized = (
            "DIAGNOSTICS_CAPTURE:v1",
            request.context.tenant_id.to_string(),
            request.context.branch_id.to_string(),
            request.context.device_id.to_string(),
            request.context.register_id.to_string(),
            request.user_id.to_string(),
            &app_version,
            &build_sha,
            &hub_mode,
            &whatsapp_status,
            &ocr_status,
            &printer_state,
            &printer_detail,
        );
        let request_sha256 = sha256_hex(&serde_json::to_vec(&normalized)?);
        const ACTION: &str = "DIAGNOSTICS_CAPTURE";
        if let Some(existing) = self.load_idempotent_result(
            request.context.tenant_id,
            request.operation_id,
            ACTION,
            &request_sha256,
        )? {
            return Ok(existing);
        }

        let operational = self.operational_diagnostics(request.context, request.user_id)?;
        let payload_json = serde_json::to_string(&serde_json::json!({
            "format_version": 1,
            "captured_at": request.now.to_rfc3339(),
            "application_version": app_version,
            "build_sha": build_sha,
            "hub_mode": hub_mode,
            "whatsapp_status": whatsapp_status,
            "ocr_status": ocr_status,
            "printer_state": printer_state,
            "printer_detail": printer_detail,
            "operational": operational,
        }))?;
        let payload_sha256 = sha256_hex(payload_json.as_bytes());
        let snapshot_id = Uuid::new_v4();
        let result = DiagnosticSnapshotResult {
            snapshot_id,
            payload_sha256: payload_sha256.clone(),
            created_at: request.now.to_rfc3339(),
            app_version: app_version.clone(),
            build_sha: build_sha.clone(),
            schema_version: LATEST_SCHEMA.into(),
            payload_json: payload_json.clone(),
        };
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute(
            "INSERT INTO diagnostics_snapshots(id,tenant_id,branch_id,device_id,app_version,build_sha,schema_version,payload_json,created_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)",
            params![
                snapshot_id.to_string(),
                request.context.tenant_id.to_string(),
                request.context.branch_id.to_string(),
                request.context.device_id.to_string(),
                app_version,
                build_sha,
                LATEST_SCHEMA,
                payload_json,
                request.now.to_rfc3339(),
            ],
        )?;
        tx.execute(
            "INSERT INTO diagnostic_snapshot_operations(tenant_id,operation_id,action,request_sha256,snapshot_id,branch_id,device_id,user_id,payload_sha256,committed_at) VALUES(?1,?2,'CAPTURE_REDACTED_DIAGNOSTICS',?3,?4,?5,?6,?7,?8,?9)",
            params![
                request.context.tenant_id.to_string(),
                request.operation_id.to_string(),
                request_sha256,
                snapshot_id.to_string(),
                request.context.branch_id.to_string(),
                request.context.device_id.to_string(),
                request.user_id.to_string(),
                payload_sha256,
                request.now.to_rfc3339(),
            ],
        )?;
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
            "DIAGNOSTICS_CAPTURED",
            "diagnostics_snapshot",
            &snapshot_id.to_string(),
            &serde_json::to_string(&serde_json::json!({
                "payload_sha256": result.payload_sha256,
                "schema_version": result.schema_version,
            }))?,
            request.now,
        )?;
        tx.commit()?;
        Ok(result)
    }

    pub fn list_diagnostic_snapshots(
        &self,
        context: LocalTerminalContext,
        user: UserId,
        limit: i64,
    ) -> Result<Vec<DiagnosticSnapshotSummary>, StoreError> {
        self.validate_local_session(context, user)?;
        if !self.user_has_permission(context, user, "diagnostics.view")? {
            return Err(StoreError::Authorization("diagnostics.view"));
        }
        if !(1..=100).contains(&limit) {
            return Err(StoreError::Validation(
                "diagnostic snapshot history limit must be between 1 and 100".into(),
            ));
        }
        let mut statement = self.conn.prepare(
            "SELECT s.id,o.payload_sha256,s.created_at,s.app_version,s.build_sha,s.schema_version
             FROM diagnostics_snapshots s
             JOIN diagnostic_snapshot_operations o ON o.snapshot_id=s.id AND o.tenant_id=s.tenant_id
             WHERE s.tenant_id=?1 AND s.branch_id=?2 AND s.device_id=?3
             ORDER BY s.created_at DESC,s.id DESC LIMIT ?4",
        )?;
        let rows = statement.query_map(
            params![
                context.tenant_id.to_string(),
                context.branch_id.to_string(),
                context.device_id.to_string(),
                limit,
            ],
            |row| {
                let id: String = row.get(0)?;
                Ok(DiagnosticSnapshotSummary {
                    snapshot_id: Uuid::parse_str(&id).map_err(|_| rusqlite::Error::InvalidQuery)?,
                    payload_sha256: row.get(1)?,
                    created_at: row.get(2)?,
                    app_version: row.get(3)?,
                    build_sha: row.get(4)?,
                    schema_version: row.get(5)?,
                })
            },
        )?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(StoreError::from)
    }

    fn diagnostic_label(value: &str, field: &str) -> Result<String, StoreError> {
        let normalized = value.trim();
        if normalized.is_empty()
            || normalized.len() > 128
            || normalized.chars().any(char::is_control)
        {
            return Err(StoreError::Validation(format!("invalid {field}")));
        }
        Ok(normalized.to_owned())
    }
}
