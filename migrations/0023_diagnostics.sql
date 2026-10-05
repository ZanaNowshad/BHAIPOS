INSERT INTO permissions(code,description) VALUES('diagnostics.view','View redacted operational diagnostics')
ON CONFLICT(code) DO UPDATE SET description=excluded.description;
INSERT OR IGNORE INTO role_permissions(role_id,permission_code)
SELECT id,'diagnostics.view' FROM roles WHERE lower(name) IN ('owner','administrator');

CREATE TRIGGER IF NOT EXISTS guard_diagnostics_snapshot_scope BEFORE INSERT ON diagnostics_snapshots
WHEN NEW.branch_id IS NULL OR NEW.device_id IS NULL OR json_valid(NEW.payload_json)!=1
 OR NOT EXISTS(SELECT 1 FROM branches b WHERE b.id=NEW.branch_id AND b.tenant_id=NEW.tenant_id)
 OR NOT EXISTS(SELECT 1 FROM devices d WHERE d.id=NEW.device_id AND d.tenant_id=NEW.tenant_id AND d.branch_id=NEW.branch_id)
BEGIN SELECT RAISE(ABORT,'TENANT_SCOPE_VIOLATION:diagnostics_snapshot'); END;
CREATE TRIGGER IF NOT EXISTS immutable_diagnostics_snapshots_update BEFORE UPDATE ON diagnostics_snapshots
BEGIN SELECT RAISE(ABORT,'IMMUTABLE:diagnostics_snapshots'); END;
CREATE TRIGGER IF NOT EXISTS immutable_diagnostics_snapshots_delete BEFORE DELETE ON diagnostics_snapshots
BEGIN SELECT RAISE(ABORT,'IMMUTABLE:diagnostics_snapshots'); END;
