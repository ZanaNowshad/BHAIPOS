CREATE TABLE IF NOT EXISTS store_operation_results (
  tenant_id TEXT NOT NULL REFERENCES tenants(id), operation_id TEXT NOT NULL, action TEXT NOT NULL,
  request_sha256 TEXT NOT NULL, result_json TEXT NOT NULL, committed_at TEXT NOT NULL,
  PRIMARY KEY(tenant_id,operation_id,action), UNIQUE(tenant_id,operation_id)
);

CREATE TABLE IF NOT EXISTS customer_credit_payment_allocations (
  payment_id TEXT NOT NULL REFERENCES customer_credit_payments(id),
  credit_ledger_id TEXT NOT NULL REFERENCES customer_credit_ledger(id),
  amount_fils INTEGER NOT NULL,
  PRIMARY KEY(payment_id,credit_ledger_id)
);

CREATE UNIQUE INDEX IF NOT EXISTS ux_loyalty_operation ON loyalty_ledger(tenant_id,operation_id) WHERE operation_id IS NOT NULL;
CREATE UNIQUE INDEX IF NOT EXISTS ux_customer_credit_operation ON customer_credit_ledger(tenant_id,operation_id) WHERE operation_id IS NOT NULL;

CREATE TRIGGER IF NOT EXISTS immutable_store_operation_results_update BEFORE UPDATE ON store_operation_results BEGIN SELECT RAISE(ABORT,'IMMUTABLE:store_operation_results'); END;
CREATE TRIGGER IF NOT EXISTS immutable_store_operation_results_delete BEFORE DELETE ON store_operation_results BEGIN SELECT RAISE(ABORT,'IMMUTABLE:store_operation_results'); END;
CREATE TRIGGER IF NOT EXISTS immutable_customer_credit_payments_update BEFORE UPDATE ON customer_credit_payments BEGIN SELECT RAISE(ABORT,'IMMUTABLE:customer_credit_payments'); END;
CREATE TRIGGER IF NOT EXISTS immutable_customer_credit_payments_delete BEFORE DELETE ON customer_credit_payments BEGIN SELECT RAISE(ABORT,'IMMUTABLE:customer_credit_payments'); END;
CREATE TRIGGER IF NOT EXISTS immutable_customer_credit_payment_allocations_update BEFORE UPDATE ON customer_credit_payment_allocations BEGIN SELECT RAISE(ABORT,'IMMUTABLE:customer_credit_payment_allocations'); END;
CREATE TRIGGER IF NOT EXISTS immutable_customer_credit_payment_allocations_delete BEFORE DELETE ON customer_credit_payment_allocations BEGIN SELECT RAISE(ABORT,'IMMUTABLE:customer_credit_payment_allocations'); END;

CREATE TRIGGER IF NOT EXISTS guard_loyalty_ledger_insert_v2 BEFORE INSERT ON loyalty_ledger
WHEN NOT EXISTS (SELECT 1 FROM customers c WHERE c.id=NEW.customer_id AND c.tenant_id=NEW.tenant_id)
 OR typeof(NEW.points_delta)!='integer' OR NEW.points_delta=0
BEGIN SELECT RAISE(ABORT,'INVALID_LOYALTY_EVENT'); END;

CREATE TRIGGER IF NOT EXISTS guard_customer_credit_ledger_insert_v2 BEFORE INSERT ON customer_credit_ledger
WHEN NOT EXISTS (SELECT 1 FROM customers c WHERE c.id=NEW.customer_id AND c.tenant_id=NEW.tenant_id)
 OR typeof(NEW.debit_fils)!='integer' OR typeof(NEW.credit_fils)!='integer'
 OR NEW.debit_fils<0 OR NEW.credit_fils<0 OR (NEW.debit_fils=0 AND NEW.credit_fils=0) OR (NEW.debit_fils>0 AND NEW.credit_fils>0)
BEGIN SELECT RAISE(ABORT,'INVALID_CUSTOMER_CREDIT_EVENT'); END;

CREATE TRIGGER IF NOT EXISTS guard_customer_credit_allocation_insert BEFORE INSERT ON customer_credit_payment_allocations
WHEN typeof(NEW.amount_fils)!='integer' OR NEW.amount_fils<=0
 OR (SELECT tenant_id FROM customer_credit_payments WHERE id=NEW.payment_id)!=(SELECT tenant_id FROM customer_credit_ledger WHERE id=NEW.credit_ledger_id)
 OR (SELECT customer_id FROM customer_credit_payments WHERE id=NEW.payment_id)!=(SELECT customer_id FROM customer_credit_ledger WHERE id=NEW.credit_ledger_id)
BEGIN SELECT RAISE(ABORT,'INVALID_CUSTOMER_CREDIT_ALLOCATION'); END;

INSERT INTO permissions(code,description) VALUES('customer.manage','Manage customers') ON CONFLICT(code) DO UPDATE SET description=excluded.description;
INSERT INTO permissions(code,description) VALUES('loyalty.adjust','Post loyalty ledger events') ON CONFLICT(code) DO UPDATE SET description=excluded.description;
INSERT INTO permissions(code,description) VALUES('customer.credit.manage','Manage customer credit limits') ON CONFLICT(code) DO UPDATE SET description=excluded.description;
INSERT INTO permissions(code,description) VALUES('customer.credit.collect','Collect customer credit payments') ON CONFLICT(code) DO UPDATE SET description=excluded.description;
INSERT INTO permissions(code,description) VALUES('customer.credit.view','View customer credit statements') ON CONFLICT(code) DO UPDATE SET description=excluded.description;

INSERT OR IGNORE INTO role_permissions(role_id,permission_code)
SELECT r.id,p.code FROM roles r JOIN permissions p ON p.code IN ('customer.manage','loyalty.adjust','customer.credit.manage','customer.credit.collect','customer.credit.view')
WHERE lower(r.name) IN ('owner','administrator','admin','manager','accountant');
