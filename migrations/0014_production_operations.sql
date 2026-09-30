CREATE TABLE IF NOT EXISTS production_events (
  id TEXT PRIMARY KEY, tenant_id TEXT NOT NULL REFERENCES tenants(id), production_order_id TEXT NOT NULL REFERENCES production_orders(id),
  event_type TEXT NOT NULL CHECK(event_type IN ('CREATED','STARTED','COMPLETED','CANCELLED')),
  operation_id TEXT NOT NULL, device_id TEXT NOT NULL REFERENCES devices(id), user_id TEXT NOT NULL REFERENCES users(id),
  evidence_json TEXT NOT NULL, created_at TEXT NOT NULL, UNIQUE(tenant_id,operation_id,event_type)
);

CREATE TRIGGER IF NOT EXISTS immutable_production_events_update BEFORE UPDATE ON production_events BEGIN SELECT RAISE(ABORT,'IMMUTABLE:production_events'); END;
CREATE TRIGGER IF NOT EXISTS immutable_production_events_delete BEFORE DELETE ON production_events BEGIN SELECT RAISE(ABORT,'IMMUTABLE:production_events'); END;
CREATE TRIGGER IF NOT EXISTS immutable_production_usage_financial BEFORE UPDATE OF production_order_id,product_id,expected_qty_milli,actual_qty_milli,lot_id,cost_fils ON production_usage BEGIN SELECT RAISE(ABORT,'IMMUTABLE:production_usage'); END;
CREATE TRIGGER IF NOT EXISTS immutable_production_usage_delete BEFORE DELETE ON production_usage BEGIN SELECT RAISE(ABORT,'IMMUTABLE:production_usage'); END;
CREATE TRIGGER IF NOT EXISTS guard_production_events_insert BEFORE INSERT ON production_events
WHEN NOT EXISTS (SELECT 1 FROM production_orders p WHERE p.id=NEW.production_order_id AND p.tenant_id=NEW.tenant_id)
 OR NOT EXISTS (SELECT 1 FROM users u WHERE u.id=NEW.user_id AND u.tenant_id=NEW.tenant_id)
 OR NOT EXISTS (SELECT 1 FROM devices d JOIN production_orders p ON p.id=NEW.production_order_id WHERE d.id=NEW.device_id AND d.tenant_id=NEW.tenant_id AND d.branch_id=p.branch_id)
BEGIN SELECT RAISE(ABORT,'TENANT_SCOPE_VIOLATION:production_events'); END;

INSERT INTO permissions(code,description) VALUES('production.manage','Create and complete production orders') ON CONFLICT(code) DO UPDATE SET description=excluded.description;
INSERT OR IGNORE INTO role_permissions(role_id,permission_code)
SELECT r.id,'production.manage' FROM roles r WHERE lower(r.name) IN ('owner','administrator','admin','manager','inventory staff');
