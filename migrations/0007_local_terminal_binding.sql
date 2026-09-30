PRAGMA foreign_keys = ON;

-- This row identifies the physical local installation. Synced device rows may
-- contain many terminals, so authority must never be inferred by selecting an
-- arbitrary active device from the business catalogue.
CREATE TABLE IF NOT EXISTS local_terminal_binding (
  singleton INTEGER PRIMARY KEY CHECK(singleton=1),
  tenant_id TEXT NOT NULL REFERENCES tenants(id),
  branch_id TEXT NOT NULL REFERENCES branches(id),
  device_id TEXT NOT NULL REFERENCES devices(id),
  register_id TEXT NOT NULL REFERENCES registers(id),
  installed_at TEXT NOT NULL
);

CREATE TRIGGER IF NOT EXISTS guard_local_terminal_binding_insert BEFORE INSERT ON local_terminal_binding
WHEN NEW.singleton!=1
  OR (SELECT tenant_id FROM branches WHERE id=NEW.branch_id) IS NOT NEW.tenant_id
  OR (SELECT tenant_id FROM devices WHERE id=NEW.device_id) IS NOT NEW.tenant_id
  OR (SELECT branch_id FROM devices WHERE id=NEW.device_id) IS NOT NEW.branch_id
  OR (SELECT tenant_id FROM registers WHERE id=NEW.register_id) IS NOT NEW.tenant_id
  OR (SELECT branch_id FROM registers WHERE id=NEW.register_id) IS NOT NEW.branch_id
BEGIN SELECT RAISE(ABORT,'TENANT_SCOPE_VIOLATION:local_terminal_binding'); END;

CREATE TRIGGER IF NOT EXISTS immutable_local_terminal_binding_update BEFORE UPDATE ON local_terminal_binding
BEGIN SELECT RAISE(ABORT,'IMMUTABLE:local_terminal_binding'); END;
CREATE TRIGGER IF NOT EXISTS immutable_local_terminal_binding_delete BEFORE DELETE ON local_terminal_binding
BEGIN SELECT RAISE(ABORT,'IMMUTABLE:local_terminal_binding'); END;

