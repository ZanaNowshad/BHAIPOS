INSERT INTO permissions(code,description) VALUES('offline_policy.view','View bounded offline authorization policy')
ON CONFLICT(code) DO UPDATE SET description=excluded.description;
INSERT INTO permissions(code,description) VALUES('offline_policy.manage','Configure bounded offline authorization policy')
ON CONFLICT(code) DO UPDATE SET description=excluded.description;
INSERT OR IGNORE INTO role_permissions(role_id,permission_code)
SELECT id,'offline_policy.view' FROM roles WHERE lower(name) IN ('owner','administrator','manager');
INSERT OR IGNORE INTO role_permissions(role_id,permission_code)
SELECT id,'offline_policy.manage' FROM roles WHERE lower(name) IN ('owner','administrator');

CREATE TABLE IF NOT EXISTS offline_policy_versions (
  id TEXT PRIMARY KEY,
  tenant_id TEXT NOT NULL REFERENCES tenants(id),
  branch_id TEXT NOT NULL REFERENCES branches(id),
  version INTEGER NOT NULL CHECK(version>0),
  state TEXT NOT NULL CHECK(state IN ('ACTIVE','RETIRED')),
  offline_login_window_minutes INTEGER NOT NULL CHECK(offline_login_window_minutes BETWEEN 15 AND 10080),
  max_policy_staleness_minutes INTEGER NOT NULL CHECK(max_policy_staleness_minutes BETWEEN 15 AND 43200),
  valid_from TEXT NOT NULL,
  valid_until TEXT NOT NULL,
  operation_id TEXT NOT NULL,
  created_by_user_id TEXT NOT NULL REFERENCES users(id),
  created_on_device_id TEXT NOT NULL REFERENCES devices(id),
  created_at TEXT NOT NULL,
  retired_at TEXT,
  UNIQUE(tenant_id,branch_id,version),
  UNIQUE(tenant_id,operation_id)
);
CREATE UNIQUE INDEX IF NOT EXISTS ux_offline_policy_active_branch
ON offline_policy_versions(tenant_id,branch_id) WHERE state='ACTIVE';

CREATE TABLE IF NOT EXISTS offline_policy_rules (
  policy_id TEXT NOT NULL REFERENCES offline_policy_versions(id),
  tenant_id TEXT NOT NULL REFERENCES tenants(id),
  action_type TEXT NOT NULL,
  decision TEXT NOT NULL CHECK(decision IN ('ALLOW','REQUIRE_MANAGER_APPROVAL','DENY')),
  max_offline_age_minutes INTEGER NOT NULL CHECK(max_offline_age_minutes BETWEEN 0 AND 43200),
  constraints_json TEXT NOT NULL CHECK(json_valid(constraints_json)=1),
  PRIMARY KEY(policy_id,action_type)
);

CREATE TABLE IF NOT EXISTS device_offline_policy_state (
  tenant_id TEXT NOT NULL REFERENCES tenants(id),
  branch_id TEXT NOT NULL REFERENCES branches(id),
  device_id TEXT NOT NULL REFERENCES devices(id),
  policy_id TEXT NOT NULL REFERENCES offline_policy_versions(id),
  policy_version INTEGER NOT NULL CHECK(policy_version>0),
  state TEXT NOT NULL CHECK(state IN ('ACTIVE','STALE','REVOKED')),
  synchronized_at TEXT NOT NULL,
  valid_until TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  PRIMARY KEY(tenant_id,device_id)
);

CREATE INDEX IF NOT EXISTS idx_offline_policy_history
ON offline_policy_versions(tenant_id,branch_id,version DESC);

CREATE TRIGGER IF NOT EXISTS guard_offline_policy_scope BEFORE INSERT ON offline_policy_versions
WHEN NOT EXISTS(SELECT 1 FROM branches b WHERE b.id=NEW.branch_id AND b.tenant_id=NEW.tenant_id)
 OR NOT EXISTS(SELECT 1 FROM users u WHERE u.id=NEW.created_by_user_id AND u.tenant_id=NEW.tenant_id)
 OR NOT EXISTS(SELECT 1 FROM devices d WHERE d.id=NEW.created_on_device_id AND d.tenant_id=NEW.tenant_id AND d.branch_id=NEW.branch_id)
 OR NEW.valid_until<=NEW.valid_from
BEGIN SELECT RAISE(ABORT,'TENANT_SCOPE_VIOLATION:offline_policy'); END;

CREATE TRIGGER IF NOT EXISTS guard_offline_policy_retirement BEFORE UPDATE ON offline_policy_versions
WHEN OLD.id IS NOT NEW.id OR OLD.tenant_id IS NOT NEW.tenant_id OR OLD.branch_id IS NOT NEW.branch_id
 OR OLD.version IS NOT NEW.version OR OLD.offline_login_window_minutes IS NOT NEW.offline_login_window_minutes
 OR OLD.max_policy_staleness_minutes IS NOT NEW.max_policy_staleness_minutes
 OR OLD.valid_from IS NOT NEW.valid_from OR OLD.valid_until IS NOT NEW.valid_until
 OR OLD.operation_id IS NOT NEW.operation_id OR OLD.created_by_user_id IS NOT NEW.created_by_user_id
 OR OLD.created_on_device_id IS NOT NEW.created_on_device_id OR OLD.created_at IS NOT NEW.created_at
 OR OLD.state!='ACTIVE' OR NEW.state!='RETIRED' OR NEW.retired_at IS NULL
BEGIN SELECT RAISE(ABORT,'IMMUTABLE:offline_policy_versions'); END;
CREATE TRIGGER IF NOT EXISTS immutable_offline_policy_delete BEFORE DELETE ON offline_policy_versions
BEGIN SELECT RAISE(ABORT,'IMMUTABLE:offline_policy_versions'); END;

CREATE TRIGGER IF NOT EXISTS guard_offline_policy_rule_scope BEFORE INSERT ON offline_policy_rules
WHEN NOT EXISTS(SELECT 1 FROM offline_policy_versions p WHERE p.id=NEW.policy_id AND p.tenant_id=NEW.tenant_id)
BEGIN SELECT RAISE(ABORT,'TENANT_SCOPE_VIOLATION:offline_policy_rule'); END;
CREATE TRIGGER IF NOT EXISTS immutable_offline_policy_rules_update BEFORE UPDATE ON offline_policy_rules
BEGIN SELECT RAISE(ABORT,'IMMUTABLE:offline_policy_rules'); END;
CREATE TRIGGER IF NOT EXISTS immutable_offline_policy_rules_delete BEFORE DELETE ON offline_policy_rules
BEGIN SELECT RAISE(ABORT,'IMMUTABLE:offline_policy_rules'); END;

CREATE TRIGGER IF NOT EXISTS guard_device_offline_policy_state_insert BEFORE INSERT ON device_offline_policy_state
WHEN NOT EXISTS(SELECT 1 FROM devices d WHERE d.id=NEW.device_id AND d.tenant_id=NEW.tenant_id AND d.branch_id=NEW.branch_id)
 OR NOT EXISTS(SELECT 1 FROM offline_policy_versions p WHERE p.id=NEW.policy_id AND p.tenant_id=NEW.tenant_id AND p.branch_id=NEW.branch_id AND p.version=NEW.policy_version)
BEGIN SELECT RAISE(ABORT,'TENANT_SCOPE_VIOLATION:device_offline_policy_state'); END;
CREATE TRIGGER IF NOT EXISTS guard_device_offline_policy_state_update BEFORE UPDATE ON device_offline_policy_state
WHEN OLD.tenant_id IS NOT NEW.tenant_id OR OLD.branch_id IS NOT NEW.branch_id OR OLD.device_id IS NOT NEW.device_id
 OR NOT EXISTS(SELECT 1 FROM offline_policy_versions p WHERE p.id=NEW.policy_id AND p.tenant_id=NEW.tenant_id AND p.branch_id=NEW.branch_id AND p.version=NEW.policy_version)
BEGIN SELECT RAISE(ABORT,'TENANT_SCOPE_VIOLATION:device_offline_policy_state'); END;
CREATE TRIGGER IF NOT EXISTS immutable_device_offline_policy_state_delete BEFORE DELETE ON device_offline_policy_state
BEGIN SELECT RAISE(ABORT,'IMMUTABLE:device_offline_policy_state'); END;
