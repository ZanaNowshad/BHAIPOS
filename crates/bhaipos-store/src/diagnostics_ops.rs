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
        let database_integrity: String =
            self.conn
                .query_row("PRAGMA quick_check", [], |row| row.get(0))?;
        let foreign_key_violations: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM pragma_foreign_key_check",
            [],
            |row| row.get(0),
        )?;
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
}
