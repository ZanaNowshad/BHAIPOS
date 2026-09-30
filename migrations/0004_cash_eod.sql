PRAGMA foreign_keys = ON;

-- Refund tender effects must be attributable to the cash session that actually
-- paid money out. Existing rows remain valid with NULL for non-cash/historical data.
CREATE INDEX IF NOT EXISTS ix_refund_payments_cash_session
  ON refund_payments(cash_session_id, tender_kind, created_at);

CREATE INDEX IF NOT EXISTS ix_cash_movements_session
  ON cash_movements(cash_session_id, kind, created_at);

CREATE TRIGGER IF NOT EXISTS guard_refund_payments_insert BEFORE INSERT ON refund_payments
WHEN
  (SELECT tenant_id FROM refunds WHERE id=NEW.refund_id) IS NULL
  OR (NEW.cash_session_id IS NOT NULL AND (SELECT tenant_id FROM cash_sessions WHERE id=NEW.cash_session_id) IS NOT (SELECT tenant_id FROM refunds WHERE id=NEW.refund_id))
  OR (NEW.device_id IS NOT NULL AND (SELECT tenant_id FROM devices WHERE id=NEW.device_id) IS NOT (SELECT tenant_id FROM refunds WHERE id=NEW.refund_id))
  OR (NEW.user_id IS NOT NULL AND (SELECT tenant_id FROM users WHERE id=NEW.user_id) IS NOT (SELECT tenant_id FROM refunds WHERE id=NEW.refund_id))
BEGIN SELECT RAISE(ABORT,'TENANT_SCOPE_VIOLATION:refund_payments'); END;

CREATE TRIGGER IF NOT EXISTS guard_refund_payment_amount BEFORE INSERT ON refund_payments
WHEN NEW.amount_fils <= 0
BEGIN SELECT RAISE(ABORT,'INVALID_REFUND_PAYMENT_AMOUNT'); END;

CREATE TRIGGER IF NOT EXISTS immutable_refund_payments_update BEFORE UPDATE ON refund_payments
BEGIN SELECT RAISE(ABORT,'IMMUTABLE:refund_payments'); END;
CREATE TRIGGER IF NOT EXISTS immutable_refund_payments_delete BEFORE DELETE ON refund_payments
BEGIN SELECT RAISE(ABORT,'IMMUTABLE:refund_payments'); END;

CREATE TRIGGER IF NOT EXISTS guard_cash_movement_insert BEFORE INSERT ON cash_movements
WHEN
  (SELECT tenant_id FROM cash_sessions WHERE id=NEW.cash_session_id) IS NOT NEW.tenant_id
  OR (SELECT branch_id FROM cash_sessions WHERE id=NEW.cash_session_id) IS NOT NEW.branch_id
  OR (SELECT tenant_id FROM devices WHERE id=NEW.device_id) IS NOT NEW.tenant_id
  OR (SELECT branch_id FROM devices WHERE id=NEW.device_id) IS NOT NEW.branch_id
  OR (SELECT tenant_id FROM users WHERE id=NEW.user_id) IS NOT NEW.tenant_id
BEGIN SELECT RAISE(ABORT,'TENANT_SCOPE_VIOLATION:cash_movements'); END;

CREATE TABLE IF NOT EXISTS sale_void_payment_effects (
  id TEXT PRIMARY KEY,
  tenant_id TEXT NOT NULL REFERENCES tenants(id),
  branch_id TEXT NOT NULL REFERENCES branches(id),
  sale_void_id TEXT NOT NULL REFERENCES sale_voids(id),
  tender_kind TEXT NOT NULL,
  amount_fils INTEGER NOT NULL CHECK(amount_fils > 0),
  cash_session_id TEXT REFERENCES cash_sessions(id),
  device_id TEXT NOT NULL REFERENCES devices(id),
  user_id TEXT NOT NULL REFERENCES users(id),
  reference TEXT,
  created_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS ix_sale_void_payment_cash_session
  ON sale_void_payment_effects(cash_session_id,tender_kind,created_at);

CREATE TRIGGER IF NOT EXISTS guard_sale_void_payment_insert BEFORE INSERT ON sale_void_payment_effects
WHEN
  (SELECT tenant_id FROM sale_voids WHERE id=NEW.sale_void_id) IS NOT NEW.tenant_id
  OR (SELECT branch_id FROM sale_voids WHERE id=NEW.sale_void_id) IS NOT NEW.branch_id
  OR (SELECT tenant_id FROM devices WHERE id=NEW.device_id) IS NOT NEW.tenant_id
  OR (SELECT branch_id FROM devices WHERE id=NEW.device_id) IS NOT NEW.branch_id
  OR (SELECT tenant_id FROM users WHERE id=NEW.user_id) IS NOT NEW.tenant_id
  OR (NEW.cash_session_id IS NOT NULL AND (SELECT tenant_id FROM cash_sessions WHERE id=NEW.cash_session_id) IS NOT NEW.tenant_id)
  OR (NEW.cash_session_id IS NOT NULL AND (SELECT branch_id FROM cash_sessions WHERE id=NEW.cash_session_id) IS NOT NEW.branch_id)
BEGIN SELECT RAISE(ABORT,'TENANT_SCOPE_VIOLATION:sale_void_payment_effects'); END;

CREATE TRIGGER IF NOT EXISTS immutable_sale_void_payment_update BEFORE UPDATE ON sale_void_payment_effects
BEGIN SELECT RAISE(ABORT,'IMMUTABLE:sale_void_payment_effects'); END;
CREATE TRIGGER IF NOT EXISTS immutable_sale_void_payment_delete BEFORE DELETE ON sale_void_payment_effects
BEGIN SELECT RAISE(ABORT,'IMMUTABLE:sale_void_payment_effects'); END;
