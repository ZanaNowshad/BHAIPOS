INSERT INTO permissions(code,description) VALUES('backup.retention','Prune scheduled backups under configured retention policy')
ON CONFLICT(code) DO UPDATE SET description=excluded.description;
INSERT OR IGNORE INTO role_permissions(role_id,permission_code)
SELECT id,'backup.retention' FROM roles WHERE lower(name) IN ('owner','administrator');

CREATE TABLE IF NOT EXISTS scheduled_backup_outputs (
  tenant_id TEXT NOT NULL REFERENCES tenants(id), branch_id TEXT NOT NULL REFERENCES branches(id),
  schedule_id TEXT NOT NULL REFERENCES backup_schedules(id), backup_id TEXT NOT NULL UNIQUE REFERENCES backup_records(id),
  job_id TEXT NOT NULL UNIQUE REFERENCES background_jobs(id), device_id TEXT NOT NULL REFERENCES devices(id),
  user_id TEXT NOT NULL REFERENCES users(id), created_at TEXT NOT NULL,
  PRIMARY KEY(tenant_id,schedule_id,backup_id)
);
CREATE TABLE IF NOT EXISTS backup_retention_runs (
  id TEXT PRIMARY KEY, tenant_id TEXT NOT NULL REFERENCES tenants(id), branch_id TEXT NOT NULL REFERENCES branches(id),
  schedule_id TEXT NOT NULL REFERENCES backup_schedules(id), operation_id TEXT NOT NULL, request_sha256 TEXT NOT NULL CHECK(length(request_sha256)=64),
  trusted_directory TEXT NOT NULL, retention_count INTEGER NOT NULL CHECK(retention_count BETWEEN 1 AND 365),
  retained_count INTEGER NOT NULL CHECK(retained_count>=0), protected_count INTEGER NOT NULL CHECK(protected_count>=0),
  state TEXT NOT NULL CHECK(state IN ('RUNNING','SUCCEEDED','FAILED')), device_id TEXT NOT NULL REFERENCES devices(id),
  user_id TEXT NOT NULL REFERENCES users(id), created_at TEXT NOT NULL, completed_at TEXT, result_json TEXT,
  UNIQUE(tenant_id,operation_id)
);
CREATE TABLE IF NOT EXISTS backup_retention_items (
  run_id TEXT NOT NULL REFERENCES backup_retention_runs(id), tenant_id TEXT NOT NULL REFERENCES tenants(id),
  backup_id TEXT NOT NULL REFERENCES backup_records(id), storage_path TEXT NOT NULL,
  state TEXT NOT NULL CHECK(state IN ('PENDING','PRUNED','FAILED')), error_text TEXT, updated_at TEXT NOT NULL,
  PRIMARY KEY(run_id,backup_id)
);
CREATE TABLE IF NOT EXISTS backup_retention_operation_results (
  tenant_id TEXT NOT NULL REFERENCES tenants(id), operation_id TEXT NOT NULL, request_sha256 TEXT NOT NULL CHECK(length(request_sha256)=64),
  result_json TEXT NOT NULL, committed_at TEXT NOT NULL, PRIMARY KEY(tenant_id,operation_id)
);
CREATE INDEX IF NOT EXISTS ix_scheduled_backup_outputs_history ON scheduled_backup_outputs(tenant_id,schedule_id,created_at DESC,backup_id DESC);
CREATE INDEX IF NOT EXISTS ix_backup_retention_items_state ON backup_retention_items(tenant_id,run_id,state,backup_id);

CREATE TRIGGER IF NOT EXISTS immutable_scheduled_backup_outputs_update BEFORE UPDATE ON scheduled_backup_outputs BEGIN SELECT RAISE(ABORT,'IMMUTABLE:scheduled_backup_outputs'); END;
CREATE TRIGGER IF NOT EXISTS immutable_scheduled_backup_outputs_delete BEFORE DELETE ON scheduled_backup_outputs BEGIN SELECT RAISE(ABORT,'IMMUTABLE:scheduled_backup_outputs'); END;
CREATE TRIGGER IF NOT EXISTS immutable_backup_retention_operation_results_update BEFORE UPDATE ON backup_retention_operation_results BEGIN SELECT RAISE(ABORT,'IMMUTABLE:backup_retention_operation_results'); END;
CREATE TRIGGER IF NOT EXISTS immutable_backup_retention_operation_results_delete BEFORE DELETE ON backup_retention_operation_results BEGIN SELECT RAISE(ABORT,'IMMUTABLE:backup_retention_operation_results'); END;
CREATE TRIGGER IF NOT EXISTS immutable_backup_retention_run_identity BEFORE UPDATE ON backup_retention_runs
WHEN NEW.id<>OLD.id OR NEW.tenant_id<>OLD.tenant_id OR NEW.branch_id<>OLD.branch_id OR NEW.schedule_id<>OLD.schedule_id
 OR NEW.operation_id<>OLD.operation_id OR NEW.request_sha256<>OLD.request_sha256 OR NEW.trusted_directory<>OLD.trusted_directory
 OR NEW.retention_count<>OLD.retention_count OR NEW.retained_count<>OLD.retained_count OR NEW.protected_count<>OLD.protected_count
 OR NEW.device_id<>OLD.device_id OR NEW.user_id<>OLD.user_id OR NEW.created_at<>OLD.created_at
BEGIN SELECT RAISE(ABORT,'IMMUTABLE:backup_retention_run_identity'); END;
CREATE TRIGGER IF NOT EXISTS immutable_backup_retention_runs_delete BEFORE DELETE ON backup_retention_runs BEGIN SELECT RAISE(ABORT,'IMMUTABLE:backup_retention_runs'); END;
CREATE TRIGGER IF NOT EXISTS guard_backup_retention_run_update BEFORE UPDATE ON backup_retention_runs
WHEN NOT (OLD.state='RUNNING' AND NEW.state IN ('SUCCEEDED','FAILED')) OR NEW.completed_at IS NULL OR NEW.result_json IS NULL OR json_valid(NEW.result_json)!=1
BEGIN SELECT RAISE(ABORT,'INVALID:backup_retention_run_transition'); END;
CREATE TRIGGER IF NOT EXISTS immutable_backup_retention_item_identity BEFORE UPDATE ON backup_retention_items
WHEN NEW.run_id<>OLD.run_id OR NEW.tenant_id<>OLD.tenant_id OR NEW.backup_id<>OLD.backup_id OR NEW.storage_path<>OLD.storage_path
BEGIN SELECT RAISE(ABORT,'IMMUTABLE:backup_retention_item_identity'); END;
CREATE TRIGGER IF NOT EXISTS immutable_backup_retention_items_delete BEFORE DELETE ON backup_retention_items BEGIN SELECT RAISE(ABORT,'IMMUTABLE:backup_retention_items'); END;
CREATE TRIGGER IF NOT EXISTS guard_backup_retention_item_update BEFORE UPDATE ON backup_retention_items
WHEN OLD.state!='PENDING' OR NEW.state NOT IN ('PRUNED','FAILED') OR (NEW.state='PRUNED' AND NEW.error_text IS NOT NULL)
 OR (NEW.state='FAILED' AND (NEW.error_text IS NULL OR length(trim(NEW.error_text))=0))
BEGIN SELECT RAISE(ABORT,'INVALID:backup_retention_item_transition'); END;
CREATE TRIGGER IF NOT EXISTS guard_scheduled_backup_output_scope BEFORE INSERT ON scheduled_backup_outputs
WHEN NOT EXISTS(SELECT 1 FROM backup_schedules s WHERE s.id=NEW.schedule_id AND s.tenant_id=NEW.tenant_id AND s.branch_id=NEW.branch_id AND s.device_id=NEW.device_id)
 OR NOT EXISTS(SELECT 1 FROM backup_records b WHERE b.id=NEW.backup_id AND b.tenant_id=NEW.tenant_id AND b.branch_id=NEW.branch_id AND b.origin_device_id=NEW.device_id AND b.created_by_user_id=NEW.user_id AND b.backup_type='SCHEDULED')
 OR NOT EXISTS(SELECT 1 FROM background_jobs j WHERE j.id=NEW.job_id AND j.tenant_id=NEW.tenant_id AND j.branch_id=NEW.branch_id AND j.origin_device_id=NEW.device_id AND j.created_by_user_id=NEW.user_id AND j.job_type='BACKUP_CREATE')
BEGIN SELECT RAISE(ABORT,'TENANT_SCOPE_VIOLATION:scheduled_backup_output'); END;
CREATE TRIGGER IF NOT EXISTS guard_backup_retention_run_scope BEFORE INSERT ON backup_retention_runs
WHEN NOT EXISTS(SELECT 1 FROM backup_schedules s WHERE s.id=NEW.schedule_id AND s.tenant_id=NEW.tenant_id AND s.branch_id=NEW.branch_id AND s.device_id=NEW.device_id)
 OR NOT EXISTS(SELECT 1 FROM users u WHERE u.id=NEW.user_id AND u.tenant_id=NEW.tenant_id)
BEGIN SELECT RAISE(ABORT,'TENANT_SCOPE_VIOLATION:backup_retention_run'); END;
CREATE TRIGGER IF NOT EXISTS guard_backup_retention_item_scope BEFORE INSERT ON backup_retention_items
WHEN NOT EXISTS(SELECT 1 FROM backup_retention_runs r WHERE r.id=NEW.run_id AND r.tenant_id=NEW.tenant_id)
 OR NOT EXISTS(SELECT 1 FROM backup_records b WHERE b.id=NEW.backup_id AND b.tenant_id=NEW.tenant_id AND b.storage_path=NEW.storage_path)
BEGIN SELECT RAISE(ABORT,'TENANT_SCOPE_VIOLATION:backup_retention_item'); END;
