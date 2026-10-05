CREATE TABLE IF NOT EXISTS backup_operation_results (
  tenant_id TEXT NOT NULL REFERENCES tenants(id),
  operation_id TEXT NOT NULL,
  action TEXT NOT NULL CHECK(action IN ('CREATE','RESTORE')),
  request_sha256 TEXT NOT NULL CHECK(length(request_sha256)=64),
  result_json TEXT NOT NULL,
  committed_at TEXT NOT NULL,
  PRIMARY KEY(tenant_id,operation_id)
);

CREATE TABLE IF NOT EXISTS backup_events (
  id TEXT PRIMARY KEY,
  tenant_id TEXT NOT NULL REFERENCES tenants(id),
  branch_id TEXT NOT NULL REFERENCES branches(id),
  backup_id TEXT REFERENCES backup_records(id),
  restore_run_id TEXT REFERENCES restore_runs(id),
  event_type TEXT NOT NULL,
  device_id TEXT NOT NULL REFERENCES devices(id),
  user_id TEXT NOT NULL REFERENCES users(id),
  evidence_json TEXT NOT NULL,
  created_at TEXT NOT NULL
);

CREATE UNIQUE INDEX IF NOT EXISTS ux_backup_records_operation
  ON backup_records(tenant_id,operation_id) WHERE operation_id IS NOT NULL;
CREATE UNIQUE INDEX IF NOT EXISTS ux_restore_runs_operation
  ON restore_runs(tenant_id,operation_id) WHERE operation_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS ix_backup_records_scope_created
  ON backup_records(tenant_id,branch_id,created_at DESC);
CREATE INDEX IF NOT EXISTS ix_backup_events_history
  ON backup_events(tenant_id,created_at,id);

CREATE TRIGGER IF NOT EXISTS immutable_backup_operation_results_update
BEFORE UPDATE ON backup_operation_results
BEGIN SELECT RAISE(ABORT,'IMMUTABLE:backup_operation_results'); END;
CREATE TRIGGER IF NOT EXISTS immutable_backup_operation_results_delete
BEFORE DELETE ON backup_operation_results
BEGIN SELECT RAISE(ABORT,'IMMUTABLE:backup_operation_results'); END;
CREATE TRIGGER IF NOT EXISTS immutable_backup_events_update
BEFORE UPDATE ON backup_events
BEGIN SELECT RAISE(ABORT,'IMMUTABLE:backup_events'); END;
CREATE TRIGGER IF NOT EXISTS immutable_backup_events_delete
BEFORE DELETE ON backup_events
BEGIN SELECT RAISE(ABORT,'IMMUTABLE:backup_events'); END;
CREATE TRIGGER IF NOT EXISTS immutable_backup_records_identity
BEFORE UPDATE ON backup_records
WHEN NEW.id<>OLD.id OR NEW.tenant_id<>OLD.tenant_id OR NEW.branch_id IS NOT OLD.branch_id
 OR NEW.storage_path<>OLD.storage_path OR NEW.sha256<>OLD.sha256 OR NEW.byte_size IS NOT OLD.byte_size
 OR NEW.schema_version<>OLD.schema_version OR NEW.app_version<>OLD.app_version
 OR NEW.origin_device_id IS NOT OLD.origin_device_id OR NEW.created_by_user_id IS NOT OLD.created_by_user_id
 OR NEW.operation_id IS NOT OLD.operation_id OR NEW.created_at<>OLD.created_at
BEGIN SELECT RAISE(ABORT,'IMMUTABLE:backup_record_identity'); END;
CREATE TRIGGER IF NOT EXISTS immutable_backup_records_delete
BEFORE DELETE ON backup_records
BEGIN SELECT RAISE(ABORT,'IMMUTABLE:backup_records'); END;
CREATE TRIGGER IF NOT EXISTS immutable_restore_runs_identity
BEFORE UPDATE ON restore_runs
WHEN NEW.id<>OLD.id OR NEW.tenant_id<>OLD.tenant_id OR NEW.backup_id<>OLD.backup_id
 OR NEW.pre_restore_backup_id IS NOT OLD.pre_restore_backup_id
 OR NEW.authorized_by_user_id<>OLD.authorized_by_user_id
 OR NEW.origin_device_id IS NOT OLD.origin_device_id OR NEW.operation_id IS NOT OLD.operation_id
 OR NEW.started_at<>OLD.started_at
BEGIN SELECT RAISE(ABORT,'IMMUTABLE:restore_run_identity'); END;
CREATE TRIGGER IF NOT EXISTS immutable_restore_runs_delete
BEFORE DELETE ON restore_runs
BEGIN SELECT RAISE(ABORT,'IMMUTABLE:restore_runs'); END;

CREATE TRIGGER IF NOT EXISTS guard_backup_record_scope
BEFORE INSERT ON backup_records
WHEN NEW.branch_id IS NULL OR NEW.origin_device_id IS NULL OR NEW.created_by_user_id IS NULL
 OR NOT EXISTS(SELECT 1 FROM branches b WHERE b.id=NEW.branch_id AND b.tenant_id=NEW.tenant_id)
 OR NOT EXISTS(SELECT 1 FROM devices d WHERE d.id=NEW.origin_device_id AND d.tenant_id=NEW.tenant_id AND d.branch_id=NEW.branch_id)
 OR NOT EXISTS(SELECT 1 FROM users u WHERE u.id=NEW.created_by_user_id AND u.tenant_id=NEW.tenant_id)
BEGIN SELECT RAISE(ABORT,'TENANT_SCOPE_VIOLATION:backup_record'); END;
CREATE TRIGGER IF NOT EXISTS guard_backup_event_scope
BEFORE INSERT ON backup_events
WHEN NOT EXISTS(SELECT 1 FROM branches b WHERE b.id=NEW.branch_id AND b.tenant_id=NEW.tenant_id)
 OR NOT EXISTS(SELECT 1 FROM devices d WHERE d.id=NEW.device_id AND d.tenant_id=NEW.tenant_id AND d.branch_id=NEW.branch_id)
 OR NOT EXISTS(SELECT 1 FROM users u WHERE u.id=NEW.user_id AND u.tenant_id=NEW.tenant_id)
 OR (NEW.backup_id IS NOT NULL AND NOT EXISTS(SELECT 1 FROM backup_records r WHERE r.id=NEW.backup_id AND r.tenant_id=NEW.tenant_id))
 OR (NEW.restore_run_id IS NOT NULL AND NOT EXISTS(SELECT 1 FROM restore_runs r WHERE r.id=NEW.restore_run_id AND r.tenant_id=NEW.tenant_id))
BEGIN SELECT RAISE(ABORT,'TENANT_SCOPE_VIOLATION:backup_event'); END;
