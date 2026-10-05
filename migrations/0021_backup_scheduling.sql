INSERT INTO permissions(code,description) VALUES('backup.schedule','Configure scheduled backups')
ON CONFLICT(code) DO UPDATE SET description=excluded.description;
INSERT OR IGNORE INTO role_permissions(role_id,permission_code)
SELECT id,'backup.schedule' FROM roles WHERE lower(name) IN ('owner','administrator');

CREATE TABLE IF NOT EXISTS backup_schedules (
  id TEXT PRIMARY KEY,
  tenant_id TEXT NOT NULL REFERENCES tenants(id),
  branch_id TEXT NOT NULL REFERENCES branches(id),
  device_id TEXT NOT NULL REFERENCES devices(id),
  state TEXT NOT NULL CHECK(state IN ('ACTIVE','DISABLED','REQUIRES_REVIEW')),
  interval_minutes INTEGER NOT NULL CHECK(interval_minutes BETWEEN 60 AND 10080),
  retention_count INTEGER NOT NULL CHECK(retention_count BETWEEN 1 AND 365),
  next_run_at TEXT NOT NULL,
  authorization_expires_at TEXT NOT NULL,
  authorized_by_user_id TEXT NOT NULL REFERENCES users(id),
  version INTEGER NOT NULL DEFAULT 1 CHECK(version > 0),
  last_enqueued_at TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  UNIQUE(tenant_id,device_id)
);

CREATE TABLE IF NOT EXISTS backup_schedule_events (
  id TEXT PRIMARY KEY,
  tenant_id TEXT NOT NULL REFERENCES tenants(id),
  branch_id TEXT NOT NULL REFERENCES branches(id),
  schedule_id TEXT NOT NULL REFERENCES backup_schedules(id),
  operation_id TEXT NOT NULL,
  event_type TEXT NOT NULL,
  previous_state TEXT,
  new_state TEXT NOT NULL,
  device_id TEXT NOT NULL REFERENCES devices(id),
  user_id TEXT NOT NULL REFERENCES users(id),
  evidence_json TEXT NOT NULL,
  created_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS backup_schedule_operation_results (
  tenant_id TEXT NOT NULL REFERENCES tenants(id),
  operation_id TEXT NOT NULL,
  action TEXT NOT NULL CHECK(action IN ('CONFIGURE','ENQUEUE_DUE')),
  request_sha256 TEXT NOT NULL CHECK(length(request_sha256)=64),
  result_json TEXT NOT NULL,
  committed_at TEXT NOT NULL,
  PRIMARY KEY(tenant_id,operation_id)
);

CREATE INDEX IF NOT EXISTS ix_backup_schedules_due
  ON backup_schedules(tenant_id,branch_id,device_id,state,next_run_at);
CREATE INDEX IF NOT EXISTS ix_backup_schedule_events_history
  ON backup_schedule_events(tenant_id,schedule_id,created_at,id);

CREATE TRIGGER IF NOT EXISTS immutable_backup_schedule_events_update
BEFORE UPDATE ON backup_schedule_events
BEGIN SELECT RAISE(ABORT,'IMMUTABLE:backup_schedule_events'); END;
CREATE TRIGGER IF NOT EXISTS immutable_backup_schedule_events_delete
BEFORE DELETE ON backup_schedule_events
BEGIN SELECT RAISE(ABORT,'IMMUTABLE:backup_schedule_events'); END;
CREATE TRIGGER IF NOT EXISTS immutable_backup_schedule_operation_results_update
BEFORE UPDATE ON backup_schedule_operation_results
BEGIN SELECT RAISE(ABORT,'IMMUTABLE:backup_schedule_operation_results'); END;
CREATE TRIGGER IF NOT EXISTS immutable_backup_schedule_operation_results_delete
BEFORE DELETE ON backup_schedule_operation_results
BEGIN SELECT RAISE(ABORT,'IMMUTABLE:backup_schedule_operation_results'); END;
CREATE TRIGGER IF NOT EXISTS immutable_backup_schedule_identity
BEFORE UPDATE ON backup_schedules
WHEN NEW.id<>OLD.id OR NEW.tenant_id<>OLD.tenant_id OR NEW.branch_id<>OLD.branch_id OR NEW.device_id<>OLD.device_id OR NEW.created_at<>OLD.created_at
BEGIN SELECT RAISE(ABORT,'IMMUTABLE:backup_schedule_identity'); END;
CREATE TRIGGER IF NOT EXISTS immutable_backup_schedules_delete
BEFORE DELETE ON backup_schedules
BEGIN SELECT RAISE(ABORT,'IMMUTABLE:backup_schedules'); END;
CREATE TRIGGER IF NOT EXISTS guard_backup_schedule_scope_insert
BEFORE INSERT ON backup_schedules
WHEN NOT EXISTS(SELECT 1 FROM branches b WHERE b.id=NEW.branch_id AND b.tenant_id=NEW.tenant_id)
 OR NOT EXISTS(SELECT 1 FROM devices d WHERE d.id=NEW.device_id AND d.tenant_id=NEW.tenant_id AND d.branch_id=NEW.branch_id)
 OR NOT EXISTS(SELECT 1 FROM users u WHERE u.id=NEW.authorized_by_user_id AND u.tenant_id=NEW.tenant_id)
BEGIN SELECT RAISE(ABORT,'TENANT_SCOPE_VIOLATION:backup_schedule'); END;
CREATE TRIGGER IF NOT EXISTS guard_backup_schedule_scope_update
BEFORE UPDATE ON backup_schedules
WHEN NOT EXISTS(SELECT 1 FROM users u WHERE u.id=NEW.authorized_by_user_id AND u.tenant_id=NEW.tenant_id)
 OR NEW.interval_minutes NOT BETWEEN 60 AND 10080 OR NEW.retention_count NOT BETWEEN 1 AND 365
 OR NEW.version<=OLD.version
BEGIN SELECT RAISE(ABORT,'INVALID:backup_schedule_update'); END;
CREATE TRIGGER IF NOT EXISTS guard_backup_schedule_event_scope
BEFORE INSERT ON backup_schedule_events
WHEN NOT EXISTS(SELECT 1 FROM backup_schedules s WHERE s.id=NEW.schedule_id AND s.tenant_id=NEW.tenant_id AND s.branch_id=NEW.branch_id AND s.device_id=NEW.device_id)
 OR NOT EXISTS(SELECT 1 FROM users u WHERE u.id=NEW.user_id AND u.tenant_id=NEW.tenant_id)
BEGIN SELECT RAISE(ABORT,'TENANT_SCOPE_VIOLATION:backup_schedule_event'); END;
