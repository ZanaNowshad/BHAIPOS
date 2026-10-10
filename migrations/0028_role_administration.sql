CREATE TABLE IF NOT EXISTS custom_role_profiles (
  role_id TEXT PRIMARY KEY REFERENCES roles(id),
  tenant_id TEXT NOT NULL REFERENCES tenants(id),
  version INTEGER NOT NULL CHECK(version>0),
  created_by_user_id TEXT NOT NULL REFERENCES users(id),
  created_on_device_id TEXT NOT NULL REFERENCES devices(id),
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS role_administration_events (
  id TEXT PRIMARY KEY,
  tenant_id TEXT NOT NULL REFERENCES tenants(id),
  branch_id TEXT NOT NULL REFERENCES branches(id),
  role_id TEXT NOT NULL REFERENCES roles(id),
  target_user_id TEXT REFERENCES users(id),
  actor_user_id TEXT NOT NULL REFERENCES users(id),
  device_id TEXT NOT NULL REFERENCES devices(id),
  operation_id TEXT NOT NULL,
  event_type TEXT NOT NULL CHECK(event_type IN ('ROLE_CREATED','PERMISSIONS_REPLACED','ROLE_ASSIGNED','ROLE_UNASSIGNED')),
  previous_json TEXT NOT NULL CHECK(json_valid(previous_json)=1),
  new_json TEXT NOT NULL CHECK(json_valid(new_json)=1),
  reason TEXT NOT NULL,
  created_at TEXT NOT NULL,
  UNIQUE(tenant_id,operation_id)
);

CREATE INDEX IF NOT EXISTS ix_role_administration_history
  ON role_administration_events(tenant_id,role_id,created_at,id);

CREATE TRIGGER IF NOT EXISTS immutable_role_administration_events_update BEFORE UPDATE ON role_administration_events
BEGIN SELECT RAISE(ABORT,'IMMUTABLE:role_administration_events'); END;
CREATE TRIGGER IF NOT EXISTS immutable_role_administration_events_delete BEFORE DELETE ON role_administration_events
BEGIN SELECT RAISE(ABORT,'IMMUTABLE:role_administration_events'); END;

CREATE TRIGGER IF NOT EXISTS guard_custom_role_profile_insert BEFORE INSERT ON custom_role_profiles
WHEN NOT EXISTS(SELECT 1 FROM roles r WHERE r.id=NEW.role_id AND r.tenant_id=NEW.tenant_id)
 OR NOT EXISTS(SELECT 1 FROM users u WHERE u.id=NEW.created_by_user_id AND u.tenant_id=NEW.tenant_id AND u.status='ACTIVE')
 OR NOT EXISTS(SELECT 1 FROM devices d WHERE d.id=NEW.created_on_device_id AND d.tenant_id=NEW.tenant_id AND d.status='ACTIVE')
BEGIN SELECT RAISE(ABORT,'ROLE_ADMIN_SCOPE_VIOLATION'); END;

CREATE TRIGGER IF NOT EXISTS guard_custom_role_profile_update BEFORE UPDATE ON custom_role_profiles
WHEN OLD.role_id IS NOT NEW.role_id OR OLD.tenant_id IS NOT NEW.tenant_id
 OR OLD.created_by_user_id IS NOT NEW.created_by_user_id OR OLD.created_on_device_id IS NOT NEW.created_on_device_id
 OR OLD.created_at IS NOT NEW.created_at OR NEW.version!=OLD.version+1 OR NEW.updated_at<=OLD.updated_at
BEGIN SELECT RAISE(ABORT,'INVALID_ROLE_VERSION'); END;
CREATE TRIGGER IF NOT EXISTS immutable_custom_role_profile_delete BEFORE DELETE ON custom_role_profiles
BEGIN SELECT RAISE(ABORT,'IMMUTABLE:custom_role_profiles'); END;

CREATE TRIGGER IF NOT EXISTS guard_role_administration_event_insert BEFORE INSERT ON role_administration_events
WHEN NOT EXISTS(SELECT 1 FROM branches b WHERE b.id=NEW.branch_id AND b.tenant_id=NEW.tenant_id)
 OR NOT EXISTS(SELECT 1 FROM roles r WHERE r.id=NEW.role_id AND r.tenant_id=NEW.tenant_id)
 OR (NEW.target_user_id IS NOT NULL AND NOT EXISTS(SELECT 1 FROM users u WHERE u.id=NEW.target_user_id AND u.tenant_id=NEW.tenant_id))
 OR NOT EXISTS(SELECT 1 FROM users u WHERE u.id=NEW.actor_user_id AND u.tenant_id=NEW.tenant_id AND u.status='ACTIVE')
 OR NOT EXISTS(SELECT 1 FROM devices d WHERE d.id=NEW.device_id AND d.tenant_id=NEW.tenant_id AND d.branch_id=NEW.branch_id AND d.status='ACTIVE')
 OR length(trim(NEW.reason))<3
BEGIN SELECT RAISE(ABORT,'ROLE_ADMIN_SCOPE_VIOLATION'); END;

INSERT INTO permissions(code,description) VALUES('role.manage','Configure custom roles and branch assignments')
ON CONFLICT(code) DO UPDATE SET description=excluded.description;
INSERT OR IGNORE INTO role_permissions(role_id,permission_code)
SELECT id,'role.manage' FROM roles WHERE lower(name) IN ('owner','administrator','admin');
