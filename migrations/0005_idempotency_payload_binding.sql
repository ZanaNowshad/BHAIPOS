PRAGMA foreign_keys = ON;

-- One durable identity per tenant operation. The request digest binds a retry
-- to the exact normalized command payload; action is included so an operation
-- ID cannot be reused for a different mutation family.
CREATE TABLE IF NOT EXISTS idempotency_operations (
  tenant_id TEXT NOT NULL REFERENCES tenants(id),
  operation_id TEXT NOT NULL,
  action TEXT NOT NULL,
  request_sha256 TEXT NOT NULL CHECK(length(request_sha256) = 64),
  created_at TEXT NOT NULL,
  PRIMARY KEY(tenant_id, operation_id)
);

CREATE TRIGGER IF NOT EXISTS guard_idempotency_result_insert
BEFORE INSERT ON idempotency_results
WHEN NOT EXISTS (
  SELECT 1
  FROM idempotency_operations io
  WHERE io.tenant_id = NEW.tenant_id
    AND io.operation_id = NEW.operation_id
    AND io.action = NEW.action
)
BEGIN
  SELECT RAISE(ABORT,'IDEMPOTENCY_BINDING_REQUIRED');
END;

CREATE TRIGGER IF NOT EXISTS immutable_idempotency_operations_update
BEFORE UPDATE ON idempotency_operations
BEGIN SELECT RAISE(ABORT,'IMMUTABLE:idempotency_operations'); END;

CREATE TRIGGER IF NOT EXISTS immutable_idempotency_operations_delete
BEFORE DELETE ON idempotency_operations
BEGIN SELECT RAISE(ABORT,'IMMUTABLE:idempotency_operations'); END;
