PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS tenants (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL,
  cr_number TEXT,
  vat_number TEXT,
  timezone TEXT NOT NULL DEFAULT 'Asia/Bahrain',
  currency TEXT NOT NULL DEFAULT 'BHD',
  business_close_hour INTEGER NOT NULL DEFAULT 3 CHECK(business_close_hour BETWEEN 0 AND 23),
  created_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS branches (
  id TEXT PRIMARY KEY,
  tenant_id TEXT NOT NULL REFERENCES tenants(id),
  code TEXT NOT NULL,
  name TEXT NOT NULL,
  active INTEGER NOT NULL DEFAULT 1,
  created_at TEXT NOT NULL,
  UNIQUE(tenant_id, code)
);
CREATE INDEX IF NOT EXISTS ix_branches_tenant ON branches(tenant_id);

CREATE TABLE IF NOT EXISTS users (
  id TEXT PRIMARY KEY,
  tenant_id TEXT NOT NULL REFERENCES tenants(id),
  employee_no TEXT,
  display_name TEXT NOT NULL,
  pin_hash TEXT NOT NULL,
  status TEXT NOT NULL CHECK(status IN ('ACTIVE','SUSPENDED','LOCKED')),
  failed_attempts INTEGER NOT NULL DEFAULT 0,
  locked_until TEXT,
  created_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS ix_users_tenant ON users(tenant_id);
CREATE TABLE IF NOT EXISTS roles (id TEXT PRIMARY KEY, tenant_id TEXT NOT NULL REFERENCES tenants(id), name TEXT NOT NULL, UNIQUE(tenant_id,name));
CREATE TABLE IF NOT EXISTS permissions (code TEXT PRIMARY KEY, description TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS role_permissions (role_id TEXT NOT NULL REFERENCES roles(id), permission_code TEXT NOT NULL REFERENCES permissions(code), PRIMARY KEY(role_id,permission_code));
CREATE TABLE IF NOT EXISTS user_roles (user_id TEXT NOT NULL REFERENCES users(id), role_id TEXT NOT NULL REFERENCES roles(id), branch_id TEXT REFERENCES branches(id), PRIMARY KEY(user_id,role_id,branch_id));

CREATE TABLE IF NOT EXISTS devices (
  id TEXT PRIMARY KEY,
  tenant_id TEXT NOT NULL REFERENCES tenants(id),
  branch_id TEXT NOT NULL REFERENCES branches(id),
  label TEXT NOT NULL,
  status TEXT NOT NULL CHECK(status IN ('ACTIVE','SUSPENDED','REVOKED')),
  credential_version INTEGER NOT NULL DEFAULT 1,
  credential_hash TEXT NOT NULL,
  last_heartbeat_at TEXT,
  app_version TEXT,
  created_at TEXT NOT NULL,
  revoked_at TEXT
);
CREATE INDEX IF NOT EXISTS ix_devices_scope ON devices(tenant_id,branch_id,status);

CREATE TABLE IF NOT EXISTS registers (
  id TEXT PRIMARY KEY,
  tenant_id TEXT NOT NULL REFERENCES tenants(id),
  branch_id TEXT NOT NULL REFERENCES branches(id),
  code TEXT NOT NULL,
  name TEXT NOT NULL,
  active INTEGER NOT NULL DEFAULT 1,
  UNIQUE(tenant_id,branch_id,code)
);
CREATE TABLE IF NOT EXISTS cash_sessions (
  id TEXT PRIMARY KEY,
  tenant_id TEXT NOT NULL REFERENCES tenants(id),
  branch_id TEXT NOT NULL REFERENCES branches(id),
  register_id TEXT NOT NULL REFERENCES registers(id),
  opened_by_user_id TEXT NOT NULL REFERENCES users(id),
  opened_on_device_id TEXT NOT NULL REFERENCES devices(id),
  opening_float_fils INTEGER NOT NULL,
  status TEXT NOT NULL CHECK(status IN ('OPEN','CLOSED','INTERRUPTED')),
  opened_at TEXT NOT NULL,
  closed_at TEXT,
  counted_cash_fils INTEGER,
  expected_cash_fils INTEGER,
  variance_fils INTEGER
);
CREATE UNIQUE INDEX IF NOT EXISTS ux_one_open_session_register ON cash_sessions(register_id) WHERE status='OPEN';
CREATE TABLE IF NOT EXISTS cash_movements (
  id TEXT PRIMARY KEY,
  tenant_id TEXT NOT NULL REFERENCES tenants(id), branch_id TEXT NOT NULL REFERENCES branches(id),
  cash_session_id TEXT NOT NULL REFERENCES cash_sessions(id), device_id TEXT NOT NULL REFERENCES devices(id), user_id TEXT NOT NULL REFERENCES users(id),
  operation_id TEXT NOT NULL, kind TEXT NOT NULL, amount_fils INTEGER NOT NULL, reason TEXT, created_at TEXT NOT NULL,
  UNIQUE(tenant_id, operation_id)
);

CREATE TABLE IF NOT EXISTS products (
  id TEXT PRIMARY KEY,
  tenant_id TEXT NOT NULL REFERENCES tenants(id),
  sku TEXT NOT NULL,
  name TEXT NOT NULL,
  description TEXT,
  base_price_fils INTEGER NOT NULL CHECK(base_price_fils>=0),
  current_cost_fils INTEGER NOT NULL DEFAULT 0 CHECK(current_cost_fils>=0),
  tax_category TEXT NOT NULL DEFAULT 'STANDARD',
  tax_rate_bps INTEGER NOT NULL DEFAULT 1000,
  tax_inclusive INTEGER NOT NULL DEFAULT 1,
  track_inventory INTEGER NOT NULL DEFAULT 1,
  allow_decimal_qty INTEGER NOT NULL DEFAULT 0,
  status TEXT NOT NULL DEFAULT 'ACTIVE',
  created_at TEXT NOT NULL,
  UNIQUE(tenant_id, sku)
);
CREATE TABLE IF NOT EXISTS product_barcodes (
  tenant_id TEXT NOT NULL REFERENCES tenants(id),
  barcode TEXT NOT NULL,
  product_id TEXT NOT NULL REFERENCES products(id),
  symbology TEXT NOT NULL DEFAULT 'UNKNOWN',
  is_primary INTEGER NOT NULL DEFAULT 0,
  source TEXT NOT NULL DEFAULT 'MANUAL',
  created_at TEXT NOT NULL,
  PRIMARY KEY(tenant_id, barcode)
);
CREATE INDEX IF NOT EXISTS ix_product_barcodes_product ON product_barcodes(product_id);
CREATE TABLE IF NOT EXISTS branch_assortments (
  tenant_id TEXT NOT NULL REFERENCES tenants(id), branch_id TEXT NOT NULL REFERENCES branches(id), product_id TEXT NOT NULL REFERENCES products(id),
  status TEXT NOT NULL CHECK(status IN ('CORE','OPTIONAL','SEASONAL','UNAVAILABLE','DISCONTINUED')),
  sellable INTEGER NOT NULL DEFAULT 1,
  PRIMARY KEY(tenant_id,branch_id,product_id)
);
CREATE TABLE IF NOT EXISTS price_history (
  id TEXT PRIMARY KEY,
  tenant_id TEXT NOT NULL REFERENCES tenants(id), branch_id TEXT REFERENCES branches(id), product_id TEXT NOT NULL REFERENCES products(id),
  channel TEXT NOT NULL DEFAULT 'POS', price_fils INTEGER NOT NULL CHECK(price_fils>=0),
  effective_from TEXT NOT NULL, effective_to TEXT, source TEXT NOT NULL, changed_by_user_id TEXT, reason TEXT, created_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS ix_price_lookup ON price_history(tenant_id,branch_id,product_id,channel,effective_from,effective_to);
CREATE TABLE IF NOT EXISTS cost_history (
  id TEXT PRIMARY KEY, tenant_id TEXT NOT NULL REFERENCES tenants(id), product_id TEXT NOT NULL REFERENCES products(id),
  cost_fils INTEGER NOT NULL, effective_from TEXT NOT NULL, source TEXT NOT NULL, created_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS inventory_centres (
  id TEXT PRIMARY KEY, tenant_id TEXT NOT NULL REFERENCES tenants(id), branch_id TEXT NOT NULL REFERENCES branches(id), code TEXT NOT NULL, name TEXT NOT NULL, centre_type TEXT NOT NULL,
  UNIQUE(tenant_id,branch_id,code)
);
CREATE TABLE IF NOT EXISTS stock_levels (
  tenant_id TEXT NOT NULL REFERENCES tenants(id), branch_id TEXT NOT NULL REFERENCES branches(id), centre_id TEXT NOT NULL REFERENCES inventory_centres(id), product_id TEXT NOT NULL REFERENCES products(id),
  quantity_milli INTEGER NOT NULL DEFAULT 0, version INTEGER NOT NULL DEFAULT 0,
  PRIMARY KEY(tenant_id,branch_id,centre_id,product_id)
);
CREATE TABLE IF NOT EXISTS inventory_movements (
  id TEXT PRIMARY KEY, tenant_id TEXT NOT NULL REFERENCES tenants(id), branch_id TEXT NOT NULL REFERENCES branches(id), centre_id TEXT NOT NULL REFERENCES inventory_centres(id), product_id TEXT NOT NULL REFERENCES products(id),
  operation_id TEXT NOT NULL, movement_type TEXT NOT NULL, quantity_milli INTEGER NOT NULL, unit_cost_fils INTEGER NOT NULL DEFAULT 0,
  source_type TEXT NOT NULL, source_id TEXT NOT NULL, device_id TEXT NOT NULL REFERENCES devices(id), user_id TEXT NOT NULL REFERENCES users(id), created_at TEXT NOT NULL,
  UNIQUE(tenant_id, operation_id, product_id, movement_type, source_id)
);
CREATE INDEX IF NOT EXISTS ix_inventory_movements_product ON inventory_movements(tenant_id,branch_id,product_id,created_at);

CREATE TABLE IF NOT EXISTS carts (
  id TEXT PRIMARY KEY, tenant_id TEXT NOT NULL REFERENCES tenants(id), branch_id TEXT NOT NULL REFERENCES branches(id), device_id TEXT NOT NULL REFERENCES devices(id),
  cashier_user_id TEXT NOT NULL REFERENCES users(id), customer_id TEXT, status TEXT NOT NULL CHECK(status IN ('ACTIVE','HELD','CHECKING_OUT','COMPLETED','CANCELLED')),
  note TEXT, version INTEGER NOT NULL DEFAULT 0, created_at TEXT NOT NULL, updated_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS ix_carts_scope ON carts(tenant_id,branch_id,device_id,status);
CREATE TABLE IF NOT EXISTS cart_lines (
  id TEXT PRIMARY KEY, cart_id TEXT NOT NULL REFERENCES carts(id) ON DELETE CASCADE, product_id TEXT NOT NULL REFERENCES products(id),
  product_name_snapshot TEXT NOT NULL, sku_snapshot TEXT NOT NULL, barcode_snapshot TEXT,
  quantity_milli INTEGER NOT NULL CHECK(quantity_milli>0), unit_price_fils INTEGER NOT NULL CHECK(unit_price_fils>=0), unit_cost_fils INTEGER NOT NULL DEFAULT 0,
  tax_category_snapshot TEXT NOT NULL, tax_rate_bps INTEGER NOT NULL, tax_inclusive INTEGER NOT NULL, line_note TEXT, created_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS receipt_sequences (
  tenant_id TEXT NOT NULL, branch_id TEXT NOT NULL, business_date TEXT NOT NULL, last_sequence INTEGER NOT NULL,
  PRIMARY KEY(tenant_id,branch_id,business_date)
);
CREATE TABLE IF NOT EXISTS sales (
  id TEXT PRIMARY KEY, tenant_id TEXT NOT NULL REFERENCES tenants(id), branch_id TEXT NOT NULL REFERENCES branches(id), device_id TEXT NOT NULL REFERENCES devices(id), register_id TEXT NOT NULL REFERENCES registers(id), cash_session_id TEXT,
  cart_id TEXT NOT NULL REFERENCES carts(id), cashier_user_id TEXT NOT NULL REFERENCES users(id), customer_id TEXT,
  operation_id TEXT NOT NULL, receipt_number TEXT NOT NULL, business_date TEXT NOT NULL,
  subtotal_fils INTEGER NOT NULL, tax_fils INTEGER NOT NULL, total_fils INTEGER NOT NULL, cogs_fils INTEGER NOT NULL,
  status TEXT NOT NULL DEFAULT 'COMPLETED', completed_at TEXT NOT NULL,
  UNIQUE(tenant_id,operation_id), UNIQUE(tenant_id,branch_id,receipt_number)
);
CREATE TABLE IF NOT EXISTS sale_lines (
  id TEXT PRIMARY KEY, sale_id TEXT NOT NULL REFERENCES sales(id), product_id TEXT NOT NULL REFERENCES products(id), product_name_snapshot TEXT NOT NULL, sku_snapshot TEXT NOT NULL, barcode_snapshot TEXT,
  quantity_milli INTEGER NOT NULL, unit_price_fils INTEGER NOT NULL, unit_cost_fils INTEGER NOT NULL, net_fils INTEGER NOT NULL, tax_fils INTEGER NOT NULL, gross_fils INTEGER NOT NULL,
  tax_category_snapshot TEXT NOT NULL, tax_rate_bps INTEGER NOT NULL, tax_inclusive INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS sale_payments (
  id TEXT PRIMARY KEY, sale_id TEXT NOT NULL REFERENCES sales(id), tender_kind TEXT NOT NULL, amount_fils INTEGER NOT NULL, tendered_fils INTEGER, change_fils INTEGER NOT NULL DEFAULT 0,
  reference TEXT, evidence_status TEXT, created_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS receipt_snapshots (
  sale_id TEXT PRIMARY KEY REFERENCES sales(id), format_version INTEGER NOT NULL, receipt_text TEXT NOT NULL, receipt_sha256 TEXT NOT NULL, created_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS refunds (
  id TEXT PRIMARY KEY, tenant_id TEXT NOT NULL REFERENCES tenants(id), branch_id TEXT NOT NULL REFERENCES branches(id), sale_id TEXT NOT NULL REFERENCES sales(id),
  operation_id TEXT NOT NULL, device_id TEXT NOT NULL REFERENCES devices(id), user_id TEXT NOT NULL REFERENCES users(id), reason TEXT NOT NULL,
  subtotal_fils INTEGER NOT NULL, tax_fils INTEGER NOT NULL, total_fils INTEGER NOT NULL, created_at TEXT NOT NULL,
  UNIQUE(tenant_id,operation_id)
);
CREATE TABLE IF NOT EXISTS refund_lines (
  id TEXT PRIMARY KEY, refund_id TEXT NOT NULL REFERENCES refunds(id), sale_line_id TEXT NOT NULL REFERENCES sale_lines(id), quantity_milli INTEGER NOT NULL,
  net_fils INTEGER NOT NULL, tax_fils INTEGER NOT NULL, gross_fils INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS unknown_barcodes (
  tenant_id TEXT NOT NULL, branch_id TEXT NOT NULL, barcode TEXT NOT NULL, first_seen_at TEXT NOT NULL, last_seen_at TEXT NOT NULL, scan_count INTEGER NOT NULL DEFAULT 1,
  last_device_id TEXT NOT NULL, status TEXT NOT NULL DEFAULT 'OPEN', resolution_product_id TEXT,
  PRIMARY KEY(tenant_id,branch_id,barcode)
);

CREATE TABLE IF NOT EXISTS idempotency_results (
  tenant_id TEXT NOT NULL, operation_id TEXT NOT NULL, action TEXT NOT NULL, result_json TEXT NOT NULL, committed_at TEXT NOT NULL,
  PRIMARY KEY(tenant_id,operation_id,action)
);
CREATE TABLE IF NOT EXISTS sync_queue (
  id TEXT PRIMARY KEY, tenant_id TEXT NOT NULL, branch_id TEXT NOT NULL, device_id TEXT NOT NULL, operation_id TEXT NOT NULL,
  entity_type TEXT NOT NULL, entity_id TEXT NOT NULL, mutation_type TEXT NOT NULL, payload_json TEXT NOT NULL,
  state TEXT NOT NULL CHECK(state IN ('PENDING','SENDING','RETRYING','COMMITTED','FAILED','REQUIRES_REVIEW','RESOLVED')),
  attempts INTEGER NOT NULL DEFAULT 0, created_at TEXT NOT NULL, updated_at TEXT NOT NULL,
  UNIQUE(tenant_id,device_id,operation_id,entity_type,entity_id)
);
CREATE TABLE IF NOT EXISTS audit_events (
  id TEXT PRIMARY KEY, tenant_id TEXT NOT NULL, device_id TEXT NOT NULL, actor_user_id TEXT NOT NULL,
  event_type TEXT NOT NULL, entity_type TEXT NOT NULL, entity_id TEXT NOT NULL, payload_json TEXT NOT NULL,
  previous_hash TEXT NOT NULL, event_hash TEXT NOT NULL, created_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS ix_audit_chain ON audit_events(tenant_id,device_id,created_at,id);
