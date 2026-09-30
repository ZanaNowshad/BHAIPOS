PRAGMA foreign_keys = ON;

-- Cross-tenant guardrails for authoritative core records.
CREATE TRIGGER IF NOT EXISTS guard_devices_insert BEFORE INSERT ON devices
WHEN (SELECT tenant_id FROM branches WHERE id=NEW.branch_id) IS NOT NEW.tenant_id
BEGIN SELECT RAISE(ABORT,'TENANT_SCOPE_VIOLATION:devices.branch'); END;
CREATE TRIGGER IF NOT EXISTS guard_devices_update BEFORE UPDATE OF tenant_id,branch_id ON devices
WHEN (SELECT tenant_id FROM branches WHERE id=NEW.branch_id) IS NOT NEW.tenant_id
BEGIN SELECT RAISE(ABORT,'TENANT_SCOPE_VIOLATION:devices.branch'); END;

CREATE TRIGGER IF NOT EXISTS guard_registers_insert BEFORE INSERT ON registers
WHEN (SELECT tenant_id FROM branches WHERE id=NEW.branch_id) IS NOT NEW.tenant_id
BEGIN SELECT RAISE(ABORT,'TENANT_SCOPE_VIOLATION:registers.branch'); END;
CREATE TRIGGER IF NOT EXISTS guard_inventory_centres_insert BEFORE INSERT ON inventory_centres
WHEN (SELECT tenant_id FROM branches WHERE id=NEW.branch_id) IS NOT NEW.tenant_id
BEGIN SELECT RAISE(ABORT,'TENANT_SCOPE_VIOLATION:inventory_centres.branch'); END;

CREATE TRIGGER IF NOT EXISTS guard_user_roles_insert BEFORE INSERT ON user_roles
WHEN (SELECT tenant_id FROM roles WHERE id=NEW.role_id) != (SELECT tenant_id FROM users WHERE id=NEW.user_id)
  OR (NEW.branch_id IS NOT NULL AND (SELECT tenant_id FROM branches WHERE id=NEW.branch_id) != (SELECT tenant_id FROM users WHERE id=NEW.user_id))
BEGIN SELECT RAISE(ABORT,'TENANT_SCOPE_VIOLATION:user_roles'); END;

CREATE TRIGGER IF NOT EXISTS guard_product_barcodes_insert BEFORE INSERT ON product_barcodes
WHEN (SELECT tenant_id FROM products WHERE id=NEW.product_id) IS NOT NEW.tenant_id
BEGIN SELECT RAISE(ABORT,'TENANT_SCOPE_VIOLATION:product_barcodes.product'); END;
CREATE TRIGGER IF NOT EXISTS guard_branch_assortments_insert BEFORE INSERT ON branch_assortments
WHEN (SELECT tenant_id FROM products WHERE id=NEW.product_id) IS NOT NEW.tenant_id
  OR (SELECT tenant_id FROM branches WHERE id=NEW.branch_id) IS NOT NEW.tenant_id
BEGIN SELECT RAISE(ABORT,'TENANT_SCOPE_VIOLATION:branch_assortments'); END;
CREATE TRIGGER IF NOT EXISTS guard_price_history_insert BEFORE INSERT ON price_history
WHEN (SELECT tenant_id FROM products WHERE id=NEW.product_id) IS NOT NEW.tenant_id
  OR (NEW.branch_id IS NOT NULL AND (SELECT tenant_id FROM branches WHERE id=NEW.branch_id) IS NOT NEW.tenant_id)
BEGIN SELECT RAISE(ABORT,'TENANT_SCOPE_VIOLATION:price_history'); END;

CREATE TRIGGER IF NOT EXISTS guard_cash_sessions_insert BEFORE INSERT ON cash_sessions
WHEN (SELECT tenant_id FROM branches WHERE id=NEW.branch_id) IS NOT NEW.tenant_id
  OR (SELECT tenant_id FROM registers WHERE id=NEW.register_id) IS NOT NEW.tenant_id
  OR (SELECT branch_id FROM registers WHERE id=NEW.register_id) IS NOT NEW.branch_id
  OR (SELECT tenant_id FROM users WHERE id=NEW.opened_by_user_id) IS NOT NEW.tenant_id
  OR (SELECT tenant_id FROM devices WHERE id=NEW.opened_on_device_id) IS NOT NEW.tenant_id
  OR (SELECT branch_id FROM devices WHERE id=NEW.opened_on_device_id) IS NOT NEW.branch_id
BEGIN SELECT RAISE(ABORT,'TENANT_SCOPE_VIOLATION:cash_sessions'); END;

CREATE TRIGGER IF NOT EXISTS guard_stock_levels_insert BEFORE INSERT ON stock_levels
WHEN (SELECT tenant_id FROM branches WHERE id=NEW.branch_id) IS NOT NEW.tenant_id
  OR (SELECT tenant_id FROM inventory_centres WHERE id=NEW.centre_id) IS NOT NEW.tenant_id
  OR (SELECT branch_id FROM inventory_centres WHERE id=NEW.centre_id) IS NOT NEW.branch_id
  OR (SELECT tenant_id FROM products WHERE id=NEW.product_id) IS NOT NEW.tenant_id
BEGIN SELECT RAISE(ABORT,'TENANT_SCOPE_VIOLATION:stock_levels'); END;
CREATE TRIGGER IF NOT EXISTS guard_stock_levels_update BEFORE UPDATE OF tenant_id,branch_id,centre_id,product_id ON stock_levels
WHEN (SELECT tenant_id FROM branches WHERE id=NEW.branch_id) IS NOT NEW.tenant_id
  OR (SELECT tenant_id FROM inventory_centres WHERE id=NEW.centre_id) IS NOT NEW.tenant_id
  OR (SELECT branch_id FROM inventory_centres WHERE id=NEW.centre_id) IS NOT NEW.branch_id
  OR (SELECT tenant_id FROM products WHERE id=NEW.product_id) IS NOT NEW.tenant_id
BEGIN SELECT RAISE(ABORT,'TENANT_SCOPE_VIOLATION:stock_levels'); END;

CREATE TRIGGER IF NOT EXISTS guard_inventory_movements_insert BEFORE INSERT ON inventory_movements
WHEN (SELECT tenant_id FROM branches WHERE id=NEW.branch_id) IS NOT NEW.tenant_id
  OR (SELECT tenant_id FROM inventory_centres WHERE id=NEW.centre_id) IS NOT NEW.tenant_id
  OR (SELECT branch_id FROM inventory_centres WHERE id=NEW.centre_id) IS NOT NEW.branch_id
  OR (SELECT tenant_id FROM products WHERE id=NEW.product_id) IS NOT NEW.tenant_id
  OR (SELECT tenant_id FROM devices WHERE id=NEW.device_id) IS NOT NEW.tenant_id
  OR (SELECT branch_id FROM devices WHERE id=NEW.device_id) IS NOT NEW.branch_id
  OR (SELECT tenant_id FROM users WHERE id=NEW.user_id) IS NOT NEW.tenant_id
BEGIN SELECT RAISE(ABORT,'TENANT_SCOPE_VIOLATION:inventory_movements'); END;

CREATE TRIGGER IF NOT EXISTS guard_carts_insert BEFORE INSERT ON carts
WHEN (SELECT tenant_id FROM branches WHERE id=NEW.branch_id) IS NOT NEW.tenant_id
  OR (SELECT tenant_id FROM devices WHERE id=NEW.device_id) IS NOT NEW.tenant_id
  OR (SELECT branch_id FROM devices WHERE id=NEW.device_id) IS NOT NEW.branch_id
  OR (SELECT tenant_id FROM users WHERE id=NEW.cashier_user_id) IS NOT NEW.tenant_id
BEGIN SELECT RAISE(ABORT,'TENANT_SCOPE_VIOLATION:carts'); END;
CREATE TRIGGER IF NOT EXISTS guard_cart_lines_insert BEFORE INSERT ON cart_lines
WHEN (SELECT tenant_id FROM products WHERE id=NEW.product_id) != (SELECT tenant_id FROM carts WHERE id=NEW.cart_id)
BEGIN SELECT RAISE(ABORT,'TENANT_SCOPE_VIOLATION:cart_lines'); END;

CREATE TRIGGER IF NOT EXISTS guard_sales_insert BEFORE INSERT ON sales
WHEN (SELECT tenant_id FROM branches WHERE id=NEW.branch_id) IS NOT NEW.tenant_id
  OR (SELECT tenant_id FROM devices WHERE id=NEW.device_id) IS NOT NEW.tenant_id
  OR (SELECT branch_id FROM devices WHERE id=NEW.device_id) IS NOT NEW.branch_id
  OR (SELECT tenant_id FROM registers WHERE id=NEW.register_id) IS NOT NEW.tenant_id
  OR (SELECT branch_id FROM registers WHERE id=NEW.register_id) IS NOT NEW.branch_id
  OR (SELECT tenant_id FROM carts WHERE id=NEW.cart_id) IS NOT NEW.tenant_id
  OR (SELECT branch_id FROM carts WHERE id=NEW.cart_id) IS NOT NEW.branch_id
  OR (SELECT tenant_id FROM users WHERE id=NEW.cashier_user_id) IS NOT NEW.tenant_id
  OR (NEW.cash_session_id IS NOT NULL AND (SELECT tenant_id FROM cash_sessions WHERE id=NEW.cash_session_id) IS NOT NEW.tenant_id)
  OR (NEW.cash_session_id IS NOT NULL AND (SELECT branch_id FROM cash_sessions WHERE id=NEW.cash_session_id) IS NOT NEW.branch_id)
BEGIN SELECT RAISE(ABORT,'TENANT_SCOPE_VIOLATION:sales'); END;
CREATE TRIGGER IF NOT EXISTS guard_sale_lines_insert BEFORE INSERT ON sale_lines
WHEN (SELECT tenant_id FROM products WHERE id=NEW.product_id) != (SELECT tenant_id FROM sales WHERE id=NEW.sale_id)
BEGIN SELECT RAISE(ABORT,'TENANT_SCOPE_VIOLATION:sale_lines'); END;

CREATE TRIGGER IF NOT EXISTS guard_refunds_insert BEFORE INSERT ON refunds
WHEN (SELECT tenant_id FROM branches WHERE id=NEW.branch_id) IS NOT NEW.tenant_id
  OR (SELECT tenant_id FROM sales WHERE id=NEW.sale_id) IS NOT NEW.tenant_id
  OR (SELECT branch_id FROM sales WHERE id=NEW.sale_id) IS NOT NEW.branch_id
  OR (SELECT tenant_id FROM devices WHERE id=NEW.device_id) IS NOT NEW.tenant_id
  OR (SELECT branch_id FROM devices WHERE id=NEW.device_id) IS NOT NEW.branch_id
  OR (SELECT tenant_id FROM users WHERE id=NEW.user_id) IS NOT NEW.tenant_id
BEGIN SELECT RAISE(ABORT,'TENANT_SCOPE_VIOLATION:refunds'); END;
CREATE TRIGGER IF NOT EXISTS guard_refund_lines_insert BEFORE INSERT ON refund_lines
WHEN (SELECT sale_id FROM sale_lines WHERE id=NEW.sale_line_id) != (SELECT sale_id FROM refunds WHERE id=NEW.refund_id)
BEGIN SELECT RAISE(ABORT,'REFUND_LINE_SALE_MISMATCH'); END;

CREATE TRIGGER IF NOT EXISTS guard_sync_queue_insert BEFORE INSERT ON sync_queue
WHEN (SELECT tenant_id FROM branches WHERE id=NEW.branch_id) IS NOT NEW.tenant_id
  OR (SELECT tenant_id FROM devices WHERE id=NEW.device_id) IS NOT NEW.tenant_id
  OR (SELECT branch_id FROM devices WHERE id=NEW.device_id) IS NOT NEW.branch_id
BEGIN SELECT RAISE(ABORT,'TENANT_SCOPE_VIOLATION:sync_queue'); END;
CREATE TRIGGER IF NOT EXISTS guard_audit_events_insert BEFORE INSERT ON audit_events
WHEN (SELECT tenant_id FROM devices WHERE id=NEW.device_id) IS NOT NEW.tenant_id
  OR (SELECT tenant_id FROM users WHERE id=NEW.actor_user_id) IS NOT NEW.tenant_id
BEGIN SELECT RAISE(ABORT,'TENANT_SCOPE_VIOLATION:audit_events'); END;
CREATE TRIGGER IF NOT EXISTS guard_approval_insert BEFORE INSERT ON manager_approval_consumptions
WHEN (SELECT tenant_id FROM devices WHERE id=NEW.device_id) IS NOT NEW.tenant_id
  OR (SELECT tenant_id FROM users WHERE id=NEW.approver_user_id) IS NOT NEW.tenant_id
BEGIN SELECT RAISE(ABORT,'TENANT_SCOPE_VIOLATION:manager_approval'); END;

-- High-value retail OS scope guards.
CREATE TRIGGER IF NOT EXISTS guard_supplier_products_insert BEFORE INSERT ON supplier_products
WHEN (SELECT tenant_id FROM suppliers WHERE id=NEW.supplier_id) IS NOT NEW.tenant_id
  OR (SELECT tenant_id FROM products WHERE id=NEW.product_id) IS NOT NEW.tenant_id
BEGIN SELECT RAISE(ABORT,'TENANT_SCOPE_VIOLATION:supplier_products'); END;
CREATE TRIGGER IF NOT EXISTS guard_purchase_orders_insert BEFORE INSERT ON purchase_orders
WHEN (SELECT tenant_id FROM branches WHERE id=NEW.branch_id) IS NOT NEW.tenant_id
  OR (SELECT tenant_id FROM suppliers WHERE id=NEW.supplier_id) IS NOT NEW.tenant_id
  OR (SELECT tenant_id FROM users WHERE id=NEW.created_by_user_id) IS NOT NEW.tenant_id
BEGIN SELECT RAISE(ABORT,'TENANT_SCOPE_VIOLATION:purchase_orders'); END;
CREATE TRIGGER IF NOT EXISTS guard_goods_receipts_insert BEFORE INSERT ON goods_receipts
WHEN (SELECT tenant_id FROM branches WHERE id=NEW.branch_id) IS NOT NEW.tenant_id
  OR (SELECT tenant_id FROM suppliers WHERE id=NEW.supplier_id) IS NOT NEW.tenant_id
  OR (SELECT tenant_id FROM inventory_centres WHERE id=NEW.centre_id) IS NOT NEW.tenant_id
  OR (SELECT tenant_id FROM devices WHERE id=NEW.device_id) IS NOT NEW.tenant_id
  OR (SELECT tenant_id FROM users WHERE id=NEW.received_by_user_id) IS NOT NEW.tenant_id
BEGIN SELECT RAISE(ABORT,'TENANT_SCOPE_VIOLATION:goods_receipts'); END;
CREATE TRIGGER IF NOT EXISTS guard_supplier_payments_insert BEFORE INSERT ON supplier_payments
WHEN (SELECT tenant_id FROM branches WHERE id=NEW.branch_id) IS NOT NEW.tenant_id
  OR (SELECT tenant_id FROM suppliers WHERE id=NEW.supplier_id) IS NOT NEW.tenant_id
  OR (SELECT tenant_id FROM devices WHERE id=NEW.device_id) IS NOT NEW.tenant_id
  OR (SELECT tenant_id FROM users WHERE id=NEW.user_id) IS NOT NEW.tenant_id
BEGIN SELECT RAISE(ABORT,'TENANT_SCOPE_VIOLATION:supplier_payments'); END;
CREATE TRIGGER IF NOT EXISTS guard_customer_addresses_insert BEFORE INSERT ON customer_addresses
WHEN (SELECT tenant_id FROM customers WHERE id=NEW.customer_id) IS NOT NEW.tenant_id
BEGIN SELECT RAISE(ABORT,'TENANT_SCOPE_VIOLATION:customer_addresses'); END;
CREATE TRIGGER IF NOT EXISTS guard_customer_credit_payments_insert BEFORE INSERT ON customer_credit_payments
WHEN (SELECT tenant_id FROM customers WHERE id=NEW.customer_id) IS NOT NEW.tenant_id
  OR (SELECT tenant_id FROM branches WHERE id=NEW.branch_id) IS NOT NEW.tenant_id
  OR (SELECT tenant_id FROM devices WHERE id=NEW.device_id) IS NOT NEW.tenant_id
  OR (SELECT tenant_id FROM users WHERE id=NEW.user_id) IS NOT NEW.tenant_id
BEGIN SELECT RAISE(ABORT,'TENANT_SCOPE_VIOLATION:customer_credit_payments'); END;
CREATE TRIGGER IF NOT EXISTS guard_print_jobs_insert BEFORE INSERT ON print_jobs
WHEN (SELECT tenant_id FROM branches WHERE id=NEW.branch_id) IS NOT NEW.tenant_id
  OR (SELECT tenant_id FROM devices WHERE id=NEW.device_id) IS NOT NEW.tenant_id
  OR (NEW.sale_id IS NOT NULL AND (SELECT tenant_id FROM sales WHERE id=NEW.sale_id) IS NOT NEW.tenant_id)
BEGIN SELECT RAISE(ABORT,'TENANT_SCOPE_VIOLATION:print_jobs'); END;
CREATE TRIGGER IF NOT EXISTS guard_sale_voids_insert BEFORE INSERT ON sale_voids
WHEN (SELECT tenant_id FROM sales WHERE id=NEW.sale_id) IS NOT NEW.tenant_id
  OR (SELECT branch_id FROM sales WHERE id=NEW.sale_id) IS NOT NEW.branch_id
  OR (SELECT tenant_id FROM devices WHERE id=NEW.device_id) IS NOT NEW.tenant_id
  OR (SELECT tenant_id FROM users WHERE id=NEW.user_id) IS NOT NEW.tenant_id
BEGIN SELECT RAISE(ABORT,'TENANT_SCOPE_VIOLATION:sale_voids'); END;

-- Value-domain guards omitted from legacy table declarations are enforced here.
CREATE TRIGGER IF NOT EXISTS guard_sale_payment_amount BEFORE INSERT ON sale_payments
WHEN NEW.amount_fils <= 0 OR NEW.change_fils < 0
BEGIN SELECT RAISE(ABORT,'INVALID_PAYMENT_AMOUNT'); END;
CREATE TRIGGER IF NOT EXISTS guard_refund_line_qty BEFORE INSERT ON refund_lines
WHEN NEW.quantity_milli <= 0
BEGIN SELECT RAISE(ABORT,'INVALID_REFUND_QUANTITY'); END;
CREATE TRIGGER IF NOT EXISTS guard_inventory_movement_qty BEFORE INSERT ON inventory_movements
WHEN NEW.quantity_milli = 0
BEGIN SELECT RAISE(ABORT,'ZERO_INVENTORY_MOVEMENT'); END;

-- Append-only / immutable evidence. Corrections use compensating records.
CREATE TRIGGER IF NOT EXISTS immutable_inventory_movements_update BEFORE UPDATE ON inventory_movements BEGIN SELECT RAISE(ABORT,'IMMUTABLE:inventory_movements'); END;
CREATE TRIGGER IF NOT EXISTS immutable_inventory_movements_delete BEFORE DELETE ON inventory_movements BEGIN SELECT RAISE(ABORT,'IMMUTABLE:inventory_movements'); END;
CREATE TRIGGER IF NOT EXISTS immutable_sale_lines_update BEFORE UPDATE ON sale_lines BEGIN SELECT RAISE(ABORT,'IMMUTABLE:sale_lines'); END;
CREATE TRIGGER IF NOT EXISTS immutable_sale_lines_delete BEFORE DELETE ON sale_lines BEGIN SELECT RAISE(ABORT,'IMMUTABLE:sale_lines'); END;
CREATE TRIGGER IF NOT EXISTS immutable_sale_payments_update BEFORE UPDATE ON sale_payments BEGIN SELECT RAISE(ABORT,'IMMUTABLE:sale_payments'); END;
CREATE TRIGGER IF NOT EXISTS immutable_sale_payments_delete BEFORE DELETE ON sale_payments BEGIN SELECT RAISE(ABORT,'IMMUTABLE:sale_payments'); END;
CREATE TRIGGER IF NOT EXISTS immutable_receipt_snapshots_update BEFORE UPDATE ON receipt_snapshots BEGIN SELECT RAISE(ABORT,'IMMUTABLE:receipt_snapshots'); END;
CREATE TRIGGER IF NOT EXISTS immutable_receipt_snapshots_delete BEFORE DELETE ON receipt_snapshots BEGIN SELECT RAISE(ABORT,'IMMUTABLE:receipt_snapshots'); END;
CREATE TRIGGER IF NOT EXISTS immutable_refunds_update BEFORE UPDATE ON refunds BEGIN SELECT RAISE(ABORT,'IMMUTABLE:refunds'); END;
CREATE TRIGGER IF NOT EXISTS immutable_refunds_delete BEFORE DELETE ON refunds BEGIN SELECT RAISE(ABORT,'IMMUTABLE:refunds'); END;
CREATE TRIGGER IF NOT EXISTS immutable_refund_lines_update BEFORE UPDATE ON refund_lines BEGIN SELECT RAISE(ABORT,'IMMUTABLE:refund_lines'); END;
CREATE TRIGGER IF NOT EXISTS immutable_refund_lines_delete BEFORE DELETE ON refund_lines BEGIN SELECT RAISE(ABORT,'IMMUTABLE:refund_lines'); END;
CREATE TRIGGER IF NOT EXISTS immutable_cash_movements_update BEFORE UPDATE ON cash_movements BEGIN SELECT RAISE(ABORT,'IMMUTABLE:cash_movements'); END;
CREATE TRIGGER IF NOT EXISTS immutable_cash_movements_delete BEFORE DELETE ON cash_movements BEGIN SELECT RAISE(ABORT,'IMMUTABLE:cash_movements'); END;
CREATE TRIGGER IF NOT EXISTS immutable_audit_events_update BEFORE UPDATE ON audit_events BEGIN SELECT RAISE(ABORT,'IMMUTABLE:audit_events'); END;
CREATE TRIGGER IF NOT EXISTS immutable_audit_events_delete BEFORE DELETE ON audit_events BEGIN SELECT RAISE(ABORT,'IMMUTABLE:audit_events'); END;
CREATE TRIGGER IF NOT EXISTS immutable_idempotency_update BEFORE UPDATE ON idempotency_results BEGIN SELECT RAISE(ABORT,'IMMUTABLE:idempotency_results'); END;
CREATE TRIGGER IF NOT EXISTS immutable_idempotency_delete BEFORE DELETE ON idempotency_results BEGIN SELECT RAISE(ABORT,'IMMUTABLE:idempotency_results'); END;
CREATE TRIGGER IF NOT EXISTS immutable_supplier_ledger_update BEFORE UPDATE ON supplier_ledger BEGIN SELECT RAISE(ABORT,'IMMUTABLE:supplier_ledger'); END;
CREATE TRIGGER IF NOT EXISTS immutable_supplier_ledger_delete BEFORE DELETE ON supplier_ledger BEGIN SELECT RAISE(ABORT,'IMMUTABLE:supplier_ledger'); END;
CREATE TRIGGER IF NOT EXISTS immutable_credit_ledger_update BEFORE UPDATE ON customer_credit_ledger BEGIN SELECT RAISE(ABORT,'IMMUTABLE:customer_credit_ledger'); END;
CREATE TRIGGER IF NOT EXISTS immutable_credit_ledger_delete BEFORE DELETE ON customer_credit_ledger BEGIN SELECT RAISE(ABORT,'IMMUTABLE:customer_credit_ledger'); END;
CREATE TRIGGER IF NOT EXISTS immutable_loyalty_ledger_update BEFORE UPDATE ON loyalty_ledger BEGIN SELECT RAISE(ABORT,'IMMUTABLE:loyalty_ledger'); END;
CREATE TRIGGER IF NOT EXISTS immutable_loyalty_ledger_delete BEFORE DELETE ON loyalty_ledger BEGIN SELECT RAISE(ABORT,'IMMUTABLE:loyalty_ledger'); END;
CREATE TRIGGER IF NOT EXISTS immutable_approval_update BEFORE UPDATE ON manager_approval_consumptions BEGIN SELECT RAISE(ABORT,'IMMUTABLE:manager_approval_consumptions'); END;
CREATE TRIGGER IF NOT EXISTS immutable_approval_delete BEFORE DELETE ON manager_approval_consumptions BEGIN SELECT RAISE(ABORT,'IMMUTABLE:manager_approval_consumptions'); END;
