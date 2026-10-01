CREATE TABLE IF NOT EXISTS expense_operation_results (
  tenant_id TEXT NOT NULL REFERENCES tenants(id),
  operation_id TEXT NOT NULL,
  action TEXT NOT NULL,
  request_sha256 TEXT NOT NULL,
  result_json TEXT NOT NULL,
  committed_at TEXT NOT NULL,
  PRIMARY KEY(tenant_id,operation_id,action),
  UNIQUE(tenant_id,operation_id)
);

CREATE TABLE IF NOT EXISTS expense_events (
  id TEXT PRIMARY KEY,
  tenant_id TEXT NOT NULL REFERENCES tenants(id),
  expense_id TEXT NOT NULL REFERENCES expenses(id),
  event_type TEXT NOT NULL CHECK(event_type IN ('CREATED','SUBMITTED','APPROVED','REJECTED','PAID')),
  operation_id TEXT NOT NULL,
  device_id TEXT NOT NULL REFERENCES devices(id),
  user_id TEXT NOT NULL REFERENCES users(id),
  evidence_json TEXT NOT NULL,
  created_at TEXT NOT NULL,
  UNIQUE(tenant_id,operation_id,event_type)
);

CREATE TABLE IF NOT EXISTS expense_payments (
  id TEXT PRIMARY KEY,
  tenant_id TEXT NOT NULL REFERENCES tenants(id),
  branch_id TEXT NOT NULL REFERENCES branches(id),
  expense_id TEXT NOT NULL REFERENCES expenses(id),
  operation_id TEXT NOT NULL,
  device_id TEXT NOT NULL REFERENCES devices(id),
  user_id TEXT NOT NULL REFERENCES users(id),
  method TEXT NOT NULL CHECK(method IN ('CASH','CARD','BANK_TRANSFER','BENEFIT_PAY','CHEQUE','OTHER')),
  amount_fils INTEGER NOT NULL,
  reference TEXT,
  paid_at TEXT NOT NULL,
  UNIQUE(tenant_id,operation_id),
  UNIQUE(expense_id)
);

CREATE INDEX IF NOT EXISTS ix_expense_events ON expense_events(tenant_id,expense_id,created_at,id);
CREATE INDEX IF NOT EXISTS ix_expense_paid_report ON expenses(tenant_id,branch_id,status,paid_at);

CREATE TRIGGER IF NOT EXISTS immutable_expense_operation_results_update BEFORE UPDATE ON expense_operation_results BEGIN SELECT RAISE(ABORT,'IMMUTABLE:expense_operation_results'); END;
CREATE TRIGGER IF NOT EXISTS immutable_expense_operation_results_delete BEFORE DELETE ON expense_operation_results BEGIN SELECT RAISE(ABORT,'IMMUTABLE:expense_operation_results'); END;
CREATE TRIGGER IF NOT EXISTS immutable_expense_events_update BEFORE UPDATE ON expense_events BEGIN SELECT RAISE(ABORT,'IMMUTABLE:expense_events'); END;
CREATE TRIGGER IF NOT EXISTS immutable_expense_events_delete BEFORE DELETE ON expense_events BEGIN SELECT RAISE(ABORT,'IMMUTABLE:expense_events'); END;
CREATE TRIGGER IF NOT EXISTS immutable_expense_payments_update BEFORE UPDATE ON expense_payments BEGIN SELECT RAISE(ABORT,'IMMUTABLE:expense_payments'); END;
CREATE TRIGGER IF NOT EXISTS immutable_expense_payments_delete BEFORE DELETE ON expense_payments BEGIN SELECT RAISE(ABORT,'IMMUTABLE:expense_payments'); END;

CREATE TRIGGER IF NOT EXISTS guard_expenses_insert BEFORE INSERT ON expenses
WHEN typeof(NEW.amount_fils)!='integer' OR typeof(NEW.tax_fils)!='integer'
 OR NEW.amount_fils<=0 OR NEW.tax_fils<0 OR NEW.tax_fils>NEW.amount_fils
 OR NOT EXISTS (SELECT 1 FROM expense_categories c WHERE c.id=NEW.category_id AND c.tenant_id=NEW.tenant_id AND c.active=1)
 OR (NEW.branch_id IS NOT NULL AND NOT EXISTS (SELECT 1 FROM branches b WHERE b.id=NEW.branch_id AND b.tenant_id=NEW.tenant_id))
 OR NOT EXISTS (SELECT 1 FROM users u WHERE u.id=NEW.created_by_user_id AND u.tenant_id=NEW.tenant_id)
BEGIN SELECT RAISE(ABORT,'INVALID_EXPENSE'); END;

CREATE TRIGGER IF NOT EXISTS guard_expense_financial_update BEFORE UPDATE OF tenant_id,branch_id,category_id,description,amount_fils,tax_fils,supplier_id,incurred_on,created_by_user_id,created_at ON expenses
WHEN OLD.status!='DRAFT'
BEGIN SELECT RAISE(ABORT,'IMMUTABLE:expense_financial_evidence'); END;

CREATE TRIGGER IF NOT EXISTS guard_expense_status_transition BEFORE UPDATE OF status ON expenses
WHEN NOT (
  (OLD.status='DRAFT' AND NEW.status='SUBMITTED') OR
  (OLD.status='SUBMITTED' AND NEW.status IN ('APPROVED','REJECTED')) OR
  (OLD.status='APPROVED' AND NEW.status='PAID')
) OR NOT EXISTS (
  SELECT 1 FROM expense_events e
  WHERE e.expense_id=OLD.id AND e.tenant_id=OLD.tenant_id AND e.event_type=NEW.status
)
BEGIN SELECT RAISE(ABORT,'INVALID_EXPENSE_TRANSITION'); END;

CREATE TRIGGER IF NOT EXISTS guard_expense_events_insert BEFORE INSERT ON expense_events
WHEN NOT EXISTS (SELECT 1 FROM expenses e WHERE e.id=NEW.expense_id AND e.tenant_id=NEW.tenant_id)
 OR NOT EXISTS (SELECT 1 FROM users u WHERE u.id=NEW.user_id AND u.tenant_id=NEW.tenant_id)
 OR NOT EXISTS (
   SELECT 1 FROM devices d JOIN expenses e ON e.id=NEW.expense_id
   WHERE d.id=NEW.device_id AND d.tenant_id=NEW.tenant_id AND d.branch_id=e.branch_id
 )
BEGIN SELECT RAISE(ABORT,'TENANT_SCOPE_VIOLATION:expense_events'); END;

CREATE TRIGGER IF NOT EXISTS guard_expense_event_state BEFORE INSERT ON expense_events
WHEN NOT (
  (NEW.event_type='CREATED'
    AND EXISTS (SELECT 1 FROM expenses e WHERE e.id=NEW.expense_id AND e.tenant_id=NEW.tenant_id AND e.status='DRAFT')
    AND NOT EXISTS (SELECT 1 FROM expense_events prior WHERE prior.expense_id=NEW.expense_id))
  OR (NEW.event_type='SUBMITTED'
    AND EXISTS (SELECT 1 FROM expenses e WHERE e.id=NEW.expense_id AND e.tenant_id=NEW.tenant_id AND e.status='DRAFT'))
  OR (NEW.event_type IN ('APPROVED','REJECTED')
    AND EXISTS (SELECT 1 FROM expenses e WHERE e.id=NEW.expense_id AND e.tenant_id=NEW.tenant_id AND e.status='SUBMITTED'))
  OR (NEW.event_type='PAID'
    AND EXISTS (SELECT 1 FROM expenses e WHERE e.id=NEW.expense_id AND e.tenant_id=NEW.tenant_id AND e.status='APPROVED')
    AND EXISTS (SELECT 1 FROM expense_payments p WHERE p.expense_id=NEW.expense_id AND p.tenant_id=NEW.tenant_id AND p.operation_id=NEW.operation_id))
)
BEGIN SELECT RAISE(ABORT,'INVALID_EXPENSE_EVENT_STATE'); END;

CREATE TRIGGER IF NOT EXISTS guard_expense_payments_insert BEFORE INSERT ON expense_payments
WHEN typeof(NEW.amount_fils)!='integer' OR NEW.amount_fils<=0
 OR NOT EXISTS (
   SELECT 1 FROM expenses e
   WHERE e.id=NEW.expense_id AND e.tenant_id=NEW.tenant_id AND e.branch_id=NEW.branch_id
     AND e.status='APPROVED' AND e.amount_fils=NEW.amount_fils
 )
 OR NOT EXISTS (SELECT 1 FROM users u WHERE u.id=NEW.user_id AND u.tenant_id=NEW.tenant_id)
 OR NOT EXISTS (SELECT 1 FROM devices d WHERE d.id=NEW.device_id AND d.tenant_id=NEW.tenant_id AND d.branch_id=NEW.branch_id)
BEGIN SELECT RAISE(ABORT,'INVALID_EXPENSE_PAYMENT'); END;

INSERT INTO permissions(code,description) VALUES('expense.manage','Create and submit expenses') ON CONFLICT(code) DO UPDATE SET description=excluded.description;
INSERT INTO permissions(code,description) VALUES('expense.approve','Approve or reject submitted expenses') ON CONFLICT(code) DO UPDATE SET description=excluded.description;
INSERT INTO permissions(code,description) VALUES('expense.pay','Pay approved expenses') ON CONFLICT(code) DO UPDATE SET description=excluded.description;
INSERT INTO permissions(code,description) VALUES('expense.report','View expense and operating profit reports') ON CONFLICT(code) DO UPDATE SET description=excluded.description;

INSERT OR IGNORE INTO role_permissions(role_id,permission_code)
SELECT r.id,p.code FROM roles r JOIN permissions p ON p.code IN ('expense.manage','expense.approve','expense.pay','expense.report')
WHERE lower(r.name) IN ('owner','administrator','admin','manager','accountant');
