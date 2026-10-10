CREATE INDEX IF NOT EXISTS ix_manager_approval_scope
  ON manager_approval_consumptions(tenant_id, branch_id, device_id, action, consumed_at);

CREATE TRIGGER IF NOT EXISTS guard_manager_approval_evidence_insert
BEFORE INSERT ON manager_approval_consumptions
WHEN NEW.branch_id IS NULL
  OR NEW.issued_at IS NULL
  OR length(trim(NEW.issued_at))=0
  OR NOT EXISTS (
    SELECT 1 FROM branches b
    WHERE b.id=NEW.branch_id AND b.tenant_id=NEW.tenant_id
  )
  OR NOT EXISTS (
    SELECT 1 FROM devices d
    WHERE d.id=NEW.device_id
      AND d.tenant_id=NEW.tenant_id
      AND d.branch_id=NEW.branch_id
      AND d.status='ACTIVE'
  )
BEGIN SELECT RAISE(ABORT,'MANAGER_APPROVAL_SCOPE_VIOLATION'); END;
