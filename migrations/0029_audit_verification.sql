CREATE TABLE IF NOT EXISTS audit_verification_runs (
  id TEXT PRIMARY KEY,
  tenant_id TEXT NOT NULL REFERENCES tenants(id),
  branch_id TEXT NOT NULL REFERENCES branches(id),
  device_id TEXT NOT NULL REFERENCES devices(id),
  actor_user_id TEXT NOT NULL REFERENCES users(id),
  operation_id TEXT NOT NULL,
  chain_valid INTEGER NOT NULL CHECK(chain_valid IN (0,1)),
  event_count INTEGER NOT NULL CHECK(event_count>=0),
  verified_head_hash TEXT,
  alert_id TEXT REFERENCES operational_alerts(id),
  verified_at TEXT NOT NULL,
  UNIQUE(tenant_id,operation_id)
);

CREATE INDEX IF NOT EXISTS ix_audit_verification_device
  ON audit_verification_runs(tenant_id,device_id,verified_at,id);

CREATE TRIGGER IF NOT EXISTS immutable_audit_verification_runs_update BEFORE UPDATE ON audit_verification_runs
BEGIN SELECT RAISE(ABORT,'IMMUTABLE:audit_verification_runs'); END;
CREATE TRIGGER IF NOT EXISTS immutable_audit_verification_runs_delete BEFORE DELETE ON audit_verification_runs
BEGIN SELECT RAISE(ABORT,'IMMUTABLE:audit_verification_runs'); END;
CREATE TRIGGER IF NOT EXISTS guard_audit_verification_run_insert BEFORE INSERT ON audit_verification_runs
WHEN NOT EXISTS(SELECT 1 FROM branches b WHERE b.id=NEW.branch_id AND b.tenant_id=NEW.tenant_id)
 OR NOT EXISTS(SELECT 1 FROM devices d WHERE d.id=NEW.device_id AND d.tenant_id=NEW.tenant_id AND d.branch_id=NEW.branch_id AND d.status='ACTIVE')
 OR NOT EXISTS(SELECT 1 FROM users u WHERE u.id=NEW.actor_user_id AND u.tenant_id=NEW.tenant_id AND u.status='ACTIVE')
 OR (NEW.chain_valid=1 AND NEW.alert_id IS NOT NULL)
 OR (NEW.chain_valid=0 AND NEW.alert_id IS NULL)
BEGIN SELECT RAISE(ABORT,'AUDIT_VERIFICATION_SCOPE_VIOLATION'); END;

INSERT INTO permissions(code,description) VALUES('audit.verify','Verify the local tamper-evident audit chain')
ON CONFLICT(code) DO UPDATE SET description=excluded.description;
INSERT OR IGNORE INTO role_permissions(role_id,permission_code)
SELECT id,'audit.verify' FROM roles WHERE lower(name) IN ('owner','administrator','admin');
