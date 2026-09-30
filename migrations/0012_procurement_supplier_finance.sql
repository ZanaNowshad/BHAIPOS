CREATE TABLE IF NOT EXISTS procurement_operation_results (
  tenant_id TEXT NOT NULL REFERENCES tenants(id),
  operation_id TEXT NOT NULL,
  action TEXT NOT NULL,
  request_sha256 TEXT NOT NULL,
  result_json TEXT NOT NULL,
  committed_at TEXT NOT NULL,
  PRIMARY KEY(tenant_id,operation_id,action),
  UNIQUE(tenant_id,operation_id)
);

CREATE TABLE IF NOT EXISTS purchase_order_events (
  id TEXT PRIMARY KEY,
  tenant_id TEXT NOT NULL REFERENCES tenants(id),
  purchase_order_id TEXT NOT NULL REFERENCES purchase_orders(id),
  event_type TEXT NOT NULL CHECK(event_type IN ('CREATED','APPROVED','ORDERED','RECEIVED','CANCELLED')),
  operation_id TEXT NOT NULL,
  device_id TEXT NOT NULL REFERENCES devices(id),
  user_id TEXT NOT NULL REFERENCES users(id),
  evidence_json TEXT NOT NULL,
  created_at TEXT NOT NULL,
  UNIQUE(tenant_id,operation_id)
);

CREATE TABLE IF NOT EXISTS supplier_invoice_events (
  id TEXT PRIMARY KEY,
  tenant_id TEXT NOT NULL REFERENCES tenants(id),
  supplier_invoice_id TEXT NOT NULL REFERENCES supplier_invoices(id),
  event_type TEXT NOT NULL CHECK(event_type IN ('POSTED','PAYMENT_ALLOCATED','CREDITED','VOIDED')),
  operation_id TEXT NOT NULL,
  device_id TEXT NOT NULL REFERENCES devices(id),
  user_id TEXT NOT NULL REFERENCES users(id),
  evidence_json TEXT NOT NULL,
  created_at TEXT NOT NULL,
  UNIQUE(tenant_id,supplier_invoice_id,operation_id,event_type)
);

CREATE TABLE IF NOT EXISTS supplier_return_events (
  id TEXT PRIMARY KEY,
  tenant_id TEXT NOT NULL REFERENCES tenants(id),
  supplier_return_id TEXT NOT NULL REFERENCES supplier_returns(id),
  event_type TEXT NOT NULL CHECK(event_type IN ('CREATED','DISPATCHED','ACKNOWLEDGED','CREDITED','REPLACED','CLOSED','CANCELLED')),
  operation_id TEXT NOT NULL,
  device_id TEXT NOT NULL REFERENCES devices(id),
  user_id TEXT NOT NULL REFERENCES users(id),
  evidence_json TEXT NOT NULL,
  created_at TEXT NOT NULL,
  UNIQUE(tenant_id,operation_id,event_type)
);

CREATE INDEX IF NOT EXISTS ix_supplier_invoices_open
  ON supplier_invoices(tenant_id,supplier_id,status,due_date,invoice_date);
CREATE INDEX IF NOT EXISTS ix_purchase_orders_status
  ON purchase_orders(tenant_id,branch_id,status,created_at);
CREATE INDEX IF NOT EXISTS ix_goods_receipts_po
  ON goods_receipts(tenant_id,po_id,received_at);

CREATE TRIGGER IF NOT EXISTS immutable_procurement_operation_results_update BEFORE UPDATE ON procurement_operation_results BEGIN SELECT RAISE(ABORT,'IMMUTABLE:procurement_operation_results'); END;
CREATE TRIGGER IF NOT EXISTS immutable_procurement_operation_results_delete BEFORE DELETE ON procurement_operation_results BEGIN SELECT RAISE(ABORT,'IMMUTABLE:procurement_operation_results'); END;
CREATE TRIGGER IF NOT EXISTS immutable_purchase_order_events_update BEFORE UPDATE ON purchase_order_events BEGIN SELECT RAISE(ABORT,'IMMUTABLE:purchase_order_events'); END;
CREATE TRIGGER IF NOT EXISTS immutable_purchase_order_events_delete BEFORE DELETE ON purchase_order_events BEGIN SELECT RAISE(ABORT,'IMMUTABLE:purchase_order_events'); END;
CREATE TRIGGER IF NOT EXISTS immutable_supplier_invoice_events_update BEFORE UPDATE ON supplier_invoice_events BEGIN SELECT RAISE(ABORT,'IMMUTABLE:supplier_invoice_events'); END;
CREATE TRIGGER IF NOT EXISTS immutable_supplier_invoice_events_delete BEFORE DELETE ON supplier_invoice_events BEGIN SELECT RAISE(ABORT,'IMMUTABLE:supplier_invoice_events'); END;
CREATE TRIGGER IF NOT EXISTS immutable_supplier_return_events_update BEFORE UPDATE ON supplier_return_events BEGIN SELECT RAISE(ABORT,'IMMUTABLE:supplier_return_events'); END;
CREATE TRIGGER IF NOT EXISTS immutable_supplier_return_events_delete BEFORE DELETE ON supplier_return_events BEGIN SELECT RAISE(ABORT,'IMMUTABLE:supplier_return_events'); END;
CREATE TRIGGER IF NOT EXISTS immutable_supplier_payments_update BEFORE UPDATE ON supplier_payments BEGIN SELECT RAISE(ABORT,'IMMUTABLE:supplier_payments'); END;
CREATE TRIGGER IF NOT EXISTS immutable_supplier_payments_delete BEFORE DELETE ON supplier_payments BEGIN SELECT RAISE(ABORT,'IMMUTABLE:supplier_payments'); END;
CREATE TRIGGER IF NOT EXISTS immutable_supplier_payment_allocations_update BEFORE UPDATE ON supplier_payment_allocations BEGIN SELECT RAISE(ABORT,'IMMUTABLE:supplier_payment_allocations'); END;
CREATE TRIGGER IF NOT EXISTS immutable_supplier_payment_allocations_delete BEFORE DELETE ON supplier_payment_allocations BEGIN SELECT RAISE(ABORT,'IMMUTABLE:supplier_payment_allocations'); END;
CREATE TRIGGER IF NOT EXISTS immutable_supplier_invoice_lines_update BEFORE UPDATE ON supplier_invoice_lines BEGIN SELECT RAISE(ABORT,'IMMUTABLE:supplier_invoice_lines'); END;
CREATE TRIGGER IF NOT EXISTS immutable_supplier_invoice_lines_delete BEFORE DELETE ON supplier_invoice_lines BEGIN SELECT RAISE(ABORT,'IMMUTABLE:supplier_invoice_lines'); END;
CREATE TRIGGER IF NOT EXISTS immutable_supplier_invoice_financial_fields BEFORE UPDATE OF tenant_id,branch_id,supplier_id,invoice_number,invoice_date,due_date,po_id,goods_receipt_id,subtotal_fils,tax_fils,total_fils,created_at ON supplier_invoices BEGIN SELECT RAISE(ABORT,'IMMUTABLE:supplier_invoice_financial_fields'); END;
CREATE TRIGGER IF NOT EXISTS immutable_purchase_order_financial_fields BEFORE UPDATE OF tenant_id,branch_id,supplier_id,po_number,subtotal_fils,tax_fils,total_fils,created_by_user_id,created_at ON purchase_orders BEGIN SELECT RAISE(ABORT,'IMMUTABLE:purchase_order_financial_fields'); END;
CREATE TRIGGER IF NOT EXISTS immutable_purchase_order_line_terms BEFORE UPDATE OF po_id,product_id,ordered_qty_milli,unit_cost_fils,tax_rate_bps,line_total_fils ON purchase_order_lines BEGIN SELECT RAISE(ABORT,'IMMUTABLE:purchase_order_line_terms'); END;
CREATE TRIGGER IF NOT EXISTS immutable_purchase_order_lines_delete BEFORE DELETE ON purchase_order_lines BEGIN SELECT RAISE(ABORT,'IMMUTABLE:purchase_order_lines'); END;

CREATE TRIGGER IF NOT EXISTS guard_purchase_order_events_insert BEFORE INSERT ON purchase_order_events
WHEN NOT EXISTS (SELECT 1 FROM purchase_orders p WHERE p.id=NEW.purchase_order_id AND p.tenant_id=NEW.tenant_id)
 OR NOT EXISTS (SELECT 1 FROM devices d JOIN purchase_orders p ON p.id=NEW.purchase_order_id WHERE d.id=NEW.device_id AND d.tenant_id=NEW.tenant_id AND d.branch_id=p.branch_id)
 OR NOT EXISTS (SELECT 1 FROM users u WHERE u.id=NEW.user_id AND u.tenant_id=NEW.tenant_id)
BEGIN SELECT RAISE(ABORT,'TENANT_SCOPE_VIOLATION:purchase_order_events'); END;

CREATE TRIGGER IF NOT EXISTS guard_supplier_invoice_insert BEFORE INSERT ON supplier_invoices
WHEN NOT EXISTS (SELECT 1 FROM branches b WHERE b.id=NEW.branch_id AND b.tenant_id=NEW.tenant_id)
 OR NOT EXISTS (SELECT 1 FROM suppliers s WHERE s.id=NEW.supplier_id AND s.tenant_id=NEW.tenant_id)
 OR (NEW.po_id IS NOT NULL AND NOT EXISTS (SELECT 1 FROM purchase_orders p WHERE p.id=NEW.po_id AND p.tenant_id=NEW.tenant_id AND p.supplier_id=NEW.supplier_id))
 OR (NEW.goods_receipt_id IS NOT NULL AND NOT EXISTS (SELECT 1 FROM goods_receipts g WHERE g.id=NEW.goods_receipt_id AND g.tenant_id=NEW.tenant_id AND g.supplier_id=NEW.supplier_id))
BEGIN SELECT RAISE(ABORT,'TENANT_SCOPE_VIOLATION:supplier_invoices'); END;

CREATE TRIGGER IF NOT EXISTS guard_supplier_return_events_insert BEFORE INSERT ON supplier_return_events
WHEN NOT EXISTS (SELECT 1 FROM supplier_returns r WHERE r.id=NEW.supplier_return_id AND r.tenant_id=NEW.tenant_id)
 OR NOT EXISTS (SELECT 1 FROM users u WHERE u.id=NEW.user_id AND u.tenant_id=NEW.tenant_id)
 OR NOT EXISTS (SELECT 1 FROM devices d JOIN supplier_returns r ON r.id=NEW.supplier_return_id WHERE d.id=NEW.device_id AND d.tenant_id=NEW.tenant_id AND d.branch_id=r.branch_id)
BEGIN SELECT RAISE(ABORT,'TENANT_SCOPE_VIOLATION:supplier_return_events'); END;

CREATE TRIGGER IF NOT EXISTS guard_supplier_invoice_values_insert BEFORE INSERT ON supplier_invoices
WHEN typeof(NEW.subtotal_fils)!='integer' OR typeof(NEW.tax_fils)!='integer' OR typeof(NEW.total_fils)!='integer'
 OR NEW.subtotal_fils<0 OR NEW.tax_fils<0 OR NEW.total_fils!=NEW.subtotal_fils+NEW.tax_fils OR NEW.amount_paid_fils!=0
BEGIN SELECT RAISE(ABORT,'INVALID_SUPPLIER_INVOICE_TOTAL'); END;

CREATE TRIGGER IF NOT EXISTS guard_supplier_payment_allocation_insert BEFORE INSERT ON supplier_payment_allocations
WHEN typeof(NEW.amount_fils)!='integer' OR NEW.amount_fils<=0
 OR (SELECT tenant_id FROM supplier_payments WHERE id=NEW.payment_id)!=(SELECT tenant_id FROM supplier_invoices WHERE id=NEW.invoice_id)
 OR (SELECT supplier_id FROM supplier_payments WHERE id=NEW.payment_id)!=(SELECT supplier_id FROM supplier_invoices WHERE id=NEW.invoice_id)
BEGIN SELECT RAISE(ABORT,'INVALID_SUPPLIER_PAYMENT_ALLOCATION'); END;

CREATE TRIGGER IF NOT EXISTS guard_supplier_ledger_insert BEFORE INSERT ON supplier_ledger
WHEN NOT EXISTS (SELECT 1 FROM suppliers s WHERE s.id=NEW.supplier_id AND s.tenant_id=NEW.tenant_id)
 OR (NEW.branch_id IS NOT NULL AND NOT EXISTS (SELECT 1 FROM branches b WHERE b.id=NEW.branch_id AND b.tenant_id=NEW.tenant_id))
 OR typeof(NEW.debit_fils)!='integer' OR typeof(NEW.credit_fils)!='integer'
 OR NEW.debit_fils<0 OR NEW.credit_fils<0 OR (NEW.debit_fils=0 AND NEW.credit_fils=0) OR (NEW.debit_fils>0 AND NEW.credit_fils>0)
BEGIN SELECT RAISE(ABORT,'INVALID_SUPPLIER_LEDGER_EVENT'); END;

INSERT INTO permissions(code,description) VALUES('procurement.manage','Create and progress purchase orders') ON CONFLICT(code) DO UPDATE SET description=excluded.description;
INSERT INTO permissions(code,description) VALUES('procurement.approve','Approve purchase obligations') ON CONFLICT(code) DO UPDATE SET description=excluded.description;
INSERT INTO permissions(code,description) VALUES('supplier.invoice.post','Post supplier invoices') ON CONFLICT(code) DO UPDATE SET description=excluded.description;
INSERT INTO permissions(code,description) VALUES('supplier.payment.post','Post and allocate supplier payments') ON CONFLICT(code) DO UPDATE SET description=excluded.description;
INSERT INTO permissions(code,description) VALUES('supplier.statement.view','View supplier statements') ON CONFLICT(code) DO UPDATE SET description=excluded.description;
INSERT INTO permissions(code,description) VALUES('supplier.return','Dispatch and settle supplier returns') ON CONFLICT(code) DO UPDATE SET description=excluded.description;

INSERT OR IGNORE INTO role_permissions(role_id,permission_code)
SELECT r.id,p.code FROM roles r JOIN permissions p ON p.code IN ('procurement.manage','procurement.approve','supplier.invoice.post','supplier.payment.post','supplier.statement.view','supplier.return')
WHERE lower(r.name) IN ('owner','administrator','admin','accountant','inventory staff');
