PRAGMA foreign_keys = ON;

-- SQLite uses dynamic typing even for INTEGER-affinity columns. These guards
-- reject REAL/TEXT values and invalid tax configurations at the persistence
-- boundary instead of relying only on Rust constructors.
CREATE TRIGGER IF NOT EXISTS guard_products_financial_insert BEFORE INSERT ON products
WHEN typeof(NEW.base_price_fils)!='integer' OR typeof(NEW.current_cost_fils)!='integer'
  OR NEW.base_price_fils<0 OR NEW.current_cost_fils<0
  OR typeof(NEW.tax_rate_bps)!='integer' OR NEW.tax_rate_bps<0
  OR NEW.tax_category NOT IN ('STANDARD','ZERO','EXEMPT','OUT_OF_SCOPE','CUSTOM')
  OR (NEW.tax_category IN ('ZERO','EXEMPT','OUT_OF_SCOPE') AND NEW.tax_rate_bps!=0)
  OR typeof(NEW.tax_inclusive)!='integer' OR NEW.tax_inclusive NOT IN (0,1)
BEGIN SELECT RAISE(ABORT,'INVALID_FINANCIAL_DOMAIN:products'); END;
CREATE TRIGGER IF NOT EXISTS guard_products_financial_update BEFORE UPDATE OF base_price_fils,current_cost_fils,tax_category,tax_rate_bps,tax_inclusive ON products
WHEN typeof(NEW.base_price_fils)!='integer' OR typeof(NEW.current_cost_fils)!='integer'
  OR NEW.base_price_fils<0 OR NEW.current_cost_fils<0
  OR typeof(NEW.tax_rate_bps)!='integer' OR NEW.tax_rate_bps<0
  OR NEW.tax_category NOT IN ('STANDARD','ZERO','EXEMPT','OUT_OF_SCOPE','CUSTOM')
  OR (NEW.tax_category IN ('ZERO','EXEMPT','OUT_OF_SCOPE') AND NEW.tax_rate_bps!=0)
  OR typeof(NEW.tax_inclusive)!='integer' OR NEW.tax_inclusive NOT IN (0,1)
BEGIN SELECT RAISE(ABORT,'INVALID_FINANCIAL_DOMAIN:products'); END;

CREATE TRIGGER IF NOT EXISTS guard_price_history_financial_insert BEFORE INSERT ON price_history
WHEN typeof(NEW.price_fils)!='integer' OR NEW.price_fils<0
BEGIN SELECT RAISE(ABORT,'INVALID_FINANCIAL_DOMAIN:price_history'); END;
CREATE TRIGGER IF NOT EXISTS guard_cost_history_financial_insert BEFORE INSERT ON cost_history
WHEN typeof(NEW.cost_fils)!='integer' OR NEW.cost_fils<0
BEGIN SELECT RAISE(ABORT,'INVALID_FINANCIAL_DOMAIN:cost_history'); END;

CREATE TRIGGER IF NOT EXISTS guard_cart_lines_financial_insert BEFORE INSERT ON cart_lines
WHEN typeof(NEW.quantity_milli)!='integer' OR NEW.quantity_milli<=0
  OR typeof(NEW.unit_price_fils)!='integer' OR NEW.unit_price_fils<0
  OR typeof(NEW.unit_cost_fils)!='integer' OR NEW.unit_cost_fils<0
  OR typeof(NEW.tax_rate_bps)!='integer' OR NEW.tax_rate_bps<0
  OR NEW.tax_category_snapshot NOT IN ('STANDARD','ZERO','EXEMPT','OUT_OF_SCOPE','CUSTOM')
  OR (NEW.tax_category_snapshot IN ('ZERO','EXEMPT','OUT_OF_SCOPE') AND NEW.tax_rate_bps!=0)
  OR typeof(NEW.tax_inclusive)!='integer' OR NEW.tax_inclusive NOT IN (0,1)
BEGIN SELECT RAISE(ABORT,'INVALID_FINANCIAL_DOMAIN:cart_lines'); END;

CREATE TRIGGER IF NOT EXISTS guard_sales_financial_insert BEFORE INSERT ON sales
WHEN typeof(NEW.subtotal_fils)!='integer' OR typeof(NEW.tax_fils)!='integer'
  OR typeof(NEW.total_fils)!='integer' OR typeof(NEW.cogs_fils)!='integer'
  OR NEW.subtotal_fils<0 OR NEW.tax_fils<0 OR NEW.total_fils<0 OR NEW.cogs_fils<0
  OR NEW.total_fils!=NEW.subtotal_fils+NEW.tax_fils
BEGIN SELECT RAISE(ABORT,'INVALID_FINANCIAL_DOMAIN:sales'); END;
CREATE TRIGGER IF NOT EXISTS guard_sale_lines_financial_insert BEFORE INSERT ON sale_lines
WHEN typeof(NEW.quantity_milli)!='integer' OR NEW.quantity_milli<=0
  OR typeof(NEW.unit_price_fils)!='integer' OR NEW.unit_price_fils<0
  OR typeof(NEW.unit_cost_fils)!='integer' OR NEW.unit_cost_fils<0
  OR typeof(NEW.net_fils)!='integer' OR NEW.net_fils<0
  OR typeof(NEW.tax_fils)!='integer' OR NEW.tax_fils<0
  OR typeof(NEW.gross_fils)!='integer' OR NEW.gross_fils<0
  OR NEW.gross_fils!=NEW.net_fils+NEW.tax_fils
  OR typeof(NEW.tax_rate_bps)!='integer' OR NEW.tax_rate_bps<0
  OR NEW.tax_category_snapshot NOT IN ('STANDARD','ZERO','EXEMPT','OUT_OF_SCOPE','CUSTOM')
  OR (NEW.tax_category_snapshot IN ('ZERO','EXEMPT','OUT_OF_SCOPE') AND NEW.tax_rate_bps!=0)
  OR typeof(NEW.tax_inclusive)!='integer' OR NEW.tax_inclusive NOT IN (0,1)
BEGIN SELECT RAISE(ABORT,'INVALID_FINANCIAL_DOMAIN:sale_lines'); END;
CREATE TRIGGER IF NOT EXISTS guard_sale_payments_financial_insert BEFORE INSERT ON sale_payments
WHEN typeof(NEW.amount_fils)!='integer' OR NEW.amount_fils<=0
  OR (NEW.tendered_fils IS NOT NULL AND (typeof(NEW.tendered_fils)!='integer' OR NEW.tendered_fils<NEW.amount_fils))
  OR typeof(NEW.change_fils)!='integer' OR NEW.change_fils<0
  OR (NEW.tendered_fils IS NULL AND NEW.change_fils!=0)
  OR (NEW.tendered_fils IS NOT NULL AND NEW.change_fils!=NEW.tendered_fils-NEW.amount_fils)
BEGIN SELECT RAISE(ABORT,'INVALID_FINANCIAL_DOMAIN:sale_payments'); END;

CREATE TRIGGER IF NOT EXISTS guard_refunds_financial_insert BEFORE INSERT ON refunds
WHEN typeof(NEW.subtotal_fils)!='integer' OR typeof(NEW.tax_fils)!='integer' OR typeof(NEW.total_fils)!='integer'
  OR NEW.subtotal_fils<0 OR NEW.tax_fils<0 OR NEW.total_fils<0
  OR NEW.total_fils!=NEW.subtotal_fils+NEW.tax_fils
BEGIN SELECT RAISE(ABORT,'INVALID_FINANCIAL_DOMAIN:refunds'); END;
CREATE TRIGGER IF NOT EXISTS guard_refund_lines_financial_insert BEFORE INSERT ON refund_lines
WHEN typeof(NEW.quantity_milli)!='integer' OR NEW.quantity_milli<=0
  OR typeof(NEW.net_fils)!='integer' OR NEW.net_fils<0
  OR typeof(NEW.tax_fils)!='integer' OR NEW.tax_fils<0
  OR typeof(NEW.gross_fils)!='integer' OR NEW.gross_fils<0
  OR NEW.gross_fils!=NEW.net_fils+NEW.tax_fils
BEGIN SELECT RAISE(ABORT,'INVALID_FINANCIAL_DOMAIN:refund_lines'); END;
CREATE TRIGGER IF NOT EXISTS guard_refund_payments_financial_insert BEFORE INSERT ON refund_payments
WHEN typeof(NEW.amount_fils)!='integer' OR NEW.amount_fils<=0
BEGIN SELECT RAISE(ABORT,'INVALID_FINANCIAL_DOMAIN:refund_payments'); END;

CREATE TRIGGER IF NOT EXISTS guard_cash_sessions_financial_insert BEFORE INSERT ON cash_sessions
WHEN typeof(NEW.opening_float_fils)!='integer' OR NEW.opening_float_fils<0
  OR (NEW.counted_cash_fils IS NOT NULL AND (typeof(NEW.counted_cash_fils)!='integer' OR NEW.counted_cash_fils<0))
  OR (NEW.expected_cash_fils IS NOT NULL AND typeof(NEW.expected_cash_fils)!='integer')
  OR (NEW.variance_fils IS NOT NULL AND typeof(NEW.variance_fils)!='integer')
BEGIN SELECT RAISE(ABORT,'INVALID_FINANCIAL_DOMAIN:cash_sessions'); END;
CREATE TRIGGER IF NOT EXISTS guard_cash_sessions_financial_update BEFORE UPDATE OF opening_float_fils,counted_cash_fils,expected_cash_fils,variance_fils ON cash_sessions
WHEN typeof(NEW.opening_float_fils)!='integer' OR NEW.opening_float_fils<0
  OR (NEW.counted_cash_fils IS NOT NULL AND (typeof(NEW.counted_cash_fils)!='integer' OR NEW.counted_cash_fils<0))
  OR (NEW.expected_cash_fils IS NOT NULL AND typeof(NEW.expected_cash_fils)!='integer')
  OR (NEW.variance_fils IS NOT NULL AND typeof(NEW.variance_fils)!='integer')
BEGIN SELECT RAISE(ABORT,'INVALID_FINANCIAL_DOMAIN:cash_sessions'); END;
CREATE TRIGGER IF NOT EXISTS guard_cash_movements_financial_insert BEFORE INSERT ON cash_movements
WHEN typeof(NEW.amount_fils)!='integer'
  OR (NEW.kind='NO_SALE' AND NEW.amount_fils!=0)
  OR (NEW.kind!='NO_SALE' AND NEW.amount_fils<=0)
BEGIN SELECT RAISE(ABORT,'INVALID_FINANCIAL_DOMAIN:cash_movements'); END;

CREATE TRIGGER IF NOT EXISTS guard_inventory_movements_cost_insert BEFORE INSERT ON inventory_movements
WHEN typeof(NEW.quantity_milli)!='integer' OR NEW.quantity_milli=0
  OR typeof(NEW.unit_cost_fils)!='integer' OR NEW.unit_cost_fils<0
BEGIN SELECT RAISE(ABORT,'INVALID_FINANCIAL_DOMAIN:inventory_movements'); END;
CREATE TRIGGER IF NOT EXISTS guard_sale_voids_financial_insert BEFORE INSERT ON sale_voids
WHEN typeof(NEW.reversed_total_fils)!='integer' OR NEW.reversed_total_fils<0
BEGIN SELECT RAISE(ABORT,'INVALID_FINANCIAL_DOMAIN:sale_voids'); END;
CREATE TRIGGER IF NOT EXISTS guard_sale_void_payments_financial_insert BEFORE INSERT ON sale_void_payment_effects
WHEN typeof(NEW.amount_fils)!='integer' OR NEW.amount_fils<=0
BEGIN SELECT RAISE(ABORT,'INVALID_FINANCIAL_DOMAIN:sale_void_payment_effects'); END;

-- Historical price/cost rows are evidence. Corrections append a replacement
-- interval rather than rewriting or deleting the original row.
CREATE TRIGGER IF NOT EXISTS immutable_price_history_update BEFORE UPDATE ON price_history
BEGIN SELECT RAISE(ABORT,'IMMUTABLE:price_history'); END;
CREATE TRIGGER IF NOT EXISTS immutable_price_history_delete BEFORE DELETE ON price_history
BEGIN SELECT RAISE(ABORT,'IMMUTABLE:price_history'); END;
CREATE TRIGGER IF NOT EXISTS immutable_cost_history_update BEFORE UPDATE ON cost_history
BEGIN SELECT RAISE(ABORT,'IMMUTABLE:cost_history'); END;
CREATE TRIGGER IF NOT EXISTS immutable_cost_history_delete BEFORE DELETE ON cost_history
BEGIN SELECT RAISE(ABORT,'IMMUTABLE:cost_history'); END;
