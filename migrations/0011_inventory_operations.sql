CREATE TABLE IF NOT EXISTS inventory_cost_balances (
  tenant_id TEXT NOT NULL REFERENCES tenants(id),
  branch_id TEXT NOT NULL REFERENCES branches(id),
  centre_id TEXT NOT NULL REFERENCES inventory_centres(id),
  product_id TEXT NOT NULL REFERENCES products(id),
  quantity_milli INTEGER NOT NULL,
  total_value_fils INTEGER NOT NULL,
  version INTEGER NOT NULL DEFAULT 0,
  PRIMARY KEY(tenant_id,branch_id,centre_id,product_id)
);

CREATE TABLE IF NOT EXISTS inventory_movement_lots (
  movement_id TEXT PRIMARY KEY REFERENCES inventory_movements(id),
  lot_id TEXT NOT NULL REFERENCES inventory_lots(id),
  quantity_milli INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS inventory_operation_results (
  tenant_id TEXT NOT NULL REFERENCES tenants(id),
  operation_id TEXT NOT NULL,
  action TEXT NOT NULL,
  request_sha256 TEXT NOT NULL,
  result_json TEXT NOT NULL,
  committed_at TEXT NOT NULL,
  PRIMARY KEY(tenant_id,operation_id,action),
  UNIQUE(tenant_id,operation_id)
);

CREATE TABLE IF NOT EXISTS inventory_reconciliation_runs (
  id TEXT PRIMARY KEY,
  tenant_id TEXT NOT NULL REFERENCES tenants(id),
  branch_id TEXT NOT NULL REFERENCES branches(id),
  device_id TEXT NOT NULL REFERENCES devices(id),
  user_id TEXT NOT NULL REFERENCES users(id),
  mismatches INTEGER NOT NULL,
  repaired INTEGER NOT NULL CHECK(repaired IN (0,1)),
  evidence_json TEXT NOT NULL,
  created_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS inventory_transfer_receipts (
  id TEXT PRIMARY KEY,
  tenant_id TEXT NOT NULL REFERENCES tenants(id),
  transfer_id TEXT NOT NULL REFERENCES inventory_transfers(id),
  operation_id TEXT NOT NULL,
  device_id TEXT NOT NULL REFERENCES devices(id),
  user_id TEXT NOT NULL REFERENCES users(id),
  received_at TEXT NOT NULL,
  UNIQUE(tenant_id,operation_id)
);

CREATE TABLE IF NOT EXISTS inventory_transfer_receipt_lines (
  id TEXT PRIMARY KEY,
  receipt_id TEXT NOT NULL REFERENCES inventory_transfer_receipts(id),
  transfer_line_id TEXT NOT NULL REFERENCES inventory_transfer_lines(id),
  received_qty_milli INTEGER NOT NULL,
  damaged_qty_milli INTEGER NOT NULL DEFAULT 0,
  note TEXT
);

CREATE TABLE IF NOT EXISTS stocktake_count_events (
  id TEXT PRIMARY KEY,
  tenant_id TEXT NOT NULL REFERENCES tenants(id),
  stocktake_id TEXT NOT NULL REFERENCES stocktakes(id),
  stocktake_line_id TEXT NOT NULL REFERENCES stocktake_lines(id),
  counted_qty_milli INTEGER NOT NULL,
  device_id TEXT NOT NULL REFERENCES devices(id),
  user_id TEXT NOT NULL REFERENCES users(id),
  counted_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS stocktake_approvals (
  stocktake_id TEXT PRIMARY KEY REFERENCES stocktakes(id),
  tenant_id TEXT NOT NULL REFERENCES tenants(id),
  operation_id TEXT NOT NULL,
  approved_by_user_id TEXT NOT NULL REFERENCES users(id),
  device_id TEXT NOT NULL REFERENCES devices(id),
  evidence_json TEXT NOT NULL,
  approved_at TEXT NOT NULL,
  UNIQUE(tenant_id,operation_id)
);

CREATE TRIGGER IF NOT EXISTS immutable_inventory_movement_lots_update BEFORE UPDATE ON inventory_movement_lots BEGIN SELECT RAISE(ABORT,'IMMUTABLE:inventory_movement_lots'); END;
CREATE TRIGGER IF NOT EXISTS immutable_inventory_movement_lots_delete BEFORE DELETE ON inventory_movement_lots BEGIN SELECT RAISE(ABORT,'IMMUTABLE:inventory_movement_lots'); END;
CREATE TRIGGER IF NOT EXISTS immutable_inventory_operation_results_update BEFORE UPDATE ON inventory_operation_results BEGIN SELECT RAISE(ABORT,'IMMUTABLE:inventory_operation_results'); END;
CREATE TRIGGER IF NOT EXISTS immutable_inventory_operation_results_delete BEFORE DELETE ON inventory_operation_results BEGIN SELECT RAISE(ABORT,'IMMUTABLE:inventory_operation_results'); END;
CREATE TRIGGER IF NOT EXISTS immutable_inventory_transfer_receipts_update BEFORE UPDATE ON inventory_transfer_receipts BEGIN SELECT RAISE(ABORT,'IMMUTABLE:inventory_transfer_receipts'); END;
CREATE TRIGGER IF NOT EXISTS immutable_inventory_transfer_receipts_delete BEFORE DELETE ON inventory_transfer_receipts BEGIN SELECT RAISE(ABORT,'IMMUTABLE:inventory_transfer_receipts'); END;
CREATE TRIGGER IF NOT EXISTS immutable_stocktake_count_events_update BEFORE UPDATE ON stocktake_count_events BEGIN SELECT RAISE(ABORT,'IMMUTABLE:stocktake_count_events'); END;
CREATE TRIGGER IF NOT EXISTS immutable_stocktake_count_events_delete BEFORE DELETE ON stocktake_count_events BEGIN SELECT RAISE(ABORT,'IMMUTABLE:stocktake_count_events'); END;
CREATE TRIGGER IF NOT EXISTS immutable_stocktake_approvals_update BEFORE UPDATE ON stocktake_approvals BEGIN SELECT RAISE(ABORT,'IMMUTABLE:stocktake_approvals'); END;
CREATE TRIGGER IF NOT EXISTS immutable_stocktake_approvals_delete BEFORE DELETE ON stocktake_approvals BEGIN SELECT RAISE(ABORT,'IMMUTABLE:stocktake_approvals'); END;
CREATE TRIGGER IF NOT EXISTS immutable_waste_events_update BEFORE UPDATE ON waste_events BEGIN SELECT RAISE(ABORT,'IMMUTABLE:waste_events'); END;
CREATE TRIGGER IF NOT EXISTS immutable_waste_events_delete BEFORE DELETE ON waste_events BEGIN SELECT RAISE(ABORT,'IMMUTABLE:waste_events'); END;

CREATE TRIGGER IF NOT EXISTS guard_inventory_cost_balances_insert BEFORE INSERT ON inventory_cost_balances
WHEN NOT EXISTS (SELECT 1 FROM inventory_centres c WHERE c.id=NEW.centre_id AND c.tenant_id=NEW.tenant_id AND c.branch_id=NEW.branch_id)
 OR NOT EXISTS (SELECT 1 FROM products p WHERE p.id=NEW.product_id AND p.tenant_id=NEW.tenant_id)
BEGIN SELECT RAISE(ABORT,'TENANT_SCOPE_VIOLATION:inventory_cost_balances'); END;

CREATE TRIGGER IF NOT EXISTS guard_inventory_cost_balances_values_insert BEFORE INSERT ON inventory_cost_balances
WHEN typeof(NEW.quantity_milli)!='integer' OR typeof(NEW.total_value_fils)!='integer'
BEGIN SELECT RAISE(ABORT,'INVALID_INVENTORY_COST_BALANCE'); END;

CREATE TRIGGER IF NOT EXISTS guard_inventory_cost_balances_values_update BEFORE UPDATE OF quantity_milli,total_value_fils ON inventory_cost_balances
WHEN typeof(NEW.quantity_milli)!='integer' OR typeof(NEW.total_value_fils)!='integer'
BEGIN SELECT RAISE(ABORT,'INVALID_INVENTORY_COST_BALANCE'); END;

CREATE TRIGGER IF NOT EXISTS guard_inventory_movement_lots_values BEFORE INSERT ON inventory_movement_lots
WHEN typeof(NEW.quantity_milli)!='integer' OR NEW.quantity_milli=0
BEGIN SELECT RAISE(ABORT,'INVALID_INVENTORY_LOT_MOVEMENT'); END;

CREATE TRIGGER IF NOT EXISTS guard_waste_events_insert_v2 BEFORE INSERT ON waste_events
WHEN NOT EXISTS (SELECT 1 FROM inventory_centres c WHERE c.id=NEW.centre_id AND c.tenant_id=NEW.tenant_id AND c.branch_id=NEW.branch_id)
 OR NOT EXISTS (SELECT 1 FROM products p WHERE p.id=NEW.product_id AND p.tenant_id=NEW.tenant_id)
 OR NOT EXISTS (SELECT 1 FROM devices d WHERE d.id=NEW.device_id AND d.tenant_id=NEW.tenant_id AND d.branch_id=NEW.branch_id)
BEGIN SELECT RAISE(ABORT,'TENANT_SCOPE_VIOLATION:waste_events'); END;

INSERT INTO permissions(code,description) VALUES('inventory.receive','Post inventory receiving') ON CONFLICT(code) DO UPDATE SET description=excluded.description;
INSERT INTO permissions(code,description) VALUES('inventory.transfer','Dispatch and receive stock transfers') ON CONFLICT(code) DO UPDATE SET description=excluded.description;
INSERT INTO permissions(code,description) VALUES('inventory.stocktake','Count and approve stocktakes') ON CONFLICT(code) DO UPDATE SET description=excluded.description;
INSERT INTO permissions(code,description) VALUES('inventory.waste','Record inventory waste') ON CONFLICT(code) DO UPDATE SET description=excluded.description;
INSERT INTO permissions(code,description) VALUES('inventory.reconcile','Verify and rebuild inventory projections') ON CONFLICT(code) DO UPDATE SET description=excluded.description;

INSERT OR IGNORE INTO role_permissions(role_id,permission_code)
SELECT r.id,p.code FROM roles r JOIN permissions p ON p.code IN ('inventory.receive','inventory.transfer','inventory.stocktake','inventory.waste','inventory.reconcile')
WHERE lower(r.name) IN ('owner','administrator','admin');
