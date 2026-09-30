CREATE TABLE IF NOT EXISTS sync_delivery_leases (
  mutation_id TEXT PRIMARY KEY REFERENCES sync_queue(id) ON DELETE CASCADE,
  lease_token TEXT NOT NULL UNIQUE,
  leased_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS hub_mutations (
  hub_sequence INTEGER PRIMARY KEY AUTOINCREMENT,
  tenant_id TEXT NOT NULL REFERENCES tenants(id),
  branch_id TEXT NOT NULL REFERENCES branches(id),
  device_id TEXT NOT NULL REFERENCES devices(id),
  mutation_id TEXT NOT NULL,
  operation_id TEXT NOT NULL,
  entity_type TEXT NOT NULL,
  entity_id TEXT NOT NULL,
  mutation_type TEXT NOT NULL,
  payload_json TEXT NOT NULL,
  payload_sha256 TEXT NOT NULL,
  request_sha256 TEXT NOT NULL,
  credential_version INTEGER NOT NULL,
  state TEXT NOT NULL CHECK(state IN ('COMMITTED','REQUIRES_REVIEW','REJECTED')),
  result_json TEXT NOT NULL,
  received_at TEXT NOT NULL,
  UNIQUE(tenant_id,device_id,mutation_id),
  UNIQUE(tenant_id,device_id,operation_id,entity_type,entity_id)
);

CREATE TABLE IF NOT EXISTS device_sync_checkpoints (
  tenant_id TEXT NOT NULL REFERENCES tenants(id),
  branch_id TEXT NOT NULL REFERENCES branches(id),
  device_id TEXT NOT NULL REFERENCES devices(id),
  last_hub_sequence INTEGER NOT NULL DEFAULT 0,
  last_mutation_id TEXT,
  updated_at TEXT NOT NULL,
  PRIMARY KEY(tenant_id,device_id)
);

CREATE TABLE IF NOT EXISTS sync_conflict_resolutions (
  id TEXT PRIMARY KEY,
  tenant_id TEXT NOT NULL REFERENCES tenants(id),
  hub_sequence INTEGER NOT NULL REFERENCES hub_mutations(hub_sequence),
  resolution TEXT NOT NULL CHECK(resolution IN ('APPLY','COMPENSATE','REJECT')),
  notes TEXT NOT NULL,
  resolved_by_user_id TEXT NOT NULL REFERENCES users(id),
  resolved_at TEXT NOT NULL,
  UNIQUE(tenant_id,hub_sequence)
);

CREATE TABLE IF NOT EXISTS sync_delivery_errors (
  id TEXT PRIMARY KEY,
  mutation_id TEXT NOT NULL REFERENCES sync_queue(id),
  state TEXT NOT NULL CHECK(state IN ('RETRYING','FAILED')),
  error TEXT NOT NULL,
  created_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS sync_retry_schedule (
  mutation_id TEXT PRIMARY KEY REFERENCES sync_queue(id) ON DELETE CASCADE,
  next_attempt_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS device_enrollment_grants (
  id TEXT PRIMARY KEY,
  tenant_id TEXT NOT NULL REFERENCES tenants(id),
  branch_id TEXT NOT NULL REFERENCES branches(id),
  issued_from_device_id TEXT NOT NULL REFERENCES devices(id),
  issued_by_user_id TEXT NOT NULL REFERENCES users(id),
  token_hash TEXT NOT NULL,
  friendly_name TEXT NOT NULL,
  expires_at TEXT NOT NULL,
  created_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS device_enrollment_consumptions (
  grant_id TEXT PRIMARY KEY REFERENCES device_enrollment_grants(id),
  device_id TEXT NOT NULL UNIQUE REFERENCES devices(id),
  consumed_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS ix_hub_mutations_checkpoint ON hub_mutations(tenant_id,hub_sequence);
CREATE INDEX IF NOT EXISTS ix_hub_mutations_review ON hub_mutations(tenant_id,state,received_at);

CREATE TRIGGER IF NOT EXISTS immutable_hub_mutations_update BEFORE UPDATE ON hub_mutations BEGIN SELECT RAISE(ABORT,'IMMUTABLE:hub_mutations'); END;
CREATE TRIGGER IF NOT EXISTS immutable_hub_mutations_delete BEFORE DELETE ON hub_mutations BEGIN SELECT RAISE(ABORT,'IMMUTABLE:hub_mutations'); END;
CREATE TRIGGER IF NOT EXISTS immutable_sync_conflict_resolutions_update BEFORE UPDATE ON sync_conflict_resolutions BEGIN SELECT RAISE(ABORT,'IMMUTABLE:sync_conflict_resolutions'); END;
CREATE TRIGGER IF NOT EXISTS immutable_sync_conflict_resolutions_delete BEFORE DELETE ON sync_conflict_resolutions BEGIN SELECT RAISE(ABORT,'IMMUTABLE:sync_conflict_resolutions'); END;
CREATE TRIGGER IF NOT EXISTS immutable_sync_delivery_errors_update BEFORE UPDATE ON sync_delivery_errors BEGIN SELECT RAISE(ABORT,'IMMUTABLE:sync_delivery_errors'); END;
CREATE TRIGGER IF NOT EXISTS immutable_sync_delivery_errors_delete BEFORE DELETE ON sync_delivery_errors BEGIN SELECT RAISE(ABORT,'IMMUTABLE:sync_delivery_errors'); END;
CREATE TRIGGER IF NOT EXISTS immutable_device_enrollment_grants_update BEFORE UPDATE ON device_enrollment_grants BEGIN SELECT RAISE(ABORT,'IMMUTABLE:device_enrollment_grants'); END;
CREATE TRIGGER IF NOT EXISTS immutable_device_enrollment_grants_delete BEFORE DELETE ON device_enrollment_grants BEGIN SELECT RAISE(ABORT,'IMMUTABLE:device_enrollment_grants'); END;
CREATE TRIGGER IF NOT EXISTS immutable_device_enrollment_consumptions_update BEFORE UPDATE ON device_enrollment_consumptions BEGIN SELECT RAISE(ABORT,'IMMUTABLE:device_enrollment_consumptions'); END;
CREATE TRIGGER IF NOT EXISTS immutable_device_enrollment_consumptions_delete BEFORE DELETE ON device_enrollment_consumptions BEGIN SELECT RAISE(ABORT,'IMMUTABLE:device_enrollment_consumptions'); END;

CREATE TRIGGER IF NOT EXISTS guard_hub_mutations_insert BEFORE INSERT ON hub_mutations
WHEN NOT EXISTS (SELECT 1 FROM devices d WHERE d.id=NEW.device_id AND d.tenant_id=NEW.tenant_id AND d.branch_id=NEW.branch_id)
BEGIN SELECT RAISE(ABORT,'TENANT_SCOPE_VIOLATION:hub_mutations'); END;

CREATE TRIGGER IF NOT EXISTS guard_device_sync_checkpoints_insert BEFORE INSERT ON device_sync_checkpoints
WHEN NOT EXISTS (SELECT 1 FROM devices d WHERE d.id=NEW.device_id AND d.tenant_id=NEW.tenant_id AND d.branch_id=NEW.branch_id)
BEGIN SELECT RAISE(ABORT,'TENANT_SCOPE_VIOLATION:device_sync_checkpoints'); END;

INSERT INTO permissions(code,description)
VALUES('sync.resolve','Resolve synchronization conflicts')
ON CONFLICT(code) DO UPDATE SET description=excluded.description;

INSERT INTO permissions(code,description)
VALUES('device.rotate','Rotate the local terminal credential')
ON CONFLICT(code) DO UPDATE SET description=excluded.description;

INSERT INTO permissions(code,description)
VALUES('device.enroll','Issue terminal enrollment grants')
ON CONFLICT(code) DO UPDATE SET description=excluded.description;

INSERT OR IGNORE INTO role_permissions(role_id,permission_code)
SELECT id,'sync.resolve' FROM roles
WHERE lower(name) IN ('owner','administrator','admin');

INSERT OR IGNORE INTO role_permissions(role_id,permission_code)
SELECT id,'device.rotate' FROM roles
WHERE lower(name) IN ('owner','administrator','admin');

INSERT OR IGNORE INTO role_permissions(role_id,permission_code)
SELECT id,'device.enroll' FROM roles
WHERE lower(name) IN ('owner','administrator','admin');
