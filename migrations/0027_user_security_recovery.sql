CREATE TABLE IF NOT EXISTS user_security_events (
  id TEXT PRIMARY KEY,
  tenant_id TEXT NOT NULL REFERENCES tenants(id),
  branch_id TEXT NOT NULL REFERENCES branches(id),
  target_user_id TEXT NOT NULL REFERENCES users(id),
  actor_user_id TEXT NOT NULL REFERENCES users(id),
  device_id TEXT NOT NULL REFERENCES devices(id),
  operation_id TEXT NOT NULL,
  event_type TEXT NOT NULL CHECK(event_type IN ('PIN_RESET','ACCOUNT_UNLOCKED','ACCOUNT_SUSPENDED','ACCOUNT_REACTIVATED')),
  request_pin_hash TEXT,
  previous_status TEXT NOT NULL CHECK(previous_status IN ('ACTIVE','LOCKED','SUSPENDED')),
  new_status TEXT NOT NULL CHECK(new_status IN ('ACTIVE','LOCKED','SUSPENDED')),
  reason TEXT NOT NULL,
  created_at TEXT NOT NULL,
  UNIQUE(tenant_id,operation_id),
  CHECK((event_type='PIN_RESET' AND request_pin_hash IS NOT NULL) OR (event_type!='PIN_RESET' AND request_pin_hash IS NULL))
);

CREATE INDEX IF NOT EXISTS ix_user_security_events_target
  ON user_security_events(tenant_id,target_user_id,created_at);

CREATE TRIGGER IF NOT EXISTS immutable_user_security_events_update BEFORE UPDATE ON user_security_events
BEGIN SELECT RAISE(ABORT,'IMMUTABLE:user_security_events'); END;
CREATE TRIGGER IF NOT EXISTS immutable_user_security_events_delete BEFORE DELETE ON user_security_events
BEGIN SELECT RAISE(ABORT,'IMMUTABLE:user_security_events'); END;

CREATE TRIGGER IF NOT EXISTS guard_user_security_event_insert BEFORE INSERT ON user_security_events
WHEN NOT EXISTS(SELECT 1 FROM branches b WHERE b.id=NEW.branch_id AND b.tenant_id=NEW.tenant_id)
 OR NOT EXISTS(SELECT 1 FROM devices d WHERE d.id=NEW.device_id AND d.tenant_id=NEW.tenant_id AND d.branch_id=NEW.branch_id AND d.status='ACTIVE')
 OR NOT EXISTS(SELECT 1 FROM users u WHERE u.id=NEW.target_user_id AND u.tenant_id=NEW.tenant_id)
 OR NOT EXISTS(SELECT 1 FROM users u WHERE u.id=NEW.actor_user_id AND u.tenant_id=NEW.tenant_id AND u.status='ACTIVE')
 OR length(trim(NEW.reason))=0
BEGIN SELECT RAISE(ABORT,'USER_SECURITY_SCOPE_VIOLATION'); END;

INSERT INTO permissions(code,description) VALUES('employee.security.manage','Suspend, reactivate, unlock and reset staff credentials')
ON CONFLICT(code) DO UPDATE SET description=excluded.description;
INSERT OR IGNORE INTO role_permissions(role_id,permission_code)
SELECT r.id,'employee.security.manage' FROM roles r
WHERE lower(r.name) IN ('owner','administrator','admin');
