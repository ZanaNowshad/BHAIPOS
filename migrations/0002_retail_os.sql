PRAGMA foreign_keys = ON;

-- Category, pricing, promotions, bundles
CREATE TABLE IF NOT EXISTS categories (
  id TEXT PRIMARY KEY, tenant_id TEXT NOT NULL REFERENCES tenants(id), parent_id TEXT REFERENCES categories(id),
  name TEXT NOT NULL, sort_order INTEGER NOT NULL DEFAULT 0, active INTEGER NOT NULL DEFAULT 1,
  pos_visible INTEGER NOT NULL DEFAULT 1, metadata_json TEXT NOT NULL DEFAULT '{}', UNIQUE(tenant_id,parent_id,name)
);
CREATE TABLE IF NOT EXISTS product_categories (
  tenant_id TEXT NOT NULL, product_id TEXT NOT NULL REFERENCES products(id), category_id TEXT NOT NULL REFERENCES categories(id),
  is_primary INTEGER NOT NULL DEFAULT 0, PRIMARY KEY(tenant_id,product_id,category_id)
);
CREATE TABLE IF NOT EXISTS sellability_schedules (
  id TEXT PRIMARY KEY, tenant_id TEXT NOT NULL REFERENCES tenants(id), branch_id TEXT REFERENCES branches(id),
  entity_type TEXT NOT NULL CHECK(entity_type IN ('PRODUCT','CATEGORY')), entity_id TEXT NOT NULL,
  starts_at TEXT, ends_at TEXT, weekdays_mask INTEGER, start_minute INTEGER, end_minute INTEGER,
  active INTEGER NOT NULL DEFAULT 1, created_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS pricing_policies (
  id TEXT PRIMARY KEY, tenant_id TEXT NOT NULL REFERENCES tenants(id), branch_id TEXT REFERENCES branches(id), category_id TEXT REFERENCES categories(id),
  name TEXT NOT NULL, target_margin_bps INTEGER, markup_bps INTEGER, min_margin_bps INTEGER, min_price_fils INTEGER,
  rounding_increment_fils INTEGER, allowed_endings_json TEXT, vat_mode TEXT NOT NULL DEFAULT 'INHERIT', active INTEGER NOT NULL DEFAULT 1,
  priority INTEGER NOT NULL DEFAULT 100, created_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS promotions (
  id TEXT PRIMARY KEY, tenant_id TEXT NOT NULL REFERENCES tenants(id), name TEXT NOT NULL, promotion_type TEXT NOT NULL,
  priority INTEGER NOT NULL DEFAULT 100, stacking_mode TEXT NOT NULL CHECK(stacking_mode IN ('EXCLUSIVE','STACKABLE','BEST_PRICE','PRIORITY')),
  starts_at TEXT NOT NULL, ends_at TEXT NOT NULL, weekdays_mask INTEGER, start_minute INTEGER, end_minute INTEGER,
  customer_segment TEXT, channel TEXT, active INTEGER NOT NULL DEFAULT 1, config_json TEXT NOT NULL, created_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS promotion_branches (promotion_id TEXT NOT NULL REFERENCES promotions(id), branch_id TEXT NOT NULL REFERENCES branches(id), PRIMARY KEY(promotion_id,branch_id));
CREATE TABLE IF NOT EXISTS promotion_products (promotion_id TEXT NOT NULL REFERENCES promotions(id), product_id TEXT NOT NULL REFERENCES products(id), PRIMARY KEY(promotion_id,product_id));
CREATE TABLE IF NOT EXISTS promotion_categories (promotion_id TEXT NOT NULL REFERENCES promotions(id), category_id TEXT NOT NULL REFERENCES categories(id), PRIMARY KEY(promotion_id,category_id));
CREATE TABLE IF NOT EXISTS coupons (
  id TEXT PRIMARY KEY, tenant_id TEXT NOT NULL REFERENCES tenants(id), code TEXT NOT NULL, promotion_id TEXT REFERENCES promotions(id),
  starts_at TEXT NOT NULL, ends_at TEXT NOT NULL, min_spend_fils INTEGER NOT NULL DEFAULT 0, total_usage_limit INTEGER,
  per_customer_limit INTEGER, active INTEGER NOT NULL DEFAULT 1, constraints_json TEXT NOT NULL DEFAULT '{}', UNIQUE(tenant_id,code)
);
CREATE TABLE IF NOT EXISTS coupon_redemptions (
  id TEXT PRIMARY KEY, tenant_id TEXT NOT NULL REFERENCES tenants(id), coupon_id TEXT NOT NULL REFERENCES coupons(id),
  sale_id TEXT NOT NULL REFERENCES sales(id), customer_id TEXT, redeemed_at TEXT NOT NULL, UNIQUE(coupon_id,sale_id)
);
CREATE TABLE IF NOT EXISTS bundles (
  id TEXT PRIMARY KEY, tenant_id TEXT NOT NULL REFERENCES tenants(id), sellable_product_id TEXT NOT NULL REFERENCES products(id),
  bundle_type TEXT NOT NULL CHECK(bundle_type IN ('KIT','GIFT_SET','HAMPER')), packaging_cost_fils INTEGER NOT NULL DEFAULT 0,
  active INTEGER NOT NULL DEFAULT 1, created_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS bundle_components (
  bundle_id TEXT NOT NULL REFERENCES bundles(id), component_product_id TEXT NOT NULL REFERENCES products(id), quantity_milli INTEGER NOT NULL,
  PRIMARY KEY(bundle_id,component_product_id)
);

-- Inventory lots, transfers, stocktake, replenishment
CREATE TABLE IF NOT EXISTS inventory_lots (
  id TEXT PRIMARY KEY, tenant_id TEXT NOT NULL REFERENCES tenants(id), product_id TEXT NOT NULL REFERENCES products(id), supplier_id TEXT,
  lot_number TEXT, manufactured_on TEXT, expires_on TEXT, status TEXT NOT NULL DEFAULT 'ACTIVE', unit_cost_fils INTEGER NOT NULL DEFAULT 0,
  received_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS lot_balances (
  tenant_id TEXT NOT NULL, branch_id TEXT NOT NULL REFERENCES branches(id), centre_id TEXT NOT NULL REFERENCES inventory_centres(id),
  lot_id TEXT NOT NULL REFERENCES inventory_lots(id), quantity_milli INTEGER NOT NULL, PRIMARY KEY(tenant_id,branch_id,centre_id,lot_id)
);
CREATE TABLE IF NOT EXISTS inventory_transfers (
  id TEXT PRIMARY KEY, tenant_id TEXT NOT NULL REFERENCES tenants(id), from_branch_id TEXT NOT NULL REFERENCES branches(id), to_branch_id TEXT NOT NULL REFERENCES branches(id),
  from_centre_id TEXT NOT NULL REFERENCES inventory_centres(id), to_centre_id TEXT NOT NULL REFERENCES inventory_centres(id),
  status TEXT NOT NULL CHECK(status IN ('DRAFT','DISPATCHED','PARTIALLY_RECEIVED','RECEIVED','CANCELLED','REQUIRES_REVIEW')),
  operation_id TEXT NOT NULL, created_by_user_id TEXT NOT NULL REFERENCES users(id), device_id TEXT NOT NULL REFERENCES devices(id),
  created_at TEXT NOT NULL, dispatched_at TEXT, completed_at TEXT, UNIQUE(tenant_id,operation_id)
);
CREATE TABLE IF NOT EXISTS inventory_transfer_lines (
  id TEXT PRIMARY KEY, transfer_id TEXT NOT NULL REFERENCES inventory_transfers(id), product_id TEXT NOT NULL REFERENCES products(id), lot_id TEXT REFERENCES inventory_lots(id),
  requested_qty_milli INTEGER NOT NULL, dispatched_qty_milli INTEGER NOT NULL DEFAULT 0, received_qty_milli INTEGER NOT NULL DEFAULT 0,
  damaged_qty_milli INTEGER NOT NULL DEFAULT 0, discrepancy_note TEXT
);
CREATE TABLE IF NOT EXISTS stocktakes (
  id TEXT PRIMARY KEY, tenant_id TEXT NOT NULL REFERENCES tenants(id), branch_id TEXT NOT NULL REFERENCES branches(id), centre_id TEXT NOT NULL REFERENCES inventory_centres(id),
  status TEXT NOT NULL CHECK(status IN ('DRAFT','COUNTING','REVIEW','APPROVED','CANCELLED')), snapshot_at TEXT NOT NULL,
  created_by_user_id TEXT NOT NULL REFERENCES users(id), approved_by_user_id TEXT REFERENCES users(id), operation_id TEXT,
  created_at TEXT NOT NULL, approved_at TEXT
);
CREATE TABLE IF NOT EXISTS stocktake_lines (
  id TEXT PRIMARY KEY, stocktake_id TEXT NOT NULL REFERENCES stocktakes(id), product_id TEXT NOT NULL REFERENCES products(id), lot_id TEXT REFERENCES inventory_lots(id),
  expected_qty_milli INTEGER NOT NULL, counted_qty_milli INTEGER, movements_after_snapshot_milli INTEGER NOT NULL DEFAULT 0,
  reconciled_expected_qty_milli INTEGER, variance_qty_milli INTEGER, status TEXT NOT NULL DEFAULT 'OPEN', UNIQUE(stocktake_id,product_id,lot_id)
);
CREATE TABLE IF NOT EXISTS reorder_policies (
  tenant_id TEXT NOT NULL, branch_id TEXT NOT NULL REFERENCES branches(id), product_id TEXT NOT NULL REFERENCES products(id),
  reorder_point_milli INTEGER, target_stock_milli INTEGER, lead_time_days INTEGER, safety_stock_milli INTEGER, preferred_supplier_id TEXT,
  active INTEGER NOT NULL DEFAULT 1, PRIMARY KEY(tenant_id,branch_id,product_id)
);

-- Suppliers, purchasing, AP
CREATE TABLE IF NOT EXISTS suppliers (
  id TEXT PRIMARY KEY, tenant_id TEXT NOT NULL REFERENCES tenants(id), name TEXT NOT NULL, cr_number TEXT, vat_number TEXT,
  phone TEXT, email TEXT, address_json TEXT, payment_terms_days INTEGER, notes TEXT, active INTEGER NOT NULL DEFAULT 1, created_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS ix_suppliers_tenant ON suppliers(tenant_id,name);
CREATE TABLE IF NOT EXISTS supplier_products (
  id TEXT PRIMARY KEY, tenant_id TEXT NOT NULL REFERENCES tenants(id), supplier_id TEXT NOT NULL REFERENCES suppliers(id), product_id TEXT NOT NULL REFERENCES products(id),
  supplier_sku TEXT, supplier_barcode TEXT, case_size_milli INTEGER NOT NULL DEFAULT 1000, moq_milli INTEGER NOT NULL DEFAULT 1000,
  current_cost_fils INTEGER NOT NULL, lead_time_days INTEGER, preferred INTEGER NOT NULL DEFAULT 0, last_purchase_cost_fils INTEGER,
  UNIQUE(tenant_id,supplier_id,product_id)
);
CREATE TABLE IF NOT EXISTS purchase_requisitions (
  id TEXT PRIMARY KEY, tenant_id TEXT NOT NULL REFERENCES tenants(id), branch_id TEXT NOT NULL REFERENCES branches(id),
  status TEXT NOT NULL CHECK(status IN ('DRAFT','SUBMITTED','APPROVED','REJECTED','CONVERTED','CANCELLED')),
  requested_by_user_id TEXT NOT NULL REFERENCES users(id), approved_by_user_id TEXT REFERENCES users(id), note TEXT, created_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS purchase_requisition_lines (
  id TEXT PRIMARY KEY, requisition_id TEXT NOT NULL REFERENCES purchase_requisitions(id), product_id TEXT NOT NULL REFERENCES products(id),
  requested_qty_milli INTEGER NOT NULL, suggested_supplier_id TEXT REFERENCES suppliers(id), note TEXT
);
CREATE TABLE IF NOT EXISTS purchase_orders (
  id TEXT PRIMARY KEY, tenant_id TEXT NOT NULL REFERENCES tenants(id), branch_id TEXT NOT NULL REFERENCES branches(id), supplier_id TEXT NOT NULL REFERENCES suppliers(id),
  po_number TEXT NOT NULL, status TEXT NOT NULL CHECK(status IN ('DRAFT','APPROVED','ORDERED','PARTIALLY_RECEIVED','RECEIVED','CANCELLED')),
  subtotal_fils INTEGER NOT NULL DEFAULT 0, tax_fils INTEGER NOT NULL DEFAULT 0, total_fils INTEGER NOT NULL DEFAULT 0,
  created_by_user_id TEXT NOT NULL REFERENCES users(id), approved_by_user_id TEXT REFERENCES users(id), ordered_at TEXT, created_at TEXT NOT NULL,
  UNIQUE(tenant_id,po_number)
);
CREATE TABLE IF NOT EXISTS purchase_order_lines (
  id TEXT PRIMARY KEY, po_id TEXT NOT NULL REFERENCES purchase_orders(id), product_id TEXT NOT NULL REFERENCES products(id),
  ordered_qty_milli INTEGER NOT NULL, unit_cost_fils INTEGER NOT NULL, tax_rate_bps INTEGER NOT NULL DEFAULT 0,
  received_qty_milli INTEGER NOT NULL DEFAULT 0, line_total_fils INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS goods_receipts (
  id TEXT PRIMARY KEY, tenant_id TEXT NOT NULL REFERENCES tenants(id), branch_id TEXT NOT NULL REFERENCES branches(id), supplier_id TEXT NOT NULL REFERENCES suppliers(id),
  po_id TEXT REFERENCES purchase_orders(id), centre_id TEXT NOT NULL REFERENCES inventory_centres(id), operation_id TEXT NOT NULL,
  status TEXT NOT NULL CHECK(status IN ('DRAFT','POSTED','REQUIRES_REVIEW','CANCELLED')), device_id TEXT NOT NULL REFERENCES devices(id),
  received_by_user_id TEXT NOT NULL REFERENCES users(id), supplier_document_no TEXT, received_at TEXT NOT NULL, UNIQUE(tenant_id,operation_id)
);
CREATE TABLE IF NOT EXISTS goods_receipt_lines (
  id TEXT PRIMARY KEY, receipt_id TEXT NOT NULL REFERENCES goods_receipts(id), po_line_id TEXT REFERENCES purchase_order_lines(id), product_id TEXT NOT NULL REFERENCES products(id),
  lot_id TEXT REFERENCES inventory_lots(id), ordered_qty_milli INTEGER, received_qty_milli INTEGER NOT NULL, rejected_qty_milli INTEGER NOT NULL DEFAULT 0,
  damaged_qty_milli INTEGER NOT NULL DEFAULT 0, unit_cost_fils INTEGER NOT NULL, discrepancy_type TEXT, discrepancy_note TEXT
);
CREATE TABLE IF NOT EXISTS supplier_invoices (
  id TEXT PRIMARY KEY, tenant_id TEXT NOT NULL REFERENCES tenants(id), branch_id TEXT NOT NULL REFERENCES branches(id), supplier_id TEXT NOT NULL REFERENCES suppliers(id),
  invoice_number TEXT NOT NULL, invoice_date TEXT NOT NULL, due_date TEXT, po_id TEXT REFERENCES purchase_orders(id), goods_receipt_id TEXT REFERENCES goods_receipts(id),
  subtotal_fils INTEGER NOT NULL, tax_fils INTEGER NOT NULL, total_fils INTEGER NOT NULL, amount_paid_fils INTEGER NOT NULL DEFAULT 0,
  status TEXT NOT NULL CHECK(status IN ('DRAFT','OPEN','PARTIALLY_PAID','PAID','CREDITED','VOID')), attachment_id TEXT, created_at TEXT NOT NULL,
  UNIQUE(tenant_id,supplier_id,invoice_number)
);
CREATE TABLE IF NOT EXISTS supplier_invoice_lines (
  id TEXT PRIMARY KEY, invoice_id TEXT NOT NULL REFERENCES supplier_invoices(id), product_id TEXT REFERENCES products(id), description TEXT NOT NULL,
  quantity_milli INTEGER NOT NULL, unit_cost_fils INTEGER NOT NULL, tax_fils INTEGER NOT NULL, line_total_fils INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS supplier_payments (
  id TEXT PRIMARY KEY, tenant_id TEXT NOT NULL REFERENCES tenants(id), branch_id TEXT NOT NULL REFERENCES branches(id), supplier_id TEXT NOT NULL REFERENCES suppliers(id),
  operation_id TEXT NOT NULL, device_id TEXT NOT NULL REFERENCES devices(id), user_id TEXT NOT NULL REFERENCES users(id), method TEXT NOT NULL,
  amount_fils INTEGER NOT NULL, reference TEXT, paid_at TEXT NOT NULL, attachment_id TEXT, UNIQUE(tenant_id,operation_id)
);
CREATE TABLE IF NOT EXISTS supplier_payment_allocations (
  payment_id TEXT NOT NULL REFERENCES supplier_payments(id), invoice_id TEXT NOT NULL REFERENCES supplier_invoices(id), amount_fils INTEGER NOT NULL,
  PRIMARY KEY(payment_id,invoice_id)
);
CREATE TABLE IF NOT EXISTS supplier_ledger (
  id TEXT PRIMARY KEY, tenant_id TEXT NOT NULL REFERENCES tenants(id), supplier_id TEXT NOT NULL REFERENCES suppliers(id), branch_id TEXT REFERENCES branches(id),
  event_type TEXT NOT NULL, source_type TEXT NOT NULL, source_id TEXT NOT NULL, debit_fils INTEGER NOT NULL DEFAULT 0, credit_fils INTEGER NOT NULL DEFAULT 0,
  occurred_at TEXT NOT NULL, operation_id TEXT, note TEXT
);
CREATE INDEX IF NOT EXISTS ix_supplier_ledger ON supplier_ledger(tenant_id,supplier_id,occurred_at,id);
CREATE TABLE IF NOT EXISTS supplier_returns (
  id TEXT PRIMARY KEY, tenant_id TEXT NOT NULL REFERENCES tenants(id), branch_id TEXT NOT NULL REFERENCES branches(id), supplier_id TEXT NOT NULL REFERENCES suppliers(id),
  status TEXT NOT NULL CHECK(status IN ('DRAFT','DISPATCHED','ACKNOWLEDGED','CREDITED','REPLACED','CLOSED','CANCELLED')), reason TEXT NOT NULL,
  operation_id TEXT NOT NULL, created_by_user_id TEXT NOT NULL REFERENCES users(id), device_id TEXT NOT NULL REFERENCES devices(id), created_at TEXT NOT NULL,
  UNIQUE(tenant_id,operation_id)
);
CREATE TABLE IF NOT EXISTS supplier_return_lines (
  id TEXT PRIMARY KEY, return_id TEXT NOT NULL REFERENCES supplier_returns(id), product_id TEXT NOT NULL REFERENCES products(id), lot_id TEXT REFERENCES inventory_lots(id),
  quantity_milli INTEGER NOT NULL, unit_cost_fils INTEGER NOT NULL, resolution TEXT
);

-- Expenses and petty cash
CREATE TABLE IF NOT EXISTS expense_categories (
  id TEXT PRIMARY KEY, tenant_id TEXT NOT NULL REFERENCES tenants(id), name TEXT NOT NULL, active INTEGER NOT NULL DEFAULT 1, UNIQUE(tenant_id,name)
);
CREATE TABLE IF NOT EXISTS expenses (
  id TEXT PRIMARY KEY, tenant_id TEXT NOT NULL REFERENCES tenants(id), branch_id TEXT REFERENCES branches(id), category_id TEXT NOT NULL REFERENCES expense_categories(id),
  status TEXT NOT NULL CHECK(status IN ('DRAFT','SUBMITTED','APPROVED','REJECTED','PAID')), description TEXT NOT NULL, amount_fils INTEGER NOT NULL,
  tax_fils INTEGER NOT NULL DEFAULT 0, payment_method TEXT, supplier_id TEXT REFERENCES suppliers(id), incurred_on TEXT NOT NULL,
  created_by_user_id TEXT NOT NULL REFERENCES users(id), approved_by_user_id TEXT REFERENCES users(id), approval_note TEXT, paid_at TEXT, created_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS petty_cash_funds (
  id TEXT PRIMARY KEY, tenant_id TEXT NOT NULL REFERENCES tenants(id), branch_id TEXT NOT NULL REFERENCES branches(id), name TEXT NOT NULL,
  custodian_user_id TEXT REFERENCES users(id), active INTEGER NOT NULL DEFAULT 1, UNIQUE(tenant_id,branch_id,name)
);
CREATE TABLE IF NOT EXISTS petty_cash_entries (
  id TEXT PRIMARY KEY, tenant_id TEXT NOT NULL REFERENCES tenants(id), fund_id TEXT NOT NULL REFERENCES petty_cash_funds(id), operation_id TEXT NOT NULL,
  entry_type TEXT NOT NULL CHECK(entry_type IN ('OPENING','EXPENSE','REPLENISHMENT','ADJUSTMENT','CLOSE')), amount_fils INTEGER NOT NULL,
  expense_id TEXT REFERENCES expenses(id), user_id TEXT NOT NULL REFERENCES users(id), device_id TEXT REFERENCES devices(id), note TEXT, created_at TEXT NOT NULL,
  UNIQUE(tenant_id,operation_id)
);
CREATE TABLE IF NOT EXISTS cash_variance_cases (
  id TEXT PRIMARY KEY, tenant_id TEXT NOT NULL REFERENCES tenants(id), branch_id TEXT NOT NULL REFERENCES branches(id), cash_session_id TEXT NOT NULL REFERENCES cash_sessions(id),
  expected_fils INTEGER NOT NULL, counted_fils INTEGER NOT NULL, variance_fils INTEGER NOT NULL, severity TEXT NOT NULL,
  status TEXT NOT NULL CHECK(status IN ('OPEN','ACKNOWLEDGED','INVESTIGATING','RESOLVED','ESCALATED')), assigned_user_id TEXT REFERENCES users(id),
  notes TEXT, created_at TEXT NOT NULL, resolved_at TEXT
);

-- Customers, loyalty, credit
CREATE TABLE IF NOT EXISTS customers (
  id TEXT PRIMARY KEY, tenant_id TEXT NOT NULL REFERENCES tenants(id), customer_no TEXT, name TEXT NOT NULL, phone_e164 TEXT, whatsapp_e164 TEXT,
  email TEXT, notes TEXT, active INTEGER NOT NULL DEFAULT 1, created_at TEXT NOT NULL, UNIQUE(tenant_id,customer_no)
);
CREATE INDEX IF NOT EXISTS ix_customers_phone ON customers(tenant_id,phone_e164);
CREATE TABLE IF NOT EXISTS customer_addresses (
  id TEXT PRIMARY KEY, tenant_id TEXT NOT NULL REFERENCES tenants(id), customer_id TEXT NOT NULL REFERENCES customers(id), label TEXT,
  governorate TEXT, area TEXT, block TEXT, road TEXT, building TEXT, flat_shop TEXT, landmark TEXT, directions TEXT, notes TEXT,
  is_default INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS loyalty_tiers (
  id TEXT PRIMARY KEY, tenant_id TEXT NOT NULL REFERENCES tenants(id), name TEXT NOT NULL, min_points INTEGER NOT NULL DEFAULT 0,
  earn_multiplier_bps INTEGER NOT NULL DEFAULT 10000, config_json TEXT NOT NULL DEFAULT '{}', active INTEGER NOT NULL DEFAULT 1, UNIQUE(tenant_id,name)
);
CREATE TABLE IF NOT EXISTS loyalty_accounts (
  tenant_id TEXT NOT NULL, customer_id TEXT PRIMARY KEY REFERENCES customers(id), tier_id TEXT REFERENCES loyalty_tiers(id), status TEXT NOT NULL DEFAULT 'ACTIVE', joined_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS loyalty_ledger (
  id TEXT PRIMARY KEY, tenant_id TEXT NOT NULL REFERENCES tenants(id), customer_id TEXT NOT NULL REFERENCES customers(id),
  event_type TEXT NOT NULL, points_delta INTEGER NOT NULL, source_type TEXT NOT NULL, source_id TEXT, expires_at TEXT, operation_id TEXT,
  created_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS ix_loyalty_ledger ON loyalty_ledger(tenant_id,customer_id,created_at,id);
CREATE TABLE IF NOT EXISTS customer_credit_accounts (
  tenant_id TEXT NOT NULL, customer_id TEXT PRIMARY KEY REFERENCES customers(id), credit_limit_fils INTEGER NOT NULL DEFAULT 0,
  payment_terms_days INTEGER, status TEXT NOT NULL CHECK(status IN ('ACTIVE','SUSPENDED','CLOSED')), opened_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS customer_credit_ledger (
  id TEXT PRIMARY KEY, tenant_id TEXT NOT NULL REFERENCES tenants(id), customer_id TEXT NOT NULL REFERENCES customers(id),
  event_type TEXT NOT NULL, source_type TEXT NOT NULL, source_id TEXT, debit_fils INTEGER NOT NULL DEFAULT 0, credit_fils INTEGER NOT NULL DEFAULT 0,
  due_date TEXT, operation_id TEXT, created_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS ix_customer_credit ON customer_credit_ledger(tenant_id,customer_id,created_at,id);
CREATE TABLE IF NOT EXISTS customer_credit_payments (
  id TEXT PRIMARY KEY, tenant_id TEXT NOT NULL REFERENCES tenants(id), customer_id TEXT NOT NULL REFERENCES customers(id), branch_id TEXT NOT NULL REFERENCES branches(id),
  operation_id TEXT NOT NULL, device_id TEXT NOT NULL REFERENCES devices(id), user_id TEXT NOT NULL REFERENCES users(id), method TEXT NOT NULL,
  amount_fils INTEGER NOT NULL, reference TEXT, received_at TEXT NOT NULL, UNIQUE(tenant_id,operation_id)
);

-- Delivery and channel orders
CREATE TABLE IF NOT EXISTS delivery_workers (
  id TEXT PRIMARY KEY, tenant_id TEXT NOT NULL REFERENCES tenants(id), employee_id TEXT, name TEXT NOT NULL, phone_e164 TEXT,
  branch_id TEXT REFERENCES branches(id), active INTEGER NOT NULL DEFAULT 1
);
CREATE TABLE IF NOT EXISTS delivery_orders (
  id TEXT PRIMARY KEY, tenant_id TEXT NOT NULL REFERENCES tenants(id), branch_id TEXT NOT NULL REFERENCES branches(id), sale_id TEXT REFERENCES sales(id),
  customer_id TEXT REFERENCES customers(id), address_id TEXT REFERENCES customer_addresses(id), phone_e164 TEXT, amount_due_fils INTEGER NOT NULL DEFAULT 0,
  payment_state TEXT NOT NULL, assigned_worker_id TEXT REFERENCES delivery_workers(id), status TEXT NOT NULL,
  notes TEXT, created_at TEXT NOT NULL, dispatched_at TEXT, delivered_at TEXT
);
CREATE TABLE IF NOT EXISTS delivery_events (
  id TEXT PRIMARY KEY, tenant_id TEXT NOT NULL REFERENCES tenants(id), delivery_id TEXT NOT NULL REFERENCES delivery_orders(id),
  event_type TEXT NOT NULL, amount_fils INTEGER, payment_method TEXT, reference TEXT, worker_id TEXT REFERENCES delivery_workers(id),
  device_id TEXT REFERENCES devices(id), user_id TEXT REFERENCES users(id), created_at TEXT NOT NULL, payload_json TEXT NOT NULL DEFAULT '{}'
);
CREATE TABLE IF NOT EXISTS courier_settlements (
  id TEXT PRIMARY KEY, tenant_id TEXT NOT NULL REFERENCES tenants(id), branch_id TEXT NOT NULL REFERENCES branches(id), worker_id TEXT NOT NULL REFERENCES delivery_workers(id),
  expected_cash_fils INTEGER NOT NULL, returned_cash_fils INTEGER NOT NULL, variance_fils INTEGER NOT NULL, status TEXT NOT NULL,
  settled_by_user_id TEXT NOT NULL REFERENCES users(id), device_id TEXT NOT NULL REFERENCES devices(id), settled_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS sales_channels (
  id TEXT PRIMARY KEY, tenant_id TEXT NOT NULL REFERENCES tenants(id), code TEXT NOT NULL, name TEXT NOT NULL,
  channel_type TEXT NOT NULL, active INTEGER NOT NULL DEFAULT 1, config_json TEXT NOT NULL DEFAULT '{}', UNIQUE(tenant_id,code)
);
CREATE TABLE IF NOT EXISTS digital_orders (
  id TEXT PRIMARY KEY, tenant_id TEXT NOT NULL REFERENCES tenants(id), branch_id TEXT NOT NULL REFERENCES branches(id), channel_id TEXT NOT NULL REFERENCES sales_channels(id),
  external_order_ref TEXT, customer_id TEXT REFERENCES customers(id), status TEXT NOT NULL, gross_fils INTEGER NOT NULL DEFAULT 0,
  commission_fils INTEGER NOT NULL DEFAULT 0, marketplace_fee_fils INTEGER NOT NULL DEFAULT 0, merchant_discount_fils INTEGER NOT NULL DEFAULT 0,
  platform_discount_fils INTEGER NOT NULL DEFAULT 0, delivery_fee_fils INTEGER NOT NULL DEFAULT 0, expected_settlement_fils INTEGER NOT NULL DEFAULT 0,
  created_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS digital_order_lines (
  id TEXT PRIMARY KEY, order_id TEXT NOT NULL REFERENCES digital_orders(id), product_id TEXT REFERENCES products(id), description_snapshot TEXT NOT NULL,
  quantity_milli INTEGER NOT NULL, unit_price_fils INTEGER NOT NULL, tax_fils INTEGER NOT NULL, line_total_fils INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS marketplace_settlements (
  id TEXT PRIMARY KEY, tenant_id TEXT NOT NULL REFERENCES tenants(id), channel_id TEXT NOT NULL REFERENCES sales_channels(id),
  period_start TEXT NOT NULL, period_end TEXT NOT NULL, expected_fils INTEGER NOT NULL, actual_fils INTEGER,
  variance_fils INTEGER, status TEXT NOT NULL, reference TEXT, created_at TEXT NOT NULL
);

-- Production and waste
CREATE TABLE IF NOT EXISTS recipes (
  id TEXT PRIMARY KEY, tenant_id TEXT NOT NULL REFERENCES tenants(id), output_product_id TEXT NOT NULL REFERENCES products(id),
  output_qty_milli INTEGER NOT NULL, active INTEGER NOT NULL DEFAULT 1, version INTEGER NOT NULL DEFAULT 1, created_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS recipe_components (
  recipe_id TEXT NOT NULL REFERENCES recipes(id), component_product_id TEXT NOT NULL REFERENCES products(id), quantity_milli INTEGER NOT NULL,
  PRIMARY KEY(recipe_id,component_product_id)
);
CREATE TABLE IF NOT EXISTS production_orders (
  id TEXT PRIMARY KEY, tenant_id TEXT NOT NULL REFERENCES tenants(id), branch_id TEXT NOT NULL REFERENCES branches(id), recipe_id TEXT NOT NULL REFERENCES recipes(id),
  centre_id TEXT NOT NULL REFERENCES inventory_centres(id), status TEXT NOT NULL CHECK(status IN ('DRAFT','PLANNED','IN_PROGRESS','COMPLETED','CANCELLED')),
  planned_output_milli INTEGER NOT NULL, actual_output_milli INTEGER, operation_id TEXT, device_id TEXT REFERENCES devices(id),
  created_by_user_id TEXT NOT NULL REFERENCES users(id), started_at TEXT, completed_at TEXT, created_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS production_usage (
  id TEXT PRIMARY KEY, production_order_id TEXT NOT NULL REFERENCES production_orders(id), product_id TEXT NOT NULL REFERENCES products(id),
  expected_qty_milli INTEGER NOT NULL, actual_qty_milli INTEGER, lot_id TEXT REFERENCES inventory_lots(id), cost_fils INTEGER
);
CREATE TABLE IF NOT EXISTS waste_events (
  id TEXT PRIMARY KEY, tenant_id TEXT NOT NULL REFERENCES tenants(id), branch_id TEXT NOT NULL REFERENCES branches(id), centre_id TEXT NOT NULL REFERENCES inventory_centres(id),
  product_id TEXT NOT NULL REFERENCES products(id), lot_id TEXT REFERENCES inventory_lots(id), waste_type TEXT NOT NULL,
  quantity_milli INTEGER NOT NULL, cost_value_fils INTEGER NOT NULL, reason TEXT, responsible_user_id TEXT REFERENCES users(id),
  operation_id TEXT NOT NULL, device_id TEXT NOT NULL REFERENCES devices(id), created_at TEXT NOT NULL, UNIQUE(tenant_id,operation_id,product_id,lot_id)
);

-- Employees and attendance
CREATE TABLE IF NOT EXISTS employees (
  id TEXT PRIMARY KEY, tenant_id TEXT NOT NULL REFERENCES tenants(id), employee_no TEXT NOT NULL, name TEXT NOT NULL, job_title TEXT,
  phone_e164 TEXT, hire_date TEXT, status TEXT NOT NULL DEFAULT 'ACTIVE', emergency_contact_json TEXT, notes TEXT, UNIQUE(tenant_id,employee_no)
);
CREATE TABLE IF NOT EXISTS employee_branches (employee_id TEXT NOT NULL REFERENCES employees(id), branch_id TEXT NOT NULL REFERENCES branches(id), PRIMARY KEY(employee_id,branch_id));
CREATE TABLE IF NOT EXISTS attendance_events (
  id TEXT PRIMARY KEY, tenant_id TEXT NOT NULL REFERENCES tenants(id), employee_id TEXT NOT NULL REFERENCES employees(id), branch_id TEXT REFERENCES branches(id),
  event_type TEXT NOT NULL CHECK(event_type IN ('CLOCK_IN','CLOCK_OUT','BREAK_START','BREAK_END','CORRECTION')), occurred_at TEXT NOT NULL,
  device_id TEXT REFERENCES devices(id), entered_by_user_id TEXT REFERENCES users(id), note TEXT
);

-- WhatsApp, OCR evidence, digital documents
CREATE TABLE IF NOT EXISTS whatsapp_accounts (
  id TEXT PRIMARY KEY, tenant_id TEXT NOT NULL REFERENCES tenants(id), branch_id TEXT REFERENCES branches(id), label TEXT NOT NULL,
  state TEXT NOT NULL, session_key_ref TEXT, last_connected_at TEXT, last_error TEXT, created_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS whatsapp_conversations (
  id TEXT PRIMARY KEY, tenant_id TEXT NOT NULL REFERENCES tenants(id), account_id TEXT NOT NULL REFERENCES whatsapp_accounts(id),
  external_chat_id TEXT NOT NULL, customer_id TEXT REFERENCES customers(id), phone_e164 TEXT, last_message_at TEXT, UNIQUE(account_id,external_chat_id)
);
CREATE TABLE IF NOT EXISTS whatsapp_messages (
  id TEXT PRIMARY KEY, tenant_id TEXT NOT NULL REFERENCES tenants(id), conversation_id TEXT NOT NULL REFERENCES whatsapp_conversations(id),
  external_message_id TEXT, direction TEXT NOT NULL, message_type TEXT NOT NULL, text_body TEXT, media_document_id TEXT,
  sent_at TEXT NOT NULL, delivery_state TEXT, UNIQUE(conversation_id,external_message_id)
);
CREATE TABLE IF NOT EXISTS payment_evidence (
  id TEXT PRIMARY KEY, tenant_id TEXT NOT NULL REFERENCES tenants(id), branch_id TEXT REFERENCES branches(id), customer_id TEXT REFERENCES customers(id),
  expected_amount_fils INTEGER, extracted_amount_fils INTEGER, extracted_reference TEXT, extracted_paid_at TEXT, payer_hint TEXT,
  source_document_id TEXT, match_state TEXT NOT NULL CHECK(match_state IN ('MATCH','MISMATCH','REVIEW','UNREADABLE')),
  settlement_authority TEXT NOT NULL DEFAULT 'EVIDENCE_ONLY', reviewed_by_user_id TEXT REFERENCES users(id), created_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS business_documents (
  id TEXT PRIMARY KEY, tenant_id TEXT NOT NULL REFERENCES tenants(id), scope_type TEXT NOT NULL, scope_id TEXT, title TEXT NOT NULL,
  document_type TEXT NOT NULL, owner_user_id TEXT REFERENCES users(id), active INTEGER NOT NULL DEFAULT 1, created_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS document_versions (
  id TEXT PRIMARY KEY, document_id TEXT NOT NULL REFERENCES business_documents(id), version_no INTEGER NOT NULL, storage_path TEXT NOT NULL,
  sha256 TEXT NOT NULL, mime_type TEXT, byte_size INTEGER, indexed_for_ai INTEGER NOT NULL DEFAULT 0, uploaded_by_user_id TEXT REFERENCES users(id),
  created_at TEXT NOT NULL, UNIQUE(document_id,version_no)
);
CREATE TABLE IF NOT EXISTS ocr_jobs (
  id TEXT PRIMARY KEY, tenant_id TEXT NOT NULL REFERENCES tenants(id), document_version_id TEXT NOT NULL REFERENCES document_versions(id),
  job_type TEXT NOT NULL, state TEXT NOT NULL, extracted_json TEXT, confidence_json TEXT, review_state TEXT NOT NULL DEFAULT 'PENDING',
  reviewed_by_user_id TEXT REFERENCES users(id), created_at TEXT NOT NULL, completed_at TEXT
);

-- AI deterministic action execution
CREATE TABLE IF NOT EXISTS ai_action_runs (
  id TEXT PRIMARY KEY, tenant_id TEXT NOT NULL REFERENCES tenants(id), user_id TEXT NOT NULL REFERENCES users(id), device_id TEXT REFERENCES devices(id),
  provider TEXT, model TEXT, tool_name TEXT NOT NULL, risk_class TEXT NOT NULL, state TEXT NOT NULL,
  request_json TEXT NOT NULL, preview_json TEXT, confirmation_required INTEGER NOT NULL DEFAULT 1, confirmed_at TEXT,
  operation_id TEXT, created_at TEXT NOT NULL, completed_at TEXT
);
CREATE TABLE IF NOT EXISTS ai_action_steps (
  id TEXT PRIMARY KEY, run_id TEXT NOT NULL REFERENCES ai_action_runs(id), step_no INTEGER NOT NULL, step_type TEXT NOT NULL,
  input_json TEXT NOT NULL, output_json TEXT, state TEXT NOT NULL, error_text TEXT, created_at TEXT NOT NULL, UNIQUE(run_id,step_no)
);
CREATE TABLE IF NOT EXISTS ai_undo_records (
  id TEXT PRIMARY KEY, tenant_id TEXT NOT NULL REFERENCES tenants(id), run_id TEXT NOT NULL REFERENCES ai_action_runs(id),
  action_type TEXT NOT NULL, entity_type TEXT NOT NULL, entity_id TEXT NOT NULL, before_json TEXT, after_json TEXT,
  undo_state TEXT NOT NULL DEFAULT 'AVAILABLE', expires_at TEXT, created_at TEXT NOT NULL
);

-- Alerts, jobs, settings, backups, release diagnostics
CREATE TABLE IF NOT EXISTS operational_alerts (
  id TEXT PRIMARY KEY, tenant_id TEXT NOT NULL REFERENCES tenants(id), branch_id TEXT REFERENCES branches(id), severity TEXT NOT NULL,
  alert_type TEXT NOT NULL, status TEXT NOT NULL CHECK(status IN ('NEW','ACKNOWLEDGED','IN_PROGRESS','RESOLVED','DISMISSED')),
  entity_type TEXT, entity_id TEXT, title TEXT NOT NULL, details_json TEXT NOT NULL DEFAULT '{}', assigned_user_id TEXT REFERENCES users(id),
  created_at TEXT NOT NULL, resolved_at TEXT
);
CREATE TABLE IF NOT EXISTS alert_events (
  id TEXT PRIMARY KEY, alert_id TEXT NOT NULL REFERENCES operational_alerts(id), event_type TEXT NOT NULL,
  user_id TEXT REFERENCES users(id), note TEXT, created_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS background_jobs (
  id TEXT PRIMARY KEY, tenant_id TEXT NOT NULL REFERENCES tenants(id), branch_id TEXT REFERENCES branches(id), job_type TEXT NOT NULL,
  state TEXT NOT NULL CHECK(state IN ('QUEUED','RUNNING','SUCCEEDED','FAILED','CANCELLED','REQUIRES_REVIEW')),
  progress_current INTEGER NOT NULL DEFAULT 0, progress_total INTEGER, cancellable INTEGER NOT NULL DEFAULT 1,
  payload_json TEXT NOT NULL DEFAULT '{}', result_json TEXT, error_text TEXT, created_by_user_id TEXT REFERENCES users(id), created_at TEXT NOT NULL, updated_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS feature_flags (
  tenant_id TEXT NOT NULL REFERENCES tenants(id), branch_id TEXT REFERENCES branches(id), flag_key TEXT NOT NULL,
  enabled INTEGER NOT NULL, config_json TEXT NOT NULL DEFAULT '{}', updated_by_user_id TEXT REFERENCES users(id), updated_at TEXT NOT NULL,
  PRIMARY KEY(tenant_id,branch_id,flag_key)
);
CREATE TABLE IF NOT EXISTS system_settings (
  tenant_id TEXT NOT NULL REFERENCES tenants(id), branch_id TEXT REFERENCES branches(id), setting_key TEXT NOT NULL,
  value_json TEXT NOT NULL, secret_ref TEXT, updated_by_user_id TEXT REFERENCES users(id), updated_at TEXT NOT NULL,
  PRIMARY KEY(tenant_id,branch_id,setting_key)
);
CREATE TABLE IF NOT EXISTS backup_records (
  id TEXT PRIMARY KEY, tenant_id TEXT NOT NULL REFERENCES tenants(id), branch_id TEXT REFERENCES branches(id), backup_type TEXT NOT NULL,
  storage_path TEXT NOT NULL, sha256 TEXT NOT NULL, byte_size INTEGER, schema_version TEXT NOT NULL, app_version TEXT NOT NULL,
  state TEXT NOT NULL, integrity_state TEXT, created_at TEXT NOT NULL, verified_at TEXT
);
CREATE TABLE IF NOT EXISTS restore_runs (
  id TEXT PRIMARY KEY, tenant_id TEXT NOT NULL REFERENCES tenants(id), backup_id TEXT NOT NULL REFERENCES backup_records(id),
  pre_restore_backup_id TEXT REFERENCES backup_records(id), state TEXT NOT NULL, compatibility_json TEXT NOT NULL,
  authorized_by_user_id TEXT NOT NULL REFERENCES users(id), started_at TEXT NOT NULL, completed_at TEXT, result_json TEXT
);
CREATE TABLE IF NOT EXISTS diagnostics_snapshots (
  id TEXT PRIMARY KEY, tenant_id TEXT NOT NULL REFERENCES tenants(id), branch_id TEXT REFERENCES branches(id), device_id TEXT REFERENCES devices(id),
  app_version TEXT NOT NULL, build_sha TEXT, schema_version TEXT NOT NULL, payload_json TEXT NOT NULL, created_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS update_records (
  id TEXT PRIMARY KEY, tenant_id TEXT NOT NULL REFERENCES tenants(id), device_id TEXT REFERENCES devices(id), from_version TEXT, to_version TEXT NOT NULL,
  manifest_sha256 TEXT NOT NULL, signature_state TEXT NOT NULL, state TEXT NOT NULL, created_at TEXT NOT NULL, completed_at TEXT, error_text TEXT
);

-- Printing, voids, refund payment effects
CREATE TABLE IF NOT EXISTS print_jobs (
  id TEXT PRIMARY KEY, tenant_id TEXT NOT NULL REFERENCES tenants(id), branch_id TEXT NOT NULL REFERENCES branches(id), device_id TEXT NOT NULL REFERENCES devices(id),
  sale_id TEXT REFERENCES sales(id), document_type TEXT NOT NULL, snapshot_sha256 TEXT, printer_target TEXT,
  state TEXT NOT NULL CHECK(state IN ('PENDING','PRINTING','PRINTED','FAILED','CANCELLED')), attempts INTEGER NOT NULL DEFAULT 0,
  last_error TEXT, created_at TEXT NOT NULL, updated_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS sale_voids (
  id TEXT PRIMARY KEY, tenant_id TEXT NOT NULL REFERENCES tenants(id), branch_id TEXT NOT NULL REFERENCES branches(id), sale_id TEXT NOT NULL REFERENCES sales(id),
  operation_id TEXT NOT NULL, device_id TEXT NOT NULL REFERENCES devices(id), user_id TEXT NOT NULL REFERENCES users(id), approval_ref TEXT NOT NULL,
  reason TEXT NOT NULL, reversed_total_fils INTEGER NOT NULL, created_at TEXT NOT NULL, UNIQUE(tenant_id,operation_id), UNIQUE(sale_id)
);
CREATE TABLE IF NOT EXISTS refund_payments (
  id TEXT PRIMARY KEY, refund_id TEXT NOT NULL REFERENCES refunds(id), tender_kind TEXT NOT NULL, amount_fils INTEGER NOT NULL,
  reference TEXT, created_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS manager_approval_consumptions (
  id TEXT PRIMARY KEY,
  tenant_id TEXT NOT NULL REFERENCES tenants(id),
  approver_user_id TEXT NOT NULL REFERENCES users(id),
  device_id TEXT NOT NULL REFERENCES devices(id),
  operation_id TEXT NOT NULL,
  action TEXT NOT NULL,
  entity_id TEXT NOT NULL,
  payload_sha256 TEXT NOT NULL,
  nonce TEXT NOT NULL,
  signature_sha256 TEXT NOT NULL,
  expires_at TEXT NOT NULL,
  consumed_at TEXT NOT NULL,
  UNIQUE(tenant_id,nonce),
  UNIQUE(tenant_id,operation_id,action,entity_id,nonce)
);

CREATE UNIQUE INDEX IF NOT EXISTS ux_feature_flags_scope ON feature_flags(tenant_id, IFNULL(branch_id,''), flag_key);
CREATE UNIQUE INDEX IF NOT EXISTS ux_system_settings_scope ON system_settings(tenant_id, IFNULL(branch_id,''), setting_key);
CREATE UNIQUE INDEX IF NOT EXISTS ux_user_role_scope ON user_roles(user_id, role_id, IFNULL(branch_id,''));
