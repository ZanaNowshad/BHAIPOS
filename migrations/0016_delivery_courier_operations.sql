CREATE TABLE IF NOT EXISTS delivery_operation_results (
  tenant_id TEXT NOT NULL REFERENCES tenants(id),
  operation_id TEXT NOT NULL,
  action TEXT NOT NULL,
  request_sha256 TEXT NOT NULL,
  result_json TEXT NOT NULL,
  committed_at TEXT NOT NULL,
  PRIMARY KEY(tenant_id,operation_id,action),
  UNIQUE(tenant_id,operation_id)
);

CREATE TABLE IF NOT EXISTS delivery_state_events (
  id TEXT PRIMARY KEY,
  tenant_id TEXT NOT NULL REFERENCES tenants(id),
  delivery_id TEXT NOT NULL REFERENCES delivery_orders(id),
  event_type TEXT NOT NULL CHECK(event_type IN ('CREATED','PREPARING','READY','DISPATCHED','DELIVERED','CANCELLED','RETURNED')),
  operation_id TEXT NOT NULL,
  device_id TEXT NOT NULL REFERENCES devices(id),
  user_id TEXT NOT NULL REFERENCES users(id),
  worker_id TEXT REFERENCES delivery_workers(id),
  evidence_json TEXT NOT NULL,
  created_at TEXT NOT NULL,
  UNIQUE(tenant_id,operation_id)
);

CREATE TABLE IF NOT EXISTS delivery_collections (
  id TEXT PRIMARY KEY,
  tenant_id TEXT NOT NULL REFERENCES tenants(id),
  branch_id TEXT NOT NULL REFERENCES branches(id),
  delivery_id TEXT NOT NULL REFERENCES delivery_orders(id),
  worker_id TEXT NOT NULL REFERENCES delivery_workers(id),
  operation_id TEXT NOT NULL,
  device_id TEXT NOT NULL REFERENCES devices(id),
  user_id TEXT NOT NULL REFERENCES users(id),
  method TEXT NOT NULL CHECK(method IN ('CASH','CARD','BANK_TRANSFER','BENEFIT_PAY','OTHER')),
  amount_fils INTEGER NOT NULL,
  reference TEXT,
  collected_at TEXT NOT NULL,
  UNIQUE(tenant_id,operation_id),
  UNIQUE(delivery_id)
);

CREATE TABLE IF NOT EXISTS delivery_cash_settlements (
  id TEXT PRIMARY KEY,
  tenant_id TEXT NOT NULL REFERENCES tenants(id),
  branch_id TEXT NOT NULL REFERENCES branches(id),
  worker_id TEXT NOT NULL REFERENCES delivery_workers(id),
  operation_id TEXT NOT NULL,
  device_id TEXT NOT NULL REFERENCES devices(id),
  user_id TEXT NOT NULL REFERENCES users(id),
  expected_cash_fils INTEGER NOT NULL,
  returned_cash_fils INTEGER NOT NULL,
  variance_fils INTEGER NOT NULL,
  status TEXT NOT NULL CHECK(status IN ('SETTLED','DISCREPANCY')),
  note TEXT,
  settled_at TEXT NOT NULL,
  UNIQUE(tenant_id,operation_id)
);

CREATE TABLE IF NOT EXISTS delivery_cash_settlement_allocations (
  settlement_id TEXT NOT NULL REFERENCES delivery_cash_settlements(id),
  collection_id TEXT NOT NULL REFERENCES delivery_collections(id),
  tenant_id TEXT NOT NULL REFERENCES tenants(id),
  amount_fils INTEGER NOT NULL,
  PRIMARY KEY(settlement_id,collection_id),
  UNIQUE(collection_id)
);

CREATE INDEX IF NOT EXISTS ix_delivery_state_events ON delivery_state_events(tenant_id,delivery_id,created_at,id);
CREATE INDEX IF NOT EXISTS ix_delivery_open_cash ON delivery_collections(tenant_id,branch_id,worker_id,method,collected_at,id);
CREATE INDEX IF NOT EXISTS ix_delivery_settlements ON delivery_cash_settlements(tenant_id,branch_id,worker_id,settled_at,id);

CREATE TRIGGER IF NOT EXISTS immutable_delivery_operation_results_update BEFORE UPDATE ON delivery_operation_results BEGIN SELECT RAISE(ABORT,'IMMUTABLE:delivery_operation_results'); END;
CREATE TRIGGER IF NOT EXISTS immutable_delivery_operation_results_delete BEFORE DELETE ON delivery_operation_results BEGIN SELECT RAISE(ABORT,'IMMUTABLE:delivery_operation_results'); END;
CREATE TRIGGER IF NOT EXISTS immutable_delivery_state_events_update BEFORE UPDATE ON delivery_state_events BEGIN SELECT RAISE(ABORT,'IMMUTABLE:delivery_state_events'); END;
CREATE TRIGGER IF NOT EXISTS immutable_delivery_state_events_delete BEFORE DELETE ON delivery_state_events BEGIN SELECT RAISE(ABORT,'IMMUTABLE:delivery_state_events'); END;
CREATE TRIGGER IF NOT EXISTS immutable_delivery_collections_update BEFORE UPDATE ON delivery_collections BEGIN SELECT RAISE(ABORT,'IMMUTABLE:delivery_collections'); END;
CREATE TRIGGER IF NOT EXISTS immutable_delivery_collections_delete BEFORE DELETE ON delivery_collections BEGIN SELECT RAISE(ABORT,'IMMUTABLE:delivery_collections'); END;
CREATE TRIGGER IF NOT EXISTS immutable_delivery_cash_settlements_update BEFORE UPDATE ON delivery_cash_settlements BEGIN SELECT RAISE(ABORT,'IMMUTABLE:delivery_cash_settlements'); END;
CREATE TRIGGER IF NOT EXISTS immutable_delivery_cash_settlements_delete BEFORE DELETE ON delivery_cash_settlements BEGIN SELECT RAISE(ABORT,'IMMUTABLE:delivery_cash_settlements'); END;
CREATE TRIGGER IF NOT EXISTS immutable_delivery_cash_allocations_update BEFORE UPDATE ON delivery_cash_settlement_allocations BEGIN SELECT RAISE(ABORT,'IMMUTABLE:delivery_cash_settlement_allocations'); END;
CREATE TRIGGER IF NOT EXISTS immutable_delivery_cash_allocations_delete BEFORE DELETE ON delivery_cash_settlement_allocations BEGIN SELECT RAISE(ABORT,'IMMUTABLE:delivery_cash_settlement_allocations'); END;

CREATE TRIGGER IF NOT EXISTS guard_delivery_workers_insert BEFORE INSERT ON delivery_workers
WHEN NEW.branch_id IS NULL
 OR NOT EXISTS (SELECT 1 FROM branches b WHERE b.id=NEW.branch_id AND b.tenant_id=NEW.tenant_id)
BEGIN SELECT RAISE(ABORT,'TENANT_SCOPE_VIOLATION:delivery_workers'); END;

CREATE TRIGGER IF NOT EXISTS guard_delivery_orders_insert BEFORE INSERT ON delivery_orders
WHEN typeof(NEW.amount_due_fils)!='integer' OR NEW.amount_due_fils<=0
 OR NEW.status!='PENDING' OR NEW.payment_state!='DUE' OR NEW.assigned_worker_id IS NOT NULL
 OR NOT EXISTS (SELECT 1 FROM branches b WHERE b.id=NEW.branch_id AND b.tenant_id=NEW.tenant_id)
 OR (NEW.sale_id IS NOT NULL AND NOT EXISTS (SELECT 1 FROM sales s WHERE s.id=NEW.sale_id AND s.tenant_id=NEW.tenant_id AND s.branch_id=NEW.branch_id))
 OR (NEW.customer_id IS NOT NULL AND NOT EXISTS (SELECT 1 FROM customers c WHERE c.id=NEW.customer_id AND c.tenant_id=NEW.tenant_id))
BEGIN SELECT RAISE(ABORT,'INVALID_DELIVERY_ORDER'); END;

CREATE TRIGGER IF NOT EXISTS guard_delivery_financial_evidence_update BEFORE UPDATE OF tenant_id,branch_id,sale_id,customer_id,address_id,phone_e164,amount_due_fils,notes,created_at ON delivery_orders
BEGIN SELECT RAISE(ABORT,'IMMUTABLE:delivery_financial_evidence'); END;

CREATE TRIGGER IF NOT EXISTS guard_delivery_status_transition BEFORE UPDATE OF status ON delivery_orders
WHEN NOT (
  (OLD.status='PENDING' AND NEW.status IN ('PREPARING','CANCELLED')) OR
  (OLD.status='PREPARING' AND NEW.status IN ('READY','CANCELLED')) OR
  (OLD.status='READY' AND NEW.status IN ('DISPATCHED','CANCELLED')) OR
  (OLD.status='DISPATCHED' AND NEW.status IN ('DELIVERED','RETURNED'))
) OR NOT EXISTS (
  SELECT 1 FROM delivery_state_events e
  WHERE e.delivery_id=OLD.id AND e.tenant_id=OLD.tenant_id AND e.event_type=NEW.status
)
BEGIN SELECT RAISE(ABORT,'INVALID_DELIVERY_TRANSITION'); END;

CREATE TRIGGER IF NOT EXISTS guard_delivery_assignment_update BEFORE UPDATE OF assigned_worker_id ON delivery_orders
WHEN NEW.assigned_worker_id IS NULL
 OR OLD.status!='READY'
 OR NOT EXISTS (SELECT 1 FROM delivery_workers w WHERE w.id=NEW.assigned_worker_id AND w.tenant_id=NEW.tenant_id AND w.branch_id=NEW.branch_id AND w.active=1)
 OR NOT EXISTS (SELECT 1 FROM delivery_state_events e WHERE e.delivery_id=OLD.id AND e.tenant_id=OLD.tenant_id AND e.event_type='DISPATCHED' AND e.worker_id=NEW.assigned_worker_id)
BEGIN SELECT RAISE(ABORT,'INVALID_DELIVERY_ASSIGNMENT'); END;

CREATE TRIGGER IF NOT EXISTS guard_delivery_payment_state_update BEFORE UPDATE OF payment_state ON delivery_orders
WHEN NOT (OLD.payment_state='DUE' AND NEW.payment_state='PAID')
 OR NOT EXISTS (SELECT 1 FROM delivery_collections c WHERE c.delivery_id=OLD.id AND c.tenant_id=OLD.tenant_id AND c.amount_fils=OLD.amount_due_fils)
BEGIN SELECT RAISE(ABORT,'INVALID_DELIVERY_PAYMENT_TRANSITION'); END;

CREATE TRIGGER IF NOT EXISTS guard_delivery_state_event_insert BEFORE INSERT ON delivery_state_events
WHEN NOT EXISTS (SELECT 1 FROM users u WHERE u.id=NEW.user_id AND u.tenant_id=NEW.tenant_id)
 OR NOT EXISTS (
   SELECT 1 FROM devices d JOIN delivery_orders o ON o.id=NEW.delivery_id
   WHERE d.id=NEW.device_id AND d.tenant_id=NEW.tenant_id AND d.branch_id=o.branch_id AND o.tenant_id=NEW.tenant_id
 )
 OR (NEW.worker_id IS NOT NULL AND NOT EXISTS (SELECT 1 FROM delivery_workers w JOIN delivery_orders o ON o.id=NEW.delivery_id WHERE w.id=NEW.worker_id AND w.tenant_id=NEW.tenant_id AND w.branch_id=o.branch_id AND w.active=1))
 OR NOT (
   (NEW.event_type='CREATED' AND EXISTS (SELECT 1 FROM delivery_orders o WHERE o.id=NEW.delivery_id AND o.tenant_id=NEW.tenant_id AND o.status='PENDING') AND NOT EXISTS (SELECT 1 FROM delivery_state_events prior WHERE prior.delivery_id=NEW.delivery_id)) OR
   (NEW.event_type='PREPARING' AND EXISTS (SELECT 1 FROM delivery_orders o WHERE o.id=NEW.delivery_id AND o.tenant_id=NEW.tenant_id AND o.status='PENDING')) OR
   (NEW.event_type='READY' AND EXISTS (SELECT 1 FROM delivery_orders o WHERE o.id=NEW.delivery_id AND o.tenant_id=NEW.tenant_id AND o.status='PREPARING')) OR
   (NEW.event_type='DISPATCHED' AND NEW.worker_id IS NOT NULL AND EXISTS (SELECT 1 FROM delivery_orders o WHERE o.id=NEW.delivery_id AND o.tenant_id=NEW.tenant_id AND o.status='READY')) OR
   (NEW.event_type='DELIVERED' AND EXISTS (SELECT 1 FROM delivery_orders o WHERE o.id=NEW.delivery_id AND o.tenant_id=NEW.tenant_id AND o.status='DISPATCHED')) OR
   (NEW.event_type='CANCELLED' AND EXISTS (SELECT 1 FROM delivery_orders o WHERE o.id=NEW.delivery_id AND o.tenant_id=NEW.tenant_id AND o.status IN ('PENDING','PREPARING','READY'))) OR
   (NEW.event_type='RETURNED' AND EXISTS (SELECT 1 FROM delivery_orders o WHERE o.id=NEW.delivery_id AND o.tenant_id=NEW.tenant_id AND o.status='DISPATCHED'))
 )
BEGIN SELECT RAISE(ABORT,'INVALID_DELIVERY_EVENT_STATE'); END;

CREATE TRIGGER IF NOT EXISTS guard_delivery_collection_insert BEFORE INSERT ON delivery_collections
WHEN typeof(NEW.amount_fils)!='integer' OR NEW.amount_fils<=0
 OR NOT EXISTS (
   SELECT 1 FROM delivery_orders o
   WHERE o.id=NEW.delivery_id AND o.tenant_id=NEW.tenant_id AND o.branch_id=NEW.branch_id
     AND o.status='DISPATCHED' AND o.payment_state='DUE' AND o.amount_due_fils=NEW.amount_fils
     AND o.assigned_worker_id=NEW.worker_id
 )
 OR NOT EXISTS (SELECT 1 FROM delivery_workers w WHERE w.id=NEW.worker_id AND w.tenant_id=NEW.tenant_id AND w.branch_id=NEW.branch_id AND w.active=1)
 OR NOT EXISTS (SELECT 1 FROM devices d WHERE d.id=NEW.device_id AND d.tenant_id=NEW.tenant_id AND d.branch_id=NEW.branch_id)
 OR NOT EXISTS (SELECT 1 FROM users u WHERE u.id=NEW.user_id AND u.tenant_id=NEW.tenant_id)
BEGIN SELECT RAISE(ABORT,'INVALID_DELIVERY_COLLECTION'); END;

CREATE TRIGGER IF NOT EXISTS guard_delivery_cash_settlement_insert BEFORE INSERT ON delivery_cash_settlements
WHEN typeof(NEW.expected_cash_fils)!='integer' OR typeof(NEW.returned_cash_fils)!='integer' OR typeof(NEW.variance_fils)!='integer'
 OR NEW.expected_cash_fils<=0 OR NEW.returned_cash_fils<0 OR NEW.variance_fils!=NEW.returned_cash_fils-NEW.expected_cash_fils
 OR NEW.status!=CASE WHEN NEW.variance_fils=0 THEN 'SETTLED' ELSE 'DISCREPANCY' END
 OR NEW.expected_cash_fils!=(
   SELECT COALESCE(SUM(c.amount_fils),0) FROM delivery_collections c
   WHERE c.tenant_id=NEW.tenant_id AND c.branch_id=NEW.branch_id AND c.worker_id=NEW.worker_id AND c.method='CASH'
     AND NOT EXISTS (SELECT 1 FROM delivery_cash_settlement_allocations a WHERE a.collection_id=c.id)
 )
 OR NOT EXISTS (SELECT 1 FROM delivery_workers w WHERE w.id=NEW.worker_id AND w.tenant_id=NEW.tenant_id AND w.branch_id=NEW.branch_id AND w.active=1)
 OR NOT EXISTS (SELECT 1 FROM devices d WHERE d.id=NEW.device_id AND d.tenant_id=NEW.tenant_id AND d.branch_id=NEW.branch_id)
 OR NOT EXISTS (SELECT 1 FROM users u WHERE u.id=NEW.user_id AND u.tenant_id=NEW.tenant_id)
BEGIN SELECT RAISE(ABORT,'INVALID_DELIVERY_CASH_SETTLEMENT'); END;

CREATE TRIGGER IF NOT EXISTS guard_delivery_cash_allocation_insert BEFORE INSERT ON delivery_cash_settlement_allocations
WHEN typeof(NEW.amount_fils)!='integer' OR NEW.amount_fils<=0
 OR NOT EXISTS (
   SELECT 1 FROM delivery_cash_settlements s JOIN delivery_collections c ON c.id=NEW.collection_id
   WHERE s.id=NEW.settlement_id AND s.tenant_id=NEW.tenant_id AND c.tenant_id=NEW.tenant_id
     AND s.branch_id=c.branch_id AND s.worker_id=c.worker_id AND c.method='CASH' AND c.amount_fils=NEW.amount_fils
 )
BEGIN SELECT RAISE(ABORT,'INVALID_DELIVERY_CASH_ALLOCATION'); END;

INSERT INTO permissions(code,description) VALUES('delivery.manage','Create delivery orders and workers') ON CONFLICT(code) DO UPDATE SET description=excluded.description;
INSERT INTO permissions(code,description) VALUES('delivery.dispatch','Progress and dispatch delivery orders') ON CONFLICT(code) DO UPDATE SET description=excluded.description;
INSERT INTO permissions(code,description) VALUES('delivery.collect','Record delivery payment collections') ON CONFLICT(code) DO UPDATE SET description=excluded.description;
INSERT INTO permissions(code,description) VALUES('delivery.settle','Reconcile courier cash custody') ON CONFLICT(code) DO UPDATE SET description=excluded.description;

INSERT OR IGNORE INTO role_permissions(role_id,permission_code)
SELECT r.id,p.code FROM roles r JOIN permissions p ON p.code IN ('delivery.manage','delivery.dispatch','delivery.collect','delivery.settle')
WHERE lower(r.name) IN ('owner','administrator','admin','manager');
INSERT OR IGNORE INTO role_permissions(role_id,permission_code)
SELECT r.id,p.code FROM roles r JOIN permissions p ON p.code IN ('delivery.dispatch','delivery.collect')
WHERE lower(r.name) IN ('delivery staff','delivery');
INSERT OR IGNORE INTO role_permissions(role_id,permission_code)
SELECT r.id,p.code FROM roles r JOIN permissions p ON p.code='delivery.settle'
WHERE lower(r.name)='accountant';
