INSERT INTO permissions(code,description) VALUES('diagnostics.export','Capture and export redacted operational diagnostics')
ON CONFLICT(code) DO UPDATE SET description=excluded.description;
INSERT OR IGNORE INTO role_permissions(role_id,permission_code)
SELECT id,'diagnostics.export' FROM roles WHERE lower(name) IN ('owner','administrator');

CREATE TABLE IF NOT EXISTS diagnostic_snapshot_operations (
  tenant_id TEXT NOT NULL REFERENCES tenants(id),
  operation_id TEXT NOT NULL,
  action TEXT NOT NULL CHECK(action='CAPTURE_REDACTED_DIAGNOSTICS'),
  request_sha256 TEXT NOT NULL CHECK(length(request_sha256)=64),
  snapshot_id TEXT NOT NULL UNIQUE REFERENCES diagnostics_snapshots(id),
  branch_id TEXT NOT NULL REFERENCES branches(id),
  device_id TEXT NOT NULL REFERENCES devices(id),
  user_id TEXT NOT NULL REFERENCES users(id),
  payload_sha256 TEXT NOT NULL CHECK(length(payload_sha256)=64),
  committed_at TEXT NOT NULL,
  PRIMARY KEY(tenant_id,operation_id)
);

CREATE INDEX IF NOT EXISTS idx_diagnostic_snapshots_scope_created
ON diagnostics_snapshots(tenant_id,branch_id,device_id,created_at DESC,id DESC);

CREATE TRIGGER IF NOT EXISTS guard_diagnostic_snapshot_operation_scope BEFORE INSERT ON diagnostic_snapshot_operations
WHEN NOT EXISTS(SELECT 1 FROM branches b WHERE b.id=NEW.branch_id AND b.tenant_id=NEW.tenant_id)
 OR NOT EXISTS(SELECT 1 FROM devices d WHERE d.id=NEW.device_id AND d.tenant_id=NEW.tenant_id AND d.branch_id=NEW.branch_id)
 OR NOT EXISTS(SELECT 1 FROM users u WHERE u.id=NEW.user_id AND u.tenant_id=NEW.tenant_id)
 OR NOT EXISTS(SELECT 1 FROM diagnostics_snapshots s WHERE s.id=NEW.snapshot_id AND s.tenant_id=NEW.tenant_id AND s.branch_id=NEW.branch_id AND s.device_id=NEW.device_id)
BEGIN SELECT RAISE(ABORT,'TENANT_SCOPE_VIOLATION:diagnostic_snapshot_operation'); END;

CREATE TRIGGER IF NOT EXISTS immutable_diagnostic_snapshot_operations_update BEFORE UPDATE ON diagnostic_snapshot_operations
BEGIN SELECT RAISE(ABORT,'IMMUTABLE:diagnostic_snapshot_operations'); END;
CREATE TRIGGER IF NOT EXISTS immutable_diagnostic_snapshot_operations_delete BEFORE DELETE ON diagnostic_snapshot_operations
BEGIN SELECT RAISE(ABORT,'IMMUTABLE:diagnostic_snapshot_operations'); END;
