use bhaipos_core::{
    compute_audit_hash, hash_pin, hash_secret, price_times_quantity, sha256_hex,
    sign_sync_mutation, verify_approval, verify_pin, verify_secret, verify_sync_mutation,
    ApprovalBinding, AuditMaterial, BranchId, CartId, DeviceId, Money, OperationId, ProductId,
    QuantityMilli, RegisterId, SaleId, SyncMutationEnvelope, SyncMutationMaterial, TaxCategory,
    TaxRule, TenantId, TenderKind, UserId,
};
use chrono::{DateTime, Datelike, FixedOffset, Timelike, Utc};
use rusqlite::{params, Connection, OptionalExtension, Transaction, TransactionBehavior};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use thiserror::Error;
use uuid::Uuid;

#[path = "customer_ops.rs"]
mod customer_ops;
#[path = "procurement.rs"]
mod procurement;
#[path = "production.rs"]
mod production;

const MIGRATION_0001: &str = include_str!("../../../migrations/0001_core.sql");
const MIGRATION_0002: &str = include_str!("../../../migrations/0002_retail_os.sql");
const MIGRATION_0003: &str = include_str!("../../../migrations/0003_integrity_guards.sql");
const MIGRATION_0004: &str = include_str!("../../../migrations/0004_cash_eod.sql");
const MIGRATION_0005: &str =
    include_str!("../../../migrations/0005_idempotency_payload_binding.sql");
const MIGRATION_0006: &str = include_str!("../../../migrations/0006_financial_domain_guards.sql");
const MIGRATION_0007: &str = include_str!("../../../migrations/0007_local_terminal_binding.sql");
const MIGRATION_0008: &str =
    include_str!("../../../migrations/0008_receipt_reprint_permission.sql");
const MIGRATION_0009: &str = include_str!("../../../migrations/0009_print_leases_and_profiles.sql");
const MIGRATION_0010: &str = include_str!("../../../migrations/0010_sync_protocol.sql");
const MIGRATION_0011: &str = include_str!("../../../migrations/0011_inventory_operations.sql");
const MIGRATION_0012: &str =
    include_str!("../../../migrations/0012_procurement_supplier_finance.sql");
const MIGRATION_0013: &str = include_str!("../../../migrations/0013_customer_store_operations.sql");
const MIGRATION_0014: &str = include_str!("../../../migrations/0014_production_operations.sql");
pub const LATEST_SCHEMA: &str = "0014_production_operations";

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("database: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("not found: {0}")]
    NotFound(&'static str),
    #[error("authorization denied: {0}")]
    Authorization(&'static str),
    #[error("validation failed: {0}")]
    Validation(String),
    #[error("conflict: {0}")]
    Conflict(String),
    #[error("serialization: {0}")]
    Serde(#[from] serde_json::Error),
    #[error("money: {0}")]
    Money(#[from] bhaipos_core::MoneyError),
}

#[derive(Clone, Debug)]
pub struct NewProduct<'a> {
    pub tenant_id: TenantId,
    pub sku: &'a str,
    pub name: &'a str,
    pub barcode: &'a str,
    pub price: Money,
    pub cost: Money,
    pub tax_rate_bps: i32,
    pub tax_inclusive: bool,
    pub track_inventory: bool,
    pub allow_decimal_qty: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PaymentInput {
    pub kind: TenderKind,
    pub amount: Money,
    pub tendered: Option<Money>,
    pub reference: Option<String>,
}

#[derive(Clone, Debug)]
pub struct CheckoutRequest {
    pub tenant_id: TenantId,
    pub branch_id: BranchId,
    pub device_id: DeviceId,
    pub register_id: RegisterId,
    pub user_id: UserId,
    pub cart_id: CartId,
    pub operation_id: OperationId,
    pub cash_session_id: Option<Uuid>,
    pub payments: Vec<PaymentInput>,
    pub now: DateTime<Utc>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheckoutResult {
    pub sale_id: SaleId,
    pub receipt_number: String,
    pub subtotal: Money,
    pub tax: Money,
    pub total: Money,
    pub change: Money,
    pub receipt_sha256: String,
}

#[derive(Clone, Debug)]
pub struct RefundLineInput {
    pub sale_line_id: Uuid,
    pub quantity: QuantityMilli,
}
#[derive(Clone, Debug)]
pub struct RefundRequest {
    pub tenant_id: TenantId,
    pub branch_id: BranchId,
    pub device_id: DeviceId,
    pub user_id: UserId,
    pub operation_id: OperationId,
    pub sale_id: SaleId,
    pub reason: String,
    pub lines: Vec<RefundLineInput>,
    pub cash_session_id: Option<Uuid>,
    pub payments: Vec<PaymentInput>,
    pub now: DateTime<Utc>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RefundResult {
    pub refund_id: Uuid,
    pub subtotal: Money,
    pub tax: Money,
    pub total: Money,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RefundableSaleLine {
    pub sale_line_id: Uuid,
    pub product_name: String,
    pub sold_quantity: QuantityMilli,
    pub refunded_quantity: QuantityMilli,
    pub remaining_quantity: QuantityMilli,
    pub refundable_total: Money,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RefundableSale {
    pub sale_id: SaleId,
    pub receipt_number: String,
    pub completed_at: String,
    pub lines: Vec<RefundableSaleLine>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RefundQuote {
    pub subtotal: Money,
    pub tax: Money,
    pub total: Money,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CashMovementKind {
    PaidIn,
    PaidOut,
    SafeDrop,
    NoSale,
    PettyCash,
}

impl CashMovementKind {
    fn as_db(&self) -> &'static str {
        match self {
            Self::PaidIn => "PAID_IN",
            Self::PaidOut => "PAID_OUT",
            Self::SafeDrop => "SAFE_DROP",
            Self::NoSale => "NO_SALE",
            Self::PettyCash => "PETTY_CASH",
        }
    }
    fn permission(&self) -> &'static str {
        match self {
            Self::PaidIn => "cash.movement.paid_in",
            Self::PaidOut => "cash.movement.paid_out",
            Self::SafeDrop => "cash.movement.safe_drop",
            Self::NoSale => "cash.drawer.no_sale",
            Self::PettyCash => "cash.movement.paid_out",
        }
    }
}

#[derive(Clone, Debug)]
pub struct CashMovementRequest {
    pub tenant_id: TenantId,
    pub branch_id: BranchId,
    pub device_id: DeviceId,
    pub user_id: UserId,
    pub cash_session_id: Uuid,
    pub operation_id: OperationId,
    pub kind: CashMovementKind,
    pub amount: Money,
    pub reason: Option<String>,
    pub now: DateTime<Utc>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CashMovementResult {
    pub movement_id: Uuid,
    pub kind: CashMovementKind,
    pub amount: Money,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CashSessionReport {
    pub cash_session_id: Uuid,
    pub status: String,
    pub opening_float: Money,
    pub cash_sales: Money,
    pub cash_refunds: Money,
    pub cash_voids: Money,
    pub paid_in: Money,
    pub paid_out: Money,
    pub safe_drop: Money,
    pub petty_cash: Money,
    pub no_sale_count: i64,
    pub expected_cash: Money,
    pub counted_cash: Option<Money>,
    pub variance: Option<Money>,
}

#[derive(Clone, Debug)]
pub struct CloseCashSessionRequest {
    pub tenant_id: TenantId,
    pub branch_id: BranchId,
    pub device_id: DeviceId,
    pub user_id: UserId,
    pub cash_session_id: Uuid,
    pub operation_id: OperationId,
    pub counted_cash: Money,
    pub now: DateTime<Utc>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CloseCashSessionResult {
    pub report: CashSessionReport,
    pub variance_case_id: Option<Uuid>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrintJobLease {
    pub print_job_id: Uuid,
    pub sale_id: Option<SaleId>,
    pub document_type: String,
    pub snapshot_sha256: Option<String>,
    pub receipt_text: Option<String>,
    pub printer_target: Option<String>,
    pub lease_token: Uuid,
    pub cash_drawer_pulse: bool,
    pub attempt: i64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrinterProfile {
    pub transport: String,
    pub target: Option<String>,
    pub paper_width_mm: i64,
    pub characters_per_line: i64,
    pub character_encoding: String,
    pub cut_mode: String,
    pub drawer_pulse_policy: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FailedPrintJob {
    pub print_job_id: Uuid,
    pub sale_id: Option<SaleId>,
    pub receipt_number: Option<String>,
    pub document_type: String,
    pub attempts: i64,
    pub last_error: String,
    pub updated_at: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HubMutationResult {
    pub hub_sequence: i64,
    pub state: String,
    pub replayed: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SyncDeliveryOutcome {
    Hub(HubMutationResult),
    Retry(String),
    PermanentFailure(String),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceEnrollmentGrant {
    pub grant_id: Uuid,
    pub enrollment_token: String,
    pub expires_at: DateTime<Utc>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceEnrollmentResult {
    pub device_id: DeviceId,
    pub credential_secret: String,
    pub credential_version: i64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct InventoryReceiptLineInput {
    pub product_id: ProductId,
    pub received_quantity: QuantityMilli,
    pub rejected_quantity: QuantityMilli,
    pub damaged_quantity: QuantityMilli,
    pub unit_cost: Money,
    pub lot_number: Option<String>,
    pub expires_on: Option<String>,
}
#[derive(Clone, Debug)]
pub struct InventoryReceiptRequest {
    pub context: LocalTerminalContext,
    pub user_id: UserId,
    pub operation_id: OperationId,
    pub supplier_id: Uuid,
    pub centre_id: Uuid,
    pub supplier_document_no: Option<String>,
    pub lines: Vec<InventoryReceiptLineInput>,
    pub now: DateTime<Utc>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct InventoryReceiptResult {
    pub receipt_id: Uuid,
    pub accepted_quantity: QuantityMilli,
    pub inventory_value: Money,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct InventoryTransferLineInput {
    pub product_id: ProductId,
    pub quantity: QuantityMilli,
    pub lot_id: Option<Uuid>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct InventoryTransferResult {
    pub transfer_id: Uuid,
    pub status: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransferReceiptLineInput {
    pub transfer_line_id: Uuid,
    pub received_quantity: QuantityMilli,
    pub damaged_quantity: QuantityMilli,
    pub note: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StocktakeApprovalResult {
    pub stocktake_id: Uuid,
    pub adjustments: i64,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WasteResult {
    pub waste_id: Uuid,
    pub cost_value: Money,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct InventoryValuation {
    pub quantity: QuantityMilli,
    pub total_value: Money,
    pub weighted_average_cost: Money,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct InventoryRebuildResult {
    pub reconciliation_id: Uuid,
    pub mismatches: i64,
    pub repaired: bool,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExpiringLot {
    pub lot_id: Uuid,
    pub product_id: ProductId,
    pub lot_number: Option<String>,
    pub expires_on: String,
    pub quantity: QuantityMilli,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PurchaseOrderLineInput {
    pub product_id: ProductId,
    pub quantity: QuantityMilli,
    pub unit_cost: Money,
    pub tax_rate_bps: i32,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PurchaseOrderResult {
    pub purchase_order_id: Uuid,
    pub po_number: String,
    pub status: String,
    pub total: Money,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PurchaseOrderReceiptLineInput {
    pub purchase_order_line_id: Uuid,
    pub received_quantity: QuantityMilli,
    pub rejected_quantity: QuantityMilli,
    pub damaged_quantity: QuantityMilli,
    pub unit_cost: Money,
    pub lot_number: Option<String>,
    pub expires_on: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PurchaseReceiptResult {
    pub receipt_id: Uuid,
    pub purchase_order_id: Uuid,
    pub status: String,
    pub accepted_quantity: QuantityMilli,
    pub cost_variance: Money,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SupplierInvoiceLineInput {
    pub product_id: Option<ProductId>,
    pub description: String,
    pub quantity: QuantityMilli,
    pub unit_cost: Money,
    pub tax: Money,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SupplierInvoiceResult {
    pub invoice_id: Uuid,
    pub total: Money,
    pub status: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SupplierPaymentAllocationInput {
    pub invoice_id: Uuid,
    pub amount: Money,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SupplierPaymentResult {
    pub payment_id: Uuid,
    pub amount: Money,
    pub unallocated: Money,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SupplierStatementEntry {
    pub event_type: String,
    pub source_type: String,
    pub source_id: String,
    pub debit: Money,
    pub credit: Money,
    pub occurred_at: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SupplierStatement {
    pub opening_balance: Money,
    pub debits: Money,
    pub credits: Money,
    pub closing_balance: Money,
    pub entries: Vec<SupplierStatementEntry>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SupplierReturnLineInput {
    pub product_id: ProductId,
    pub lot_id: Option<Uuid>,
    pub quantity: QuantityMilli,
    pub unit_cost: Money,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SupplierReturnResult {
    pub supplier_return_id: Uuid,
    pub status: String,
    pub value: Money,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LoyaltyEventResult {
    pub loyalty_event_id: Uuid,
    pub points_balance: i64,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CustomerCreditPaymentResult {
    pub payment_id: Uuid,
    pub amount: Money,
    pub outstanding_balance: Money,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CustomerCreditBalance {
    pub credit_limit: Money,
    pub outstanding: Money,
    pub available: Money,
    pub overdue: Money,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecipeComponentInput {
    pub product_id: ProductId,
    pub quantity: QuantityMilli,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProductionConsumptionInput {
    pub product_id: ProductId,
    pub quantity: QuantityMilli,
    pub lot_id: Option<Uuid>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProductionResult {
    pub production_order_id: Uuid,
    pub status: String,
    pub output_quantity: QuantityMilli,
    pub output_cost: Money,
}

#[derive(Clone, Debug)]
pub struct VoidSaleRequest {
    pub tenant_id: TenantId,
    pub branch_id: BranchId,
    pub device_id: DeviceId,
    pub user_id: UserId,
    pub operation_id: OperationId,
    pub sale_id: SaleId,
    pub cash_session_id: Option<Uuid>,
    pub approval_ref: Uuid,
    pub reason: String,
    pub now: DateTime<Utc>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct VoidSaleResult {
    pub void_id: Uuid,
    pub sale_id: SaleId,
    pub reversed_total: Money,
}

#[derive(Clone, Debug)]
pub struct LocalBootstrapRequest {
    pub business_name: String,
    pub branch_code: String,
    pub branch_name: String,
    pub register_code: String,
    pub register_name: String,
    pub device_label: String,
    pub owner_employee_no: String,
    pub owner_name: String,
    pub owner_pin: String,
    pub terminal: LocalTerminalContext,
    pub device_credential_secret: String,
    pub now: DateTime<Utc>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalTerminalContext {
    pub tenant_id: TenantId,
    pub branch_id: BranchId,
    pub device_id: DeviceId,
    pub register_id: RegisterId,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalBootstrapResult {
    pub terminal: LocalTerminalContext,
    pub owner_user_id: UserId,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CartLineSnapshot {
    pub id: Uuid,
    pub product_id: ProductId,
    pub name: String,
    pub sku: String,
    pub barcode: Option<String>,
    pub quantity: QuantityMilli,
    pub unit_price: Money,
    pub net: Money,
    pub tax: Money,
    pub gross: Money,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CartSnapshot {
    pub cart_id: CartId,
    pub status: String,
    pub note: Option<String>,
    pub lines: Vec<CartLineSnapshot>,
    pub candidate_subtotal: Money,
    pub candidate_tax: Money,
    pub candidate_total: Money,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HeldCartSummary {
    pub cart_id: CartId,
    pub note: Option<String>,
    pub updated_at: String,
}

pub struct Store {
    conn: Connection,
}

type BarcodeProductRow = (String, String, String, i64, i64, i32, i32, i32);
type PrintJobRow = (
    String,
    Option<String>,
    String,
    Option<String>,
    Option<String>,
    i64,
    Option<String>,
    i32,
);

impl Store {
    pub fn in_memory() -> Result<Self, StoreError> {
        let conn = Connection::open_in_memory()?;
        let s = Self { conn };
        s.migrate()?;
        Ok(s)
    }
    pub fn open(path: &str) -> Result<Self, StoreError> {
        let conn = Connection::open(path)?;
        let s = Self { conn };
        s.migrate()?;
        Ok(s)
    }
    pub fn connection(&self) -> &Connection {
        &self.conn
    }
    fn migrate(&self) -> Result<(), StoreError> {
        self.conn.execute_batch(
            "PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON; PRAGMA busy_timeout=5000;",
        )?;
        self.conn.execute_batch(MIGRATION_0001)?;
        self.conn.execute_batch(MIGRATION_0002)?;
        self.conn.execute_batch(MIGRATION_0003)?;
        Self::ensure_column(&self.conn,"refund_payments","cash_session_id","ALTER TABLE refund_payments ADD COLUMN cash_session_id TEXT REFERENCES cash_sessions(id)")?;
        Self::ensure_column(
            &self.conn,
            "refund_payments",
            "device_id",
            "ALTER TABLE refund_payments ADD COLUMN device_id TEXT REFERENCES devices(id)",
        )?;
        Self::ensure_column(
            &self.conn,
            "refund_payments",
            "user_id",
            "ALTER TABLE refund_payments ADD COLUMN user_id TEXT REFERENCES users(id)",
        )?;
        self.conn.execute_batch(MIGRATION_0004)?;
        self.conn.execute_batch(MIGRATION_0005)?;
        self.conn.execute_batch(MIGRATION_0006)?;
        self.conn.execute_batch(MIGRATION_0007)?;
        self.conn.execute_batch(MIGRATION_0008)?;
        self.conn.execute_batch(MIGRATION_0009)?;
        self.conn.execute_batch(MIGRATION_0010)?;
        self.conn.execute_batch(MIGRATION_0011)?;
        self.conn.execute_batch(MIGRATION_0012)?;
        self.conn.execute_batch(MIGRATION_0013)?;
        self.conn.execute_batch(MIGRATION_0014)?;
        Ok(())
    }
    fn ensure_column(
        conn: &Connection,
        table: &str,
        column: &str,
        ddl: &str,
    ) -> Result<(), StoreError> {
        let mut stmt = conn.prepare(&format!("PRAGMA table_info({table})"))?;
        let cols = stmt.query_map([], |r| r.get::<_, String>(1))?;
        for col in cols {
            if col? == column {
                return Ok(());
            }
        }
        conn.execute_batch(ddl)?;
        Ok(())
    }

    pub fn local_terminal_context(&self) -> Result<Option<LocalTerminalContext>, StoreError> {
        let row:Option<(String,String,String,String)>=self.conn.query_row("SELECT tenant_id,branch_id,device_id,register_id FROM local_terminal_binding WHERE singleton=1",[],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional()?;
        row.map(|(tenant, branch, device, register)| {
            Ok(LocalTerminalContext {
                tenant_id: TenantId(
                    Uuid::parse_str(&tenant).map_err(|_| {
                        StoreError::Validation("invalid bound tenant identity".into())
                    })?,
                ),
                branch_id: BranchId(
                    Uuid::parse_str(&branch).map_err(|_| {
                        StoreError::Validation("invalid bound branch identity".into())
                    })?,
                ),
                device_id: DeviceId(
                    Uuid::parse_str(&device).map_err(|_| {
                        StoreError::Validation("invalid bound device identity".into())
                    })?,
                ),
                register_id: RegisterId(Uuid::parse_str(&register).map_err(|_| {
                    StoreError::Validation("invalid bound register identity".into())
                })?),
            })
        })
        .transpose()
    }

    pub fn bootstrap_local_business(
        &mut self,
        request: LocalBootstrapRequest,
    ) -> Result<LocalBootstrapResult, StoreError> {
        for (field, value) in [
            ("business name", &request.business_name),
            ("branch code", &request.branch_code),
            ("branch name", &request.branch_name),
            ("register code", &request.register_code),
            ("register name", &request.register_name),
            ("device label", &request.device_label),
            ("owner employee number", &request.owner_employee_no),
            ("owner name", &request.owner_name),
        ] {
            if value.trim().is_empty() {
                return Err(StoreError::Validation(format!("{field} is required")));
            }
        }
        let pin_hash = hash_pin(&request.owner_pin)
            .map_err(|error| StoreError::Validation(error.to_string()))?;
        let credential_hash = hash_secret(&request.device_credential_secret)
            .map_err(|error| StoreError::Validation(error.to_string()))?;
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let existing: i64 = tx.query_row("SELECT COUNT(*) FROM tenants", [], |row| row.get(0))?;
        let binding: i64 =
            tx.query_row("SELECT COUNT(*) FROM local_terminal_binding", [], |row| {
                row.get(0)
            })?;
        if existing != 0 || binding != 0 {
            return Err(StoreError::Conflict(
                "local business is already initialized".into(),
            ));
        }
        let terminal = request.terminal;
        let owner_user_id = UserId::new();
        let role_id = Uuid::new_v4();
        let centre_id = Uuid::new_v4();
        let now = request.now.to_rfc3339();
        tx.execute(
            "INSERT INTO tenants(id,name,business_close_hour,created_at) VALUES(?1,?2,3,?3)",
            params![
                terminal.tenant_id.to_string(),
                request.business_name.trim(),
                now
            ],
        )?;
        tx.execute(
            "INSERT INTO branches(id,tenant_id,code,name,created_at) VALUES(?1,?2,?3,?4,?5)",
            params![
                terminal.branch_id.to_string(),
                terminal.tenant_id.to_string(),
                request.branch_code.trim(),
                request.branch_name.trim(),
                now
            ],
        )?;
        tx.execute("INSERT INTO users(id,tenant_id,employee_no,display_name,pin_hash,status,created_at) VALUES(?1,?2,?3,?4,?5,'ACTIVE',?6)",params![owner_user_id.to_string(),terminal.tenant_id.to_string(),request.owner_employee_no.trim(),request.owner_name.trim(),pin_hash,now])?;
        tx.execute("INSERT INTO devices(id,tenant_id,branch_id,label,status,credential_hash,created_at) VALUES(?1,?2,?3,?4,'ACTIVE',?5,?6)",params![terminal.device_id.to_string(),terminal.tenant_id.to_string(),terminal.branch_id.to_string(),request.device_label.trim(),credential_hash,now])?;
        tx.execute(
            "INSERT INTO registers(id,tenant_id,branch_id,code,name) VALUES(?1,?2,?3,?4,?5)",
            params![
                terminal.register_id.to_string(),
                terminal.tenant_id.to_string(),
                terminal.branch_id.to_string(),
                request.register_code.trim(),
                request.register_name.trim()
            ],
        )?;
        tx.execute("INSERT INTO inventory_centres(id,tenant_id,branch_id,code,name,centre_type) VALUES(?1,?2,?3,'FLOOR','Shop Floor','SHOP_FLOOR')",params![centre_id.to_string(),terminal.tenant_id.to_string(),terminal.branch_id.to_string()])?;
        tx.execute(
            "INSERT INTO roles(id,tenant_id,name) VALUES(?1,?2,'owner')",
            params![role_id.to_string(), terminal.tenant_id.to_string()],
        )?;
        for (code, description) in [
            ("sale.checkout", "Complete sales"),
            ("sale.hold", "Hold and restore carts"),
            ("sale.refund", "Refund sales"),
            ("sale.void", "Void sales"),
            ("receipt.reprint", "Requeue failed receipt printing"),
            ("sync.resolve", "Resolve synchronization conflicts"),
            ("device.rotate", "Rotate the local terminal credential"),
            ("device.enroll", "Issue terminal enrollment grants"),
            ("inventory.receive", "Post inventory receiving"),
            ("inventory.transfer", "Dispatch and receive stock transfers"),
            ("inventory.stocktake", "Count and approve stocktakes"),
            ("inventory.waste", "Record inventory waste"),
            (
                "inventory.reconcile",
                "Verify and rebuild inventory projections",
            ),
            ("procurement.manage", "Create and progress purchase orders"),
            ("procurement.approve", "Approve purchase obligations"),
            ("supplier.invoice.post", "Post supplier invoices"),
            ("supplier.payment.post", "Post supplier payments"),
            ("supplier.statement.view", "View supplier statements"),
            ("supplier.return", "Dispatch and settle supplier returns"),
            ("customer.manage", "Manage customers"),
            ("loyalty.adjust", "Post loyalty ledger events"),
            ("customer.credit.manage", "Manage customer credit limits"),
            (
                "customer.credit.collect",
                "Collect customer credit payments",
            ),
            ("customer.credit.view", "View customer credit statements"),
            ("production.manage", "Create and complete production orders"),
            ("cash.session.open", "Open cash sessions"),
            ("cash.session.close", "Close cash sessions"),
            ("cash.movement.paid_in", "Record paid in"),
            ("cash.movement.paid_out", "Record paid out"),
            ("cash.movement.safe_drop", "Record safe drops"),
            ("cash.drawer.no_sale", "Open drawer without sale"),
            ("manager.approve", "Approve sensitive actions"),
        ] {
            tx.execute("INSERT INTO permissions(code,description) VALUES(?1,?2) ON CONFLICT(code) DO UPDATE SET description=excluded.description",params![code,description])?;
            tx.execute(
                "INSERT INTO role_permissions(role_id,permission_code) VALUES(?1,?2)",
                params![role_id.to_string(), code],
            )?;
        }
        tx.execute(
            "INSERT INTO user_roles(user_id,role_id,branch_id) VALUES(?1,?2,?3)",
            params![
                owner_user_id.to_string(),
                role_id.to_string(),
                terminal.branch_id.to_string()
            ],
        )?;
        tx.execute("INSERT INTO local_terminal_binding(singleton,tenant_id,branch_id,device_id,register_id,installed_at) VALUES(1,?1,?2,?3,?4,?5)",params![terminal.tenant_id.to_string(),terminal.branch_id.to_string(),terminal.device_id.to_string(),terminal.register_id.to_string(),now])?;
        tx.execute("INSERT INTO printer_profiles(id,tenant_id,branch_id,device_id,friendly_name,transport,target,paper_width_mm,characters_per_line,character_encoding,cut_mode,drawer_pulse_policy,is_default,active,created_at,updated_at) VALUES(?1,?2,?3,?4,'Default receipt printer','WINDOWS_SPOOLER',NULL,80,48,'ASCII','PARTIAL','CASH_SALE',1,1,?5,?5)",params![Uuid::new_v4().to_string(),terminal.tenant_id.to_string(),terminal.branch_id.to_string(),terminal.device_id.to_string(),now])?;
        tx.commit()?;
        Ok(LocalBootstrapResult {
            terminal,
            owner_user_id,
        })
    }

    pub fn authenticate_employee_pin(
        &self,
        tenant: TenantId,
        employee_no: &str,
        pin: &str,
        now: DateTime<Utc>,
        max_attempts: i64,
        lock_minutes: i64,
    ) -> Result<UserId, StoreError> {
        let mut statement = self
            .conn
            .prepare("SELECT id FROM users WHERE tenant_id=?1 AND employee_no=?2 LIMIT 2")?;
        let ids = statement
            .query_map(params![tenant.to_string(), employee_no.trim()], |row| {
                row.get::<_, String>(0)
            })?
            .collect::<Result<Vec<_>, _>>()?;
        if ids.len() != 1 {
            return Err(StoreError::Authorization(
                "employee identity is unknown or ambiguous",
            ));
        }
        let user = UserId(
            Uuid::parse_str(&ids[0])
                .map_err(|_| StoreError::Validation("invalid user identity".into()))?,
        );
        self.authenticate_pin(tenant, user, pin, now, max_attempts, lock_minutes)?;
        Ok(user)
    }

    pub fn active_cash_session_for(
        &self,
        context: LocalTerminalContext,
        user: UserId,
    ) -> Result<Option<Uuid>, StoreError> {
        let id:Option<String>=self.conn.query_row("SELECT id FROM cash_sessions WHERE tenant_id=?1 AND branch_id=?2 AND register_id=?3 AND opened_by_user_id=?4 AND opened_on_device_id=?5 AND status='OPEN'",params![context.tenant_id.to_string(),context.branch_id.to_string(),context.register_id.to_string(),user.to_string(),context.device_id.to_string()],|row|row.get(0)).optional()?;
        id.map(|value| {
            Uuid::parse_str(&value)
                .map_err(|_| StoreError::Validation("invalid cash session identity".into()))
        })
        .transpose()
    }

    pub fn validate_local_session(
        &self,
        context: LocalTerminalContext,
        user: UserId,
    ) -> Result<(), StoreError> {
        let bound:Option<i32>=self.conn.query_row("SELECT 1 FROM local_terminal_binding WHERE singleton=1 AND tenant_id=?1 AND branch_id=?2 AND device_id=?3 AND register_id=?4",params![context.tenant_id.to_string(),context.branch_id.to_string(),context.device_id.to_string(),context.register_id.to_string()],|row|row.get(0)).optional()?;
        if bound.is_none() {
            return Err(StoreError::Authorization("terminal binding mismatch"));
        }
        self.assert_active_device(
            &self.conn,
            context.tenant_id,
            context.branch_id,
            context.device_id,
        )?;
        let active_user: Option<i32> = self
            .conn
            .query_row(
                "SELECT 1 FROM users WHERE id=?1 AND tenant_id=?2 AND status='ACTIVE'",
                params![user.to_string(), context.tenant_id.to_string()],
                |row| row.get(0),
            )
            .optional()?;
        if active_user.is_none() {
            return Err(StoreError::Authorization(
                "user is not active in local tenant",
            ));
        }
        Ok(())
    }

    pub fn create_tenant(
        &self,
        id: TenantId,
        name: &str,
        close_hour: u8,
        now: DateTime<Utc>,
    ) -> Result<(), StoreError> {
        self.conn.execute(
            "INSERT INTO tenants(id,name,business_close_hour,created_at) VALUES(?1,?2,?3,?4)",
            params![id.to_string(), name, close_hour, now.to_rfc3339()],
        )?;
        Ok(())
    }
    pub fn create_branch(
        &self,
        id: BranchId,
        tenant: TenantId,
        code: &str,
        name: &str,
        now: DateTime<Utc>,
    ) -> Result<(), StoreError> {
        self.conn.execute(
            "INSERT INTO branches(id,tenant_id,code,name,created_at) VALUES(?1,?2,?3,?4,?5)",
            params![
                id.to_string(),
                tenant.to_string(),
                code,
                name,
                now.to_rfc3339()
            ],
        )?;
        Ok(())
    }
    pub fn create_user(
        &self,
        id: UserId,
        tenant: TenantId,
        name: &str,
        pin_hash: &str,
        now: DateTime<Utc>,
    ) -> Result<(), StoreError> {
        self.conn.execute("INSERT INTO users(id,tenant_id,display_name,pin_hash,status,created_at) VALUES(?1,?2,?3,?4,'ACTIVE',?5)",params![id.to_string(),tenant.to_string(),name,pin_hash,now.to_rfc3339()])?;
        Ok(())
    }

    pub fn create_role(&self, id: Uuid, tenant: TenantId, name: &str) -> Result<(), StoreError> {
        self.conn.execute(
            "INSERT INTO roles(id,tenant_id,name) VALUES(?1,?2,?3)",
            params![id.to_string(), tenant.to_string(), name],
        )?;
        Ok(())
    }
    pub fn define_permission(&self, code: &str, description: &str) -> Result<(), StoreError> {
        self.conn.execute("INSERT INTO permissions(code,description) VALUES(?1,?2) ON CONFLICT(code) DO UPDATE SET description=excluded.description",params![code,description])?;
        Ok(())
    }
    pub fn grant_permission(&self, role_id: Uuid, code: &str) -> Result<(), StoreError> {
        self.conn.execute(
            "INSERT OR IGNORE INTO role_permissions(role_id,permission_code) VALUES(?1,?2)",
            params![role_id.to_string(), code],
        )?;
        Ok(())
    }
    pub fn assign_role(
        &self,
        user: UserId,
        role_id: Uuid,
        branch: Option<BranchId>,
    ) -> Result<(), StoreError> {
        self.conn.execute(
            "INSERT OR IGNORE INTO user_roles(user_id,role_id,branch_id) VALUES(?1,?2,?3)",
            params![
                user.to_string(),
                role_id.to_string(),
                branch.map(|b| b.to_string())
            ],
        )?;
        Ok(())
    }
    pub fn authenticate_pin(
        &self,
        tenant: TenantId,
        user: UserId,
        pin: &str,
        now: DateTime<Utc>,
        max_attempts: i64,
        lock_minutes: i64,
    ) -> Result<(), StoreError> {
        let row:Option<(String,String,i64,Option<String>)>=self.conn.query_row("SELECT pin_hash,status,failed_attempts,locked_until FROM users WHERE id=?1 AND tenant_id=?2",params![user.to_string(),tenant.to_string()],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional()?;
        if max_attempts < 1 || lock_minutes < 1 {
            return Err(StoreError::Validation("invalid lockout policy".into()));
        }
        let (hash, status, mut failed, locked_until) =
            row.ok_or(StoreError::Authorization("unknown user"))?;
        if status == "SUSPENDED" {
            return Err(StoreError::Authorization("user suspended"));
        }
        if status == "LOCKED" {
            if let Some(ts) = locked_until {
                if let Ok(until) = DateTime::parse_from_rfc3339(&ts) {
                    if now < until.with_timezone(&Utc) {
                        return Err(StoreError::Authorization("user locked"));
                    }
                }
            }
            self.conn.execute(
                "UPDATE users SET status='ACTIVE',failed_attempts=0,locked_until=NULL WHERE id=?1",
                params![user.to_string()],
            )?;
            failed = 0;
        }
        if verify_pin(&hash, pin) {
            self.conn.execute(
                "UPDATE users SET failed_attempts=0,locked_until=NULL,status='ACTIVE' WHERE id=?1",
                params![user.to_string()],
            )?;
            return Ok(());
        }
        let next = failed + 1;
        if next >= max_attempts {
            let until = now + chrono::Duration::minutes(lock_minutes);
            self.conn.execute(
                "UPDATE users SET failed_attempts=?2,status='LOCKED',locked_until=?3 WHERE id=?1",
                params![user.to_string(), next, until.to_rfc3339()],
            )?;
        } else {
            self.conn.execute(
                "UPDATE users SET failed_attempts=?2 WHERE id=?1",
                params![user.to_string(), next],
            )?;
        }
        Err(StoreError::Authorization("invalid PIN"))
    }
    pub fn consume_manager_approval(
        &mut self,
        secret: &[u8],
        binding: &ApprovalBinding,
        signature_hex: &str,
        required_permission: &str,
        now: DateTime<Utc>,
    ) -> Result<Uuid, StoreError> {
        if !verify_approval(secret, binding, signature_hex, now) {
            return Err(StoreError::Authorization(
                "invalid or expired manager approval",
            ));
        }
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let branch_s: Option<String> = tx
            .query_row(
                "SELECT branch_id FROM devices WHERE id=?1 AND tenant_id=?2 AND status='ACTIVE'",
                params![binding.device_id.to_string(), binding.tenant_id.to_string()],
                |r| r.get(0),
            )
            .optional()?;
        let branch_s = branch_s.ok_or(StoreError::Authorization("approval device not active"))?;
        let branch_uuid = Uuid::parse_str(&branch_s)
            .map_err(|_| StoreError::Validation("invalid branch identity".into()))?;
        let branch = BranchId(branch_uuid);
        Self::assert_user_scope(&tx, binding.tenant_id, binding.approver_user_id)?;
        Self::assert_permission(
            &tx,
            binding.tenant_id,
            branch,
            binding.approver_user_id,
            required_permission,
        )?;
        let used: Option<i32> = tx
            .query_row(
                "SELECT 1 FROM manager_approval_consumptions WHERE tenant_id=?1 AND nonce=?2",
                params![binding.tenant_id.to_string(), binding.nonce],
                |r| r.get(0),
            )
            .optional()?;
        if used.is_some() {
            return Err(StoreError::Conflict(
                "manager approval nonce already consumed".into(),
            ));
        }
        let id = Uuid::new_v4();
        let sig_hash = sha256_hex(signature_hex.as_bytes());
        tx.execute("INSERT INTO manager_approval_consumptions(id,tenant_id,approver_user_id,device_id,operation_id,action,entity_id,payload_sha256,nonce,signature_sha256,expires_at,consumed_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",params![id.to_string(),binding.tenant_id.to_string(),binding.approver_user_id.to_string(),binding.device_id.to_string(),binding.operation_id.to_string(),binding.action,binding.entity_id,binding.payload_sha256,binding.nonce,sig_hash,binding.expires_at.to_rfc3339(),now.to_rfc3339()])?;
        tx.commit()?;
        Ok(id)
    }
    pub fn rotate_local_device_credential(
        &mut self,
        context: LocalTerminalContext,
        user: UserId,
        new_secret: &str,
        now: DateTime<Utc>,
    ) -> Result<i64, StoreError> {
        self.validate_local_session(context, user)?;
        let new_hash =
            hash_secret(new_secret).map_err(|error| StoreError::Validation(error.to_string()))?;
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        Self::assert_permission(
            &tx,
            context.tenant_id,
            context.branch_id,
            user,
            "device.rotate",
        )?;
        let changed=tx.execute("UPDATE devices SET credential_hash=?2,credential_version=credential_version+1,last_heartbeat_at=?3 WHERE id=?1 AND tenant_id=?4 AND branch_id=?5 AND status='ACTIVE'",params![context.device_id.to_string(),new_hash,now.to_rfc3339(),context.tenant_id.to_string(),context.branch_id.to_string()])?;
        if changed != 1 {
            return Err(StoreError::Authorization(
                "active local device credential cannot be rotated",
            ));
        }
        let version: i64 = tx.query_row(
            "SELECT credential_version FROM devices WHERE id=?1",
            params![context.device_id.to_string()],
            |row| row.get(0),
        )?;
        Self::append_audit(
            &tx,
            context.tenant_id,
            context.device_id,
            user,
            "DEVICE_CREDENTIAL_ROTATED",
            "device",
            &context.device_id.to_string(),
            &serde_json::json!({"credential_version":version}).to_string(),
            now,
        )?;
        tx.commit()?;
        Ok(version)
    }
    pub fn issue_device_enrollment(
        &mut self,
        context: LocalTerminalContext,
        user: UserId,
        friendly_name: &str,
        valid_minutes: i64,
        now: DateTime<Utc>,
    ) -> Result<DeviceEnrollmentGrant, StoreError> {
        self.validate_local_session(context, user)?;
        Self::assert_permission_conn(
            &self.conn,
            context.tenant_id,
            context.branch_id,
            user,
            "device.enroll",
        )?;
        if friendly_name.trim().is_empty() {
            return Err(StoreError::Validation(
                "terminal friendly name is required".into(),
            ));
        }
        if !(5..=1440).contains(&valid_minutes) {
            return Err(StoreError::Validation(
                "enrollment validity must be between 5 and 1440 minutes".into(),
            ));
        }
        let grant_id = Uuid::new_v4();
        let enrollment_token = format!("{}{}", Uuid::new_v4(), Uuid::new_v4());
        let token_hash = hash_secret(&enrollment_token)
            .map_err(|error| StoreError::Validation(error.to_string()))?;
        let expires_at = now + chrono::Duration::minutes(valid_minutes);
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute("INSERT INTO device_enrollment_grants(id,tenant_id,branch_id,issued_from_device_id,issued_by_user_id,token_hash,friendly_name,expires_at,created_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)",params![grant_id.to_string(),context.tenant_id.to_string(),context.branch_id.to_string(),context.device_id.to_string(),user.to_string(),token_hash,friendly_name.trim(),expires_at.to_rfc3339(),now.to_rfc3339()])?;
        Self::append_audit(
            &tx,
            context.tenant_id,
            context.device_id,
            user,
            "DEVICE_ENROLLMENT_ISSUED",
            "device_enrollment_grant",
            &grant_id.to_string(),
            &serde_json::json!({"friendly_name":friendly_name.trim(),"expires_at":expires_at})
                .to_string(),
            now,
        )?;
        tx.commit()?;
        Ok(DeviceEnrollmentGrant {
            grant_id,
            enrollment_token,
            expires_at,
        })
    }

    pub fn activate_device_enrollment(
        &mut self,
        grant_id: Uuid,
        enrollment_token: &str,
        now: DateTime<Utc>,
    ) -> Result<DeviceEnrollmentResult, StoreError> {
        let row:Option<(String,String,String,String,String,String)>=self.conn.query_row("SELECT tenant_id,branch_id,issued_from_device_id,issued_by_user_id,token_hash,expires_at FROM device_enrollment_grants WHERE id=?1",params![grant_id.to_string()],|row|Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?,row.get(4)?,row.get(5)?))).optional()?;
        let (tenant, branch, issuer_device, issuer_user, token_hash, expires) =
            row.ok_or(StoreError::Authorization("unknown enrollment grant"))?;
        let expires_at = DateTime::parse_from_rfc3339(&expires)
            .map_err(|_| StoreError::Validation("invalid enrollment expiry".into()))?
            .with_timezone(&Utc);
        if now > expires_at || !verify_secret(&token_hash, enrollment_token) {
            return Err(StoreError::Authorization(
                "enrollment grant is expired or invalid",
            ));
        }
        let tenant_id = TenantId(
            Uuid::parse_str(&tenant)
                .map_err(|_| StoreError::Validation("invalid enrollment tenant".into()))?,
        );
        let branch_id = BranchId(
            Uuid::parse_str(&branch)
                .map_err(|_| StoreError::Validation("invalid enrollment branch".into()))?,
        );
        let issuer_device_id = DeviceId(
            Uuid::parse_str(&issuer_device)
                .map_err(|_| StoreError::Validation("invalid enrollment issuer device".into()))?,
        );
        let issuer_user_id = UserId(
            Uuid::parse_str(&issuer_user)
                .map_err(|_| StoreError::Validation("invalid enrollment issuer user".into()))?,
        );
        let device_id = DeviceId::new();
        let credential_secret = format!("{}{}", Uuid::new_v4(), Uuid::new_v4());
        let credential_hash = hash_secret(&credential_secret)
            .map_err(|error| StoreError::Validation(error.to_string()))?;
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let consumed: Option<i32> = tx
            .query_row(
                "SELECT 1 FROM device_enrollment_consumptions WHERE grant_id=?1",
                params![grant_id.to_string()],
                |row| row.get(0),
            )
            .optional()?;
        if consumed.is_some() {
            return Err(StoreError::Conflict(
                "enrollment grant already consumed".into(),
            ));
        }
        let friendly_name: String = tx.query_row(
            "SELECT friendly_name FROM device_enrollment_grants WHERE id=?1",
            params![grant_id.to_string()],
            |row| row.get(0),
        )?;
        tx.execute("INSERT INTO devices(id,tenant_id,branch_id,label,status,credential_hash,credential_version,created_at) VALUES(?1,?2,?3,?4,'ACTIVE',?5,1,?6)",params![device_id.to_string(),tenant,branch,friendly_name,credential_hash,now.to_rfc3339()])?;
        tx.execute("INSERT INTO device_enrollment_consumptions(grant_id,device_id,consumed_at) VALUES(?1,?2,?3)",params![grant_id.to_string(),device_id.to_string(),now.to_rfc3339()])?;
        Self::append_audit(
            &tx,
            tenant_id,
            issuer_device_id,
            issuer_user_id,
            "DEVICE_ENROLLED",
            "device",
            &device_id.to_string(),
            &serde_json::json!({"branch_id":branch_id,"credential_version":1}).to_string(),
            now,
        )?;
        tx.commit()?;
        Ok(DeviceEnrollmentResult {
            device_id,
            credential_secret,
            credential_version: 1,
        })
    }
    pub fn create_device(
        &self,
        id: DeviceId,
        tenant: TenantId,
        branch: BranchId,
        label: &str,
        credential_secret: &str,
        now: DateTime<Utc>,
    ) -> Result<(), StoreError> {
        let credential_hash =
            hash_secret(credential_secret).map_err(|e| StoreError::Validation(e.to_string()))?;
        self.conn.execute("INSERT INTO devices(id,tenant_id,branch_id,label,status,credential_hash,created_at) VALUES(?1,?2,?3,?4,'ACTIVE',?5,?6)",params![id.to_string(),tenant.to_string(),branch.to_string(),label,credential_hash,now.to_rfc3339()])?;
        Ok(())
    }
    pub fn authenticate_device(
        &self,
        tenant: TenantId,
        branch: BranchId,
        id: DeviceId,
        credential_secret: &str,
    ) -> Result<i64, StoreError> {
        let row:Option<(String,String,String,String,i64)>=self.conn.query_row("SELECT tenant_id,branch_id,status,credential_hash,credential_version FROM devices WHERE id=?1",params![id.to_string()],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?))).optional()?;
        let (t, b, status, hash, version) =
            row.ok_or(StoreError::Authorization("unknown device"))?;
        if t != tenant.to_string()
            || b != branch.to_string()
            || status != "ACTIVE"
            || !verify_secret(&hash, credential_secret)
        {
            return Err(StoreError::Authorization("device authentication failed"));
        }
        Ok(version)
    }
    pub fn set_device_status(
        &self,
        id: DeviceId,
        status: &str,
        now: DateTime<Utc>,
    ) -> Result<(), StoreError> {
        if !matches!(status, "ACTIVE" | "SUSPENDED" | "REVOKED") {
            return Err(StoreError::Validation("invalid device status".into()));
        }
        let current: Option<String> = self
            .conn
            .query_row(
                "SELECT status FROM devices WHERE id=?1",
                params![id.to_string()],
                |r| r.get(0),
            )
            .optional()?;
        let current = current.ok_or(StoreError::NotFound("device"))?;
        if current == "REVOKED" && status != "REVOKED" {
            return Err(StoreError::Conflict(
                "revoked device cannot be reactivated; enroll a new credential lifecycle".into(),
            ));
        }
        self.conn.execute("UPDATE devices SET status=?2, revoked_at=CASE WHEN ?2='REVOKED' THEN ?3 ELSE revoked_at END WHERE id=?1",params![id.to_string(),status,now.to_rfc3339()])?;
        Ok(())
    }
    pub fn create_register(
        &self,
        id: RegisterId,
        tenant: TenantId,
        branch: BranchId,
        code: &str,
        name: &str,
    ) -> Result<(), StoreError> {
        self.conn.execute(
            "INSERT INTO registers(id,tenant_id,branch_id,code,name) VALUES(?1,?2,?3,?4,?5)",
            params![
                id.to_string(),
                tenant.to_string(),
                branch.to_string(),
                code,
                name
            ],
        )?;
        Ok(())
    }
    pub fn create_inventory_centre(
        &self,
        id: Uuid,
        tenant: TenantId,
        branch: BranchId,
        code: &str,
        name: &str,
    ) -> Result<(), StoreError> {
        self.conn.execute("INSERT INTO inventory_centres(id,tenant_id,branch_id,code,name,centre_type) VALUES(?1,?2,?3,?4,?5,'SHOP_FLOOR')",params![id.to_string(),tenant.to_string(),branch.to_string(),code,name])?;
        Ok(())
    }
    #[allow(
        clippy::too_many_arguments,
        reason = "cash custody opening keeps every authoritative identity explicit"
    )]
    pub fn open_cash_session(
        &self,
        id: Uuid,
        tenant: TenantId,
        branch: BranchId,
        register: RegisterId,
        user: UserId,
        device: DeviceId,
        float: Money,
        now: DateTime<Utc>,
    ) -> Result<(), StoreError> {
        if float.0 < 0 {
            return Err(StoreError::Validation(
                "opening float cannot be negative".into(),
            ));
        }
        self.assert_active_device(&self.conn, tenant, branch, device)?;
        Self::assert_permission_conn(&self.conn, tenant, branch, user, "cash.session.open")?;
        let existing:Option<(String,String,String,String,String,i64,String)>=self.conn.query_row("SELECT tenant_id,branch_id,register_id,opened_by_user_id,opened_on_device_id,opening_float_fils,status FROM cash_sessions WHERE id=?1",params![id.to_string()],|row|Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?,row.get(4)?,row.get(5)?,row.get(6)?))).optional()?;
        if let Some((
            existing_tenant,
            existing_branch,
            existing_register,
            existing_user,
            existing_device,
            existing_float,
            status,
        )) = existing
        {
            if existing_tenant == tenant.to_string()
                && existing_branch == branch.to_string()
                && existing_register == register.to_string()
                && existing_user == user.to_string()
                && existing_device == device.to_string()
                && existing_float == float.0
                && status == "OPEN"
            {
                return Ok(());
            }
            return Err(StoreError::Conflict(
                "cash session ID reused with different scope, payload, or state".into(),
            ));
        }
        self.conn.execute("INSERT INTO cash_sessions(id,tenant_id,branch_id,register_id,opened_by_user_id,opened_on_device_id,opening_float_fils,status,opened_at) VALUES(?1,?2,?3,?4,?5,?6,?7,'OPEN',?8)",params![id.to_string(),tenant.to_string(),branch.to_string(),register.to_string(),user.to_string(),device.to_string(),float.0,now.to_rfc3339()])?;
        Ok(())
    }

    pub fn create_product(
        &self,
        id: ProductId,
        p: NewProduct<'_>,
        now: DateTime<Utc>,
    ) -> Result<(), StoreError> {
        if p.price.0 < 0 || p.cost.0 < 0 {
            return Err(StoreError::Validation(
                "product price and cost cannot be negative".into(),
            ));
        }
        TaxRule {
            category: TaxCategory::StandardRated,
            rate_bps: p.tax_rate_bps,
            inclusive: p.tax_inclusive,
        }
        .validate()?;
        let tx = self.conn.unchecked_transaction()?;
        tx.execute("INSERT INTO products(id,tenant_id,sku,name,base_price_fils,current_cost_fils,tax_rate_bps,tax_inclusive,track_inventory,allow_decimal_qty,created_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",params![id.to_string(),p.tenant_id.to_string(),p.sku,p.name,p.price.0,p.cost.0,p.tax_rate_bps,p.tax_inclusive as i32,p.track_inventory as i32,p.allow_decimal_qty as i32,now.to_rfc3339()])?;
        tx.execute("INSERT INTO product_barcodes(tenant_id,barcode,product_id,symbology,is_primary,created_at) VALUES(?1,?2,?3,'UNKNOWN',1,?4)",params![p.tenant_id.to_string(),p.barcode,id.to_string(),now.to_rfc3339()])?;
        tx.commit()?;
        Ok(())
    }
    pub fn set_branch_assortment(
        &self,
        tenant: TenantId,
        branch: BranchId,
        product: ProductId,
        status: &str,
        sellable: bool,
    ) -> Result<(), StoreError> {
        self.conn.execute("INSERT INTO branch_assortments(tenant_id,branch_id,product_id,status,sellable) VALUES(?1,?2,?3,?4,?5) ON CONFLICT(tenant_id,branch_id,product_id) DO UPDATE SET status=excluded.status,sellable=excluded.sellable",params![tenant.to_string(),branch.to_string(),product.to_string(),status,sellable as i32])?;
        Ok(())
    }
    pub fn create_cart(
        &self,
        id: CartId,
        tenant: TenantId,
        branch: BranchId,
        device: DeviceId,
        user: UserId,
        now: DateTime<Utc>,
    ) -> Result<(), StoreError> {
        self.assert_active_device(&self.conn, tenant, branch, device)?;
        self.conn.execute("INSERT INTO carts(id,tenant_id,branch_id,device_id,cashier_user_id,status,created_at,updated_at) VALUES(?1,?2,?3,?4,?5,'ACTIVE',?6,?6)",params![id.to_string(),tenant.to_string(),branch.to_string(),device.to_string(),user.to_string(),now.to_rfc3339()])?;
        Ok(())
    }

    pub fn add_barcode_to_cart(
        &self,
        tenant: TenantId,
        branch: BranchId,
        cart: CartId,
        barcode: &str,
        qty: QuantityMilli,
        now: DateTime<Utc>,
    ) -> Result<Uuid, StoreError> {
        if qty.0 <= 0 {
            return Err(StoreError::Validation("quantity must be > 0".into()));
        }
        let cart_scope: Option<(String, String, String)> = self
            .conn
            .query_row(
                "SELECT tenant_id,branch_id,status FROM carts WHERE id=?1",
                params![cart.to_string()],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()?;
        match cart_scope {
            Some((t, b, s))
                if t == tenant.to_string() && b == branch.to_string() && s == "ACTIVE" => {}
            Some(_) => return Err(StoreError::Authorization("cart scope/status mismatch")),
            None => return Err(StoreError::NotFound("cart")),
        }
        let row: Option<BarcodeProductRow> = self.conn.query_row(
            "SELECT p.id,p.name,p.sku,p.base_price_fils,p.current_cost_fils,p.tax_rate_bps,p.tax_inclusive,p.allow_decimal_qty FROM product_barcodes pb JOIN products p ON p.id=pb.product_id JOIN branch_assortments ba ON ba.product_id=p.id AND ba.tenant_id=p.tenant_id AND ba.branch_id=?2 WHERE pb.tenant_id=?1 AND pb.barcode=?3 AND p.status='ACTIVE' AND ba.sellable=1 LIMIT 1",
            params![tenant.to_string(),branch.to_string(),barcode],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?,r.get(7)?))).optional()?;
        let (pid, name, sku, base_price, cost, tax_bps, tax_incl, allow_decimal) = match row {
            Some(v) => v,
            None => {
                self.conn.execute("INSERT INTO unknown_barcodes(tenant_id,branch_id,barcode,first_seen_at,last_seen_at,last_device_id) SELECT tenant_id,branch_id,?2,?3,?3,device_id FROM carts WHERE id=?1 ON CONFLICT(tenant_id,branch_id,barcode) DO UPDATE SET last_seen_at=excluded.last_seen_at,scan_count=unknown_barcodes.scan_count+1,last_device_id=excluded.last_device_id",params![cart.to_string(),barcode,now.to_rfc3339()])?;
                return Err(StoreError::NotFound("barcode"));
            }
        };
        if allow_decimal == 0 && qty.0 % 1000 != 0 {
            return Err(StoreError::Validation(
                "product does not allow decimal quantity".into(),
            ));
        }
        let price: i64 = self.conn.query_row("SELECT price_fils FROM price_history WHERE tenant_id=?1 AND product_id=?2 AND channel='POS' AND (branch_id=?3 OR branch_id IS NULL) AND effective_from<=?4 AND (effective_to IS NULL OR effective_to>?4) ORDER BY CASE WHEN branch_id=?3 THEN 0 ELSE 1 END,effective_from DESC LIMIT 1",params![tenant.to_string(),pid,branch.to_string(),now.to_rfc3339()],|r|r.get(0)).optional()?.unwrap_or(base_price);
        let line = Uuid::new_v4();
        self.conn.execute("INSERT INTO cart_lines(id,cart_id,product_id,product_name_snapshot,sku_snapshot,barcode_snapshot,quantity_milli,unit_price_fils,unit_cost_fils,tax_category_snapshot,tax_rate_bps,tax_inclusive,created_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,'STANDARD',?10,?11,?12)",params![line.to_string(),cart.to_string(),pid,name,sku,barcode,qty.0,price,cost,tax_bps,tax_incl,now.to_rfc3339()])?;
        self.conn.execute(
            "UPDATE carts SET version=version+1,updated_at=?2 WHERE id=?1",
            params![cart.to_string(), now.to_rfc3339()],
        )?;
        Ok(line)
    }

    pub fn cart_snapshot(
        &self,
        context: LocalTerminalContext,
        user: UserId,
        cart: CartId,
    ) -> Result<CartSnapshot, StoreError> {
        self.validate_local_session(context, user)?;
        let cart_row:Option<(String,Option<String>)>=self.conn.query_row("SELECT status,note FROM carts WHERE id=?1 AND tenant_id=?2 AND branch_id=?3 AND device_id=?4 AND cashier_user_id=?5",params![cart.to_string(),context.tenant_id.to_string(),context.branch_id.to_string(),context.device_id.to_string(),user.to_string()],|row|Ok((row.get(0)?,row.get(1)?))).optional()?;
        let (status, note) = cart_row.ok_or(StoreError::Authorization(
            "cart does not belong to local session",
        ))?;
        if !matches!(status.as_str(), "ACTIVE" | "HELD") {
            return Err(StoreError::Conflict("cart is not viewable".into()));
        }
        let mut statement=self.conn.prepare("SELECT id,product_id,product_name_snapshot,sku_snapshot,barcode_snapshot,quantity_milli,unit_price_fils,tax_category_snapshot,tax_rate_bps,tax_inclusive FROM cart_lines WHERE cart_id=?1 ORDER BY created_at,id")?;
        let rows = statement.query_map(params![cart.to_string()], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, Option<String>>(4)?,
                row.get::<_, i64>(5)?,
                row.get::<_, i64>(6)?,
                row.get::<_, String>(7)?,
                row.get::<_, i32>(8)?,
                row.get::<_, i32>(9)?,
            ))
        })?;
        let mut lines = Vec::new();
        let mut subtotal = Money::ZERO;
        let mut tax = Money::ZERO;
        let mut total = Money::ZERO;
        for row in rows {
            let (
                id,
                product_id,
                name,
                sku,
                barcode,
                quantity,
                unit_price,
                tax_category,
                tax_rate,
                tax_inclusive,
            ) = row?;
            let basis = price_times_quantity(Money(unit_price), QuantityMilli(quantity))?;
            let rule = TaxRule {
                category: match tax_category.as_str() {
                    "ZERO" => TaxCategory::ZeroRated,
                    "EXEMPT" => TaxCategory::Exempt,
                    "OUT_OF_SCOPE" => TaxCategory::OutOfScope,
                    _ => TaxCategory::StandardRated,
                },
                rate_bps: tax_rate,
                inclusive: tax_inclusive != 0,
            };
            let breakdown = rule.calculate(basis)?;
            subtotal = subtotal.checked_add(breakdown.net)?;
            tax = tax.checked_add(breakdown.tax)?;
            total = total.checked_add(breakdown.gross)?;
            lines.push(CartLineSnapshot {
                id: Uuid::parse_str(&id)
                    .map_err(|_| StoreError::Validation("invalid cart line identity".into()))?,
                product_id: ProductId(
                    Uuid::parse_str(&product_id)
                        .map_err(|_| StoreError::Validation("invalid product identity".into()))?,
                ),
                name,
                sku,
                barcode,
                quantity: QuantityMilli(quantity),
                unit_price: Money(unit_price),
                net: breakdown.net,
                tax: breakdown.tax,
                gross: breakdown.gross,
            });
        }
        Ok(CartSnapshot {
            cart_id: cart,
            status,
            note,
            lines,
            candidate_subtotal: subtotal,
            candidate_tax: tax,
            candidate_total: total,
        })
    }

    pub fn hold_cart(
        &self,
        context: LocalTerminalContext,
        user: UserId,
        cart: CartId,
        note: Option<&str>,
        now: DateTime<Utc>,
    ) -> Result<(), StoreError> {
        self.validate_local_session(context, user)?;
        Self::assert_permission_conn(
            &self.conn,
            context.tenant_id,
            context.branch_id,
            user,
            "sale.hold",
        )?;
        let n=self.conn.execute("UPDATE carts SET status='HELD',note=?6,version=version+1,updated_at=?7 WHERE id=?1 AND tenant_id=?2 AND branch_id=?3 AND device_id=?4 AND cashier_user_id=?5 AND status='ACTIVE'",params![cart.to_string(),context.tenant_id.to_string(),context.branch_id.to_string(),context.device_id.to_string(),user.to_string(),note,now.to_rfc3339()])?;
        if n != 1 {
            return Err(StoreError::Conflict("cart cannot be held".into()));
        }
        Ok(())
    }
    pub fn held_carts(
        &self,
        context: LocalTerminalContext,
        user: UserId,
    ) -> Result<Vec<HeldCartSummary>, StoreError> {
        self.validate_local_session(context, user)?;
        Self::assert_permission_conn(
            &self.conn,
            context.tenant_id,
            context.branch_id,
            user,
            "sale.hold",
        )?;
        let mut statement=self.conn.prepare("SELECT id,note,updated_at FROM carts WHERE tenant_id=?1 AND branch_id=?2 AND cashier_user_id=?3 AND status='HELD' ORDER BY updated_at DESC,id LIMIT 100")?;
        let rows = statement.query_map(
            params![
                context.tenant_id.to_string(),
                context.branch_id.to_string(),
                user.to_string()
            ],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, String>(2)?,
                ))
            },
        )?;
        let raw = rows.collect::<Result<Vec<_>, _>>()?;
        raw.into_iter()
            .map(|(id, note, updated_at)| {
                Ok(HeldCartSummary {
                    cart_id: CartId(Uuid::parse_str(&id).map_err(|_| {
                        StoreError::Validation("invalid held cart identity".into())
                    })?),
                    note,
                    updated_at,
                })
            })
            .collect()
    }
    pub fn restore_cart(
        &self,
        context: LocalTerminalContext,
        user: UserId,
        cart: CartId,
        now: DateTime<Utc>,
    ) -> Result<(), StoreError> {
        self.validate_local_session(context, user)?;
        Self::assert_permission_conn(
            &self.conn,
            context.tenant_id,
            context.branch_id,
            user,
            "sale.hold",
        )?;
        let n=self.conn.execute("UPDATE carts SET status='ACTIVE',device_id=?4,version=version+1,updated_at=?6 WHERE id=?1 AND tenant_id=?2 AND branch_id=?3 AND cashier_user_id=?5 AND status='HELD'",params![cart.to_string(),context.tenant_id.to_string(),context.branch_id.to_string(),context.device_id.to_string(),user.to_string(),now.to_rfc3339()])?;
        if n != 1 {
            return Err(StoreError::Conflict("held cart cannot be restored".into()));
        }
        Ok(())
    }

    pub fn checkout(&mut self, req: CheckoutRequest) -> Result<CheckoutResult, StoreError> {
        let request_sha256 = Self::checkout_request_sha256(&req)?;
        if let Some(existing) = self.load_idempotent_result(
            req.tenant_id,
            req.operation_id,
            "CHECKOUT",
            &request_sha256,
        )? {
            return Ok(existing);
        }
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        Self::assert_active_device_tx(&tx, req.tenant_id, req.branch_id, req.device_id)?;
        Self::assert_user_scope(&tx, req.tenant_id, req.user_id)?;
        Self::assert_permission(
            &tx,
            req.tenant_id,
            req.branch_id,
            req.user_id,
            "sale.checkout",
        )?;
        let (cart_tenant,cart_branch,cart_device,cart_user,status,customer):(String,String,String,String,String,Option<String>)=tx.query_row("SELECT tenant_id,branch_id,device_id,cashier_user_id,status,customer_id FROM carts WHERE id=?1",params![req.cart_id.to_string()],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?))).optional()?.ok_or(StoreError::NotFound("cart"))?;
        if cart_tenant != req.tenant_id.to_string()
            || cart_branch != req.branch_id.to_string()
            || cart_device != req.device_id.to_string()
            || cart_user != req.user_id.to_string()
        {
            return Err(StoreError::Authorization(
                "cart actor/device scope mismatch",
            ));
        }
        if status != "ACTIVE" {
            return Err(StoreError::Conflict("cart is not active".into()));
        }
        let mut stmt=tx.prepare("SELECT product_id,product_name_snapshot,sku_snapshot,barcode_snapshot,quantity_milli,unit_price_fils,unit_cost_fils,tax_category_snapshot,tax_rate_bps,tax_inclusive FROM cart_lines WHERE cart_id=?1 ORDER BY created_at,id")?;
        let lines: Vec<CartLine> = stmt
            .query_map(params![req.cart_id.to_string()], |r| {
                Ok(CartLine {
                    product_id: r.get(0)?,
                    name: r.get(1)?,
                    sku: r.get(2)?,
                    barcode: r.get(3)?,
                    qty: r.get(4)?,
                    price: r.get(5)?,
                    cost: r.get(6)?,
                    tax_category: r.get(7)?,
                    tax_bps: r.get(8)?,
                    tax_inclusive: r.get::<_, i32>(9)? != 0,
                })
            })?
            .collect::<Result<_, _>>()?;
        drop(stmt);
        if lines.is_empty() {
            return Err(StoreError::Validation("empty cart".into()));
        }
        let mut computed = Vec::new();
        let mut subtotal = Money::ZERO;
        let mut tax_total = Money::ZERO;
        let mut total = Money::ZERO;
        let mut cogs = Money::ZERO;
        for l in &lines {
            let basis = price_times_quantity(Money(l.price), QuantityMilli(l.qty))?;
            let rule = TaxRule {
                category: match l.tax_category.as_str() {
                    "ZERO" => TaxCategory::ZeroRated,
                    "EXEMPT" => TaxCategory::Exempt,
                    "OUT_OF_SCOPE" => TaxCategory::OutOfScope,
                    _ => TaxCategory::StandardRated,
                },
                rate_bps: l.tax_bps,
                inclusive: l.tax_inclusive,
            };
            let b = rule.calculate(basis)?;
            subtotal = subtotal.checked_add(b.net)?;
            tax_total = tax_total.checked_add(b.tax)?;
            total = total.checked_add(b.gross)?;
            cogs = cogs.checked_add(price_times_quantity(Money(l.cost), QuantityMilli(l.qty))?)?;
            computed.push((l, b));
        }
        let applied_total = req
            .payments
            .iter()
            .try_fold(Money::ZERO, |sum, p| sum.checked_add(p.amount))?;
        if applied_total != total {
            return Err(StoreError::Validation(format!(
                "payments {} do not equal total {}",
                applied_total, total
            )));
        }
        let mut total_change = Money::ZERO;
        let needs_cash = req
            .payments
            .iter()
            .any(|p| matches!(p.kind, TenderKind::Cash));
        if needs_cash {
            let sid = req.cash_session_id.ok_or_else(|| {
                StoreError::Validation("cash payment requires open cash session".into())
            })?;
            let ok:Option<i32>=tx.query_row("SELECT 1 FROM cash_sessions WHERE id=?1 AND tenant_id=?2 AND branch_id=?3 AND register_id=?4 AND status='OPEN'",params![sid.to_string(),req.tenant_id.to_string(),req.branch_id.to_string(),req.register_id.to_string()],|r|r.get(0)).optional()?;
            if ok.is_none() {
                return Err(StoreError::Authorization(
                    "cash session scope/status mismatch",
                ));
            }
        }
        for p in &req.payments {
            if p.amount.0 <= 0 {
                return Err(StoreError::Validation(
                    "payment amount must be positive".into(),
                ));
            }
            if matches!(p.kind, TenderKind::Cash) {
                let tendered = p.tendered.unwrap_or(p.amount);
                if tendered.0 < p.amount.0 {
                    return Err(StoreError::Validation(
                        "cash tendered below applied amount".into(),
                    ));
                }
                total_change = total_change.checked_add(tendered.checked_sub(p.amount)?)?;
            }
        }
        let credit_amount = req
            .payments
            .iter()
            .filter(|payment| matches!(payment.kind, TenderKind::CustomerCredit))
            .try_fold(Money::ZERO, |sum, payment| sum.checked_add(payment.amount))?;
        let credit_context = if credit_amount.0 > 0 {
            let customer_id = customer.as_ref().ok_or_else(|| {
                StoreError::Validation("customer credit tender requires a customer".into())
            })?;
            let account:Option<(i64,Option<i64>,String)>=tx.query_row("SELECT credit_limit_fils,payment_terms_days,status FROM customer_credit_accounts WHERE tenant_id=?1 AND customer_id=?2",params![req.tenant_id.to_string(),customer_id],|row|Ok((row.get(0)?,row.get(1)?,row.get(2)?))).optional()?;
            let (limit, terms, status) =
                account.ok_or(StoreError::NotFound("customer credit account"))?;
            if status != "ACTIVE" {
                return Err(StoreError::Authorization(
                    "customer credit account is not active",
                ));
            }
            let outstanding:i64=tx.query_row("SELECT COALESCE(SUM(debit_fils-credit_fils),0) FROM customer_credit_ledger WHERE tenant_id=?1 AND customer_id=?2",params![req.tenant_id.to_string(),customer_id],|row|row.get(0))?;
            if outstanding
                .checked_add(credit_amount.0)
                .ok_or(bhaipos_core::MoneyError::Overflow)?
                > limit
            {
                return Err(StoreError::Conflict(
                    "customer credit limit exceeded".into(),
                ));
            }
            Some((customer_id.clone(), terms))
        } else {
            None
        };
        let (business_date, branch_code, _close_hour) =
            Self::business_date_for(&tx, req.tenant_id, req.branch_id, req.now)?;
        let sequence =
            Self::next_receipt_sequence(&tx, req.tenant_id, req.branch_id, &business_date)?;
        let receipt_number = format!(
            "{}-{}-{:06}",
            branch_code,
            business_date.replace('-', ""),
            sequence
        );
        let sale_id = SaleId::new();
        tx.execute("INSERT INTO sales(id,tenant_id,branch_id,device_id,register_id,cash_session_id,cart_id,cashier_user_id,customer_id,operation_id,receipt_number,business_date,subtotal_fils,tax_fils,total_fils,cogs_fils,completed_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17)",params![sale_id.to_string(),req.tenant_id.to_string(),req.branch_id.to_string(),req.device_id.to_string(),req.register_id.to_string(),req.cash_session_id.map(|x|x.to_string()),req.cart_id.to_string(),req.user_id.to_string(),customer,req.operation_id.to_string(),receipt_number,business_date,subtotal.0,tax_total.0,total.0,cogs.0,req.now.to_rfc3339()])?;
        let centre_id:String=tx.query_row("SELECT id FROM inventory_centres WHERE tenant_id=?1 AND branch_id=?2 ORDER BY CASE WHEN centre_type='SHOP_FLOOR' THEN 0 ELSE 1 END LIMIT 1",params![req.tenant_id.to_string(),req.branch_id.to_string()],|r|r.get(0)).optional()?.ok_or(StoreError::NotFound("inventory centre"))?;
        for (l, b) in computed {
            let sale_line_id = Uuid::new_v4();
            tx.execute("INSERT INTO sale_lines(id,sale_id,product_id,product_name_snapshot,sku_snapshot,barcode_snapshot,quantity_milli,unit_price_fils,unit_cost_fils,net_fils,tax_fils,gross_fils,tax_category_snapshot,tax_rate_bps,tax_inclusive) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15)",params![sale_line_id.to_string(),sale_id.to_string(),l.product_id,l.name,l.sku,l.barcode,l.qty,l.price,l.cost,b.net.0,b.tax.0,b.gross.0,l.tax_category,l.tax_bps,l.tax_inclusive as i32])?;
            let track: i32 = tx.query_row(
                "SELECT track_inventory FROM products WHERE id=?1 AND tenant_id=?2",
                params![l.product_id, req.tenant_id.to_string()],
                |r| r.get(0),
            )?;
            if track != 0 {
                let changed=tx.execute("UPDATE stock_levels SET quantity_milli=quantity_milli-?5,version=version+1 WHERE tenant_id=?1 AND branch_id=?2 AND centre_id=?3 AND product_id=?4",params![req.tenant_id.to_string(),req.branch_id.to_string(),centre_id,l.product_id,l.qty])?;
                if changed == 0 {
                    tx.execute("INSERT INTO stock_levels(tenant_id,branch_id,centre_id,product_id,quantity_milli) VALUES(?1,?2,?3,?4,?5)",params![req.tenant_id.to_string(),req.branch_id.to_string(),centre_id,l.product_id,-l.qty])?;
                }
                tx.execute("INSERT INTO inventory_movements(id,tenant_id,branch_id,centre_id,product_id,operation_id,movement_type,quantity_milli,unit_cost_fils,source_type,source_id,device_id,user_id,created_at) VALUES(?1,?2,?3,?4,?5,?6,'SALE',?7,?8,'SALE',?9,?10,?11,?12)",params![Uuid::new_v4().to_string(),req.tenant_id.to_string(),req.branch_id.to_string(),centre_id,l.product_id,req.operation_id.to_string(),-l.qty,l.cost,sale_id.to_string(),req.device_id.to_string(),req.user_id.to_string(),req.now.to_rfc3339()])?;
                Self::apply_cost_projection(
                    &tx,
                    req.tenant_id,
                    req.branch_id,
                    &centre_id,
                    &l.product_id,
                    QuantityMilli(-l.qty),
                    Money(l.cost),
                )?;
            }
        }
        for p in &req.payments {
            let tendered = p.tendered.map(|m| m.0);
            let change = if matches!(p.kind, TenderKind::Cash) {
                p.tendered.unwrap_or(p.amount).0 - p.amount.0
            } else {
                0
            };
            tx.execute("INSERT INTO sale_payments(id,sale_id,tender_kind,amount_fils,tendered_fils,change_fils,reference,evidence_status,created_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)",params![Uuid::new_v4().to_string(),sale_id.to_string(),format!("{:?}",p.kind).to_uppercase(),p.amount.0,tendered,change,p.reference,if matches!(p.kind,TenderKind::BenefitPay){"RECORDED_NOT_SETTLED"}else{"RECORDED"},req.now.to_rfc3339()])?;
        }
        if let Some((customer_id, terms)) = credit_context {
            let due_date = terms.map(|days| {
                (req.now + chrono::Duration::days(days))
                    .format("%Y-%m-%d")
                    .to_string()
            });
            tx.execute("INSERT INTO customer_credit_ledger(id,tenant_id,customer_id,event_type,source_type,source_id,debit_fils,credit_fils,due_date,operation_id,created_at) VALUES(?1,?2,?3,'SALE','SALE',?4,?5,0,?6,?7,?8)",params![Uuid::new_v4().to_string(),req.tenant_id.to_string(),customer_id,sale_id.to_string(),credit_amount.0,due_date,req.operation_id.to_string(),req.now.to_rfc3339()])?;
        }
        let receipt = Self::render_receipt(
            &tx,
            sale_id,
            &receipt_number,
            subtotal,
            tax_total,
            total,
            total_change,
        )?;
        let receipt_hash = sha256_hex(receipt.as_bytes());
        tx.execute("INSERT INTO receipt_snapshots(sale_id,format_version,receipt_text,receipt_sha256,created_at) VALUES(?1,1,?2,?3,?4)",params![sale_id.to_string(),receipt,receipt_hash,req.now.to_rfc3339()])?;
        tx.execute("INSERT INTO print_jobs(id,tenant_id,branch_id,device_id,sale_id,document_type,snapshot_sha256,state,attempts,created_at,updated_at) VALUES(?1,?2,?3,?4,?5,'SALE_RECEIPT',?6,'PENDING',0,?7,?7)",params![Uuid::new_v4().to_string(),req.tenant_id.to_string(),req.branch_id.to_string(),req.device_id.to_string(),sale_id.to_string(),receipt_hash,req.now.to_rfc3339()])?;
        tx.execute(
            "UPDATE carts SET status='COMPLETED',version=version+1,updated_at=?2 WHERE id=?1",
            params![req.cart_id.to_string(), req.now.to_rfc3339()],
        )?;
        let result = CheckoutResult {
            sale_id,
            receipt_number,
            subtotal,
            tax: tax_total,
            total,
            change: total_change,
            receipt_sha256: receipt_hash,
        };
        let payload = serde_json::to_string(&result)?;
        Self::append_audit(
            &tx,
            req.tenant_id,
            req.device_id,
            req.user_id,
            "SALE_COMPLETED",
            "sale",
            &sale_id.to_string(),
            &payload,
            req.now,
        )?;
        tx.execute("INSERT INTO sync_queue(id,tenant_id,branch_id,device_id,operation_id,entity_type,entity_id,mutation_type,payload_json,state,created_at,updated_at) VALUES(?1,?2,?3,?4,?5,'sale',?6,'UPSERT',?7,'PENDING',?8,?8)",params![Uuid::new_v4().to_string(),req.tenant_id.to_string(),req.branch_id.to_string(),req.device_id.to_string(),req.operation_id.to_string(),sale_id.to_string(),payload,req.now.to_rfc3339()])?;
        Self::record_idempotent_result(
            &tx,
            req.tenant_id,
            req.operation_id,
            "CHECKOUT",
            &request_sha256,
            &result,
            req.now,
        )?;
        tx.commit()?;
        Ok(result)
    }

    pub fn refund(&mut self, req: RefundRequest) -> Result<RefundResult, StoreError> {
        let request_sha256 = Self::refund_request_sha256(&req)?;
        if let Some(existing) =
            self.load_idempotent_result(req.tenant_id, req.operation_id, "REFUND", &request_sha256)?
        {
            return Ok(existing);
        }
        if req.lines.is_empty() {
            return Err(StoreError::Validation("refund has no lines".into()));
        }
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        Self::assert_active_device_tx(&tx, req.tenant_id, req.branch_id, req.device_id)?;
        Self::assert_user_scope(&tx, req.tenant_id, req.user_id)?;
        Self::assert_permission(
            &tx,
            req.tenant_id,
            req.branch_id,
            req.user_id,
            "sale.refund",
        )?;
        let sale_scope: Option<(String, String, String)> = tx
            .query_row(
                "SELECT tenant_id,branch_id,status FROM sales WHERE id=?1",
                params![req.sale_id.to_string()],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()?;
        match sale_scope {
            Some((t, b, status))
                if t == req.tenant_id.to_string()
                    && b == req.branch_id.to_string()
                    && status == "COMPLETED" => {}
            Some((t, b, _)) if t == req.tenant_id.to_string() && b == req.branch_id.to_string() => {
                return Err(StoreError::Conflict(
                    "only completed sales can be refunded".into(),
                ))
            }
            Some(_) => return Err(StoreError::Authorization("sale tenant/branch mismatch")),
            None => return Err(StoreError::NotFound("sale")),
        }
        let centre_id:String=tx.query_row("SELECT id FROM inventory_centres WHERE tenant_id=?1 AND branch_id=?2 ORDER BY CASE WHEN centre_type='SHOP_FLOOR' THEN 0 ELSE 1 END LIMIT 1",params![req.tenant_id.to_string(),req.branch_id.to_string()],|r|r.get(0))?;
        let refund_id = Uuid::new_v4();
        let mut subtotal = Money::ZERO;
        let mut tax = Money::ZERO;
        let mut total = Money::ZERO;
        let mut prepared: Vec<PreparedRefundLine> = Vec::new();
        let mut requested_line_ids = HashSet::new();
        for rli in &req.lines {
            if rli.quantity.0 <= 0 {
                return Err(StoreError::Validation("refund quantity must be > 0".into()));
            }
            if !requested_line_ids.insert(rli.sale_line_id) {
                return Err(StoreError::Validation(
                    "refund request contains duplicate sale line".into(),
                ));
            }
            let row:Option<(String,i64,i64,i64,i32,i32,String)>=tx.query_row(
                "SELECT product_id,quantity_milli,unit_price_fils,unit_cost_fils,tax_rate_bps,tax_inclusive,tax_category_snapshot FROM sale_lines WHERE id=?1 AND sale_id=?2",
                params![rli.sale_line_id.to_string(),req.sale_id.to_string()],
                |r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?))
            ).optional()?;
            let (product_id, sold_qty, unit_price, unit_cost, tax_bps, tax_inclusive, tax_category) =
                row.ok_or(StoreError::NotFound("sale line"))?;
            let already:i64=tx.query_row("SELECT COALESCE(SUM(rl.quantity_milli),0) FROM refund_lines rl JOIN refunds r ON r.id=rl.refund_id WHERE rl.sale_line_id=?1",params![rli.sale_line_id.to_string()],|rr|rr.get(0))?;
            if already
                .checked_add(rli.quantity.0)
                .ok_or(bhaipos_core::MoneyError::Overflow)?
                > sold_qty
            {
                return Err(StoreError::Validation(
                    "refund exceeds remaining refundable quantity".into(),
                ));
            }
            let basis = price_times_quantity(Money(unit_price), rli.quantity)?;
            let rule = TaxRule {
                category: match tax_category.as_str() {
                    "ZERO" => TaxCategory::ZeroRated,
                    "EXEMPT" => TaxCategory::Exempt,
                    "OUT_OF_SCOPE" => TaxCategory::OutOfScope,
                    _ => TaxCategory::StandardRated,
                },
                rate_bps: tax_bps,
                inclusive: tax_inclusive != 0,
            };
            let b = rule.calculate(basis)?;
            subtotal = subtotal.checked_add(b.net)?;
            tax = tax.checked_add(b.tax)?;
            total = total.checked_add(b.gross)?;
            prepared.push(PreparedRefundLine {
                sale_line_id: rli.sale_line_id,
                product_id,
                quantity: rli.quantity,
                unit_cost,
                net: b.net,
                tax: b.tax,
                gross: b.gross,
            });
        }
        if total.0 > 0 && req.payments.is_empty() {
            return Err(StoreError::Validation(
                "non-zero refund requires payment effect".into(),
            ));
        }
        let payment_total = req
            .payments
            .iter()
            .try_fold(Money::ZERO, |sum, p| sum.checked_add(p.amount))?;
        if payment_total != total {
            return Err(StoreError::Validation(format!(
                "refund payments {} do not equal refund total {}",
                payment_total, total
            )));
        }
        for p in &req.payments {
            if p.amount.0 <= 0 {
                return Err(StoreError::Validation(
                    "refund payment must be positive".into(),
                ));
            }
        }
        let has_cash = req
            .payments
            .iter()
            .any(|p| matches!(p.kind, TenderKind::Cash));
        if has_cash {
            let sid = req.cash_session_id.ok_or_else(|| {
                StoreError::Validation("cash refund requires open cash session".into())
            })?;
            let ok:Option<i32>=tx.query_row("SELECT 1 FROM cash_sessions WHERE id=?1 AND tenant_id=?2 AND branch_id=?3 AND status='OPEN'",params![sid.to_string(),req.tenant_id.to_string(),req.branch_id.to_string()],|r|r.get(0)).optional()?;
            if ok.is_none() {
                return Err(StoreError::Authorization(
                    "refund cash session scope/status mismatch",
                ));
            }
        }
        tx.execute("INSERT INTO refunds(id,tenant_id,branch_id,sale_id,operation_id,device_id,user_id,reason,subtotal_fils,tax_fils,total_fils,created_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",params![refund_id.to_string(),req.tenant_id.to_string(),req.branch_id.to_string(),req.sale_id.to_string(),req.operation_id.to_string(),req.device_id.to_string(),req.user_id.to_string(),req.reason,subtotal.0,tax.0,total.0,req.now.to_rfc3339()])?;
        for pl in prepared {
            tx.execute("INSERT INTO refund_lines(id,refund_id,sale_line_id,quantity_milli,net_fils,tax_fils,gross_fils) VALUES(?1,?2,?3,?4,?5,?6,?7)",params![Uuid::new_v4().to_string(),refund_id.to_string(),pl.sale_line_id.to_string(),pl.quantity.0,pl.net.0,pl.tax.0,pl.gross.0])?;
            let changed=tx.execute("UPDATE stock_levels SET quantity_milli=quantity_milli+?5,version=version+1 WHERE tenant_id=?1 AND branch_id=?2 AND centre_id=?3 AND product_id=?4",params![req.tenant_id.to_string(),req.branch_id.to_string(),centre_id,pl.product_id,pl.quantity.0])?;
            if changed == 0 {
                tx.execute("INSERT INTO stock_levels(tenant_id,branch_id,centre_id,product_id,quantity_milli) VALUES(?1,?2,?3,?4,?5)",params![req.tenant_id.to_string(),req.branch_id.to_string(),centre_id,pl.product_id,pl.quantity.0])?;
            }
            tx.execute("INSERT INTO inventory_movements(id,tenant_id,branch_id,centre_id,product_id,operation_id,movement_type,quantity_milli,unit_cost_fils,source_type,source_id,device_id,user_id,created_at) VALUES(?1,?2,?3,?4,?5,?6,'REFUND',?7,?8,'REFUND',?9,?10,?11,?12)",params![Uuid::new_v4().to_string(),req.tenant_id.to_string(),req.branch_id.to_string(),centre_id,pl.product_id,req.operation_id.to_string(),pl.quantity.0,pl.unit_cost,refund_id.to_string(),req.device_id.to_string(),req.user_id.to_string(),req.now.to_rfc3339()])?;
            Self::apply_cost_projection(
                &tx,
                req.tenant_id,
                req.branch_id,
                &centre_id,
                &pl.product_id,
                pl.quantity,
                Money(pl.unit_cost),
            )?;
        }
        for p in &req.payments {
            tx.execute("INSERT INTO refund_payments(id,refund_id,tender_kind,amount_fils,reference,created_at,cash_session_id,device_id,user_id) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)",params![Uuid::new_v4().to_string(),refund_id.to_string(),format!("{:?}",p.kind).to_uppercase(),p.amount.0,p.reference,req.now.to_rfc3339(),if matches!(p.kind,TenderKind::Cash){req.cash_session_id.map(|x|x.to_string())}else{None},req.device_id.to_string(),req.user_id.to_string()])?;
        }
        let result = RefundResult {
            refund_id,
            subtotal,
            tax,
            total,
        };
        let payload = serde_json::to_string(&result)?;
        Self::append_audit(
            &tx,
            req.tenant_id,
            req.device_id,
            req.user_id,
            "SALE_REFUNDED",
            "refund",
            &refund_id.to_string(),
            &payload,
            req.now,
        )?;
        tx.execute("INSERT INTO sync_queue(id,tenant_id,branch_id,device_id,operation_id,entity_type,entity_id,mutation_type,payload_json,state,created_at,updated_at) VALUES(?1,?2,?3,?4,?5,'refund',?6,'INSERT',?7,'PENDING',?8,?8)",params![Uuid::new_v4().to_string(),req.tenant_id.to_string(),req.branch_id.to_string(),req.device_id.to_string(),req.operation_id.to_string(),refund_id.to_string(),payload,req.now.to_rfc3339()])?;
        Self::record_idempotent_result(
            &tx,
            req.tenant_id,
            req.operation_id,
            "REFUND",
            &request_sha256,
            &result,
            req.now,
        )?;
        tx.commit()?;
        Ok(result)
    }

    pub fn find_refundable_sale(
        &self,
        context: LocalTerminalContext,
        user: UserId,
        receipt_number: &str,
    ) -> Result<RefundableSale, StoreError> {
        self.validate_local_session(context, user)?;
        Self::assert_permission_conn(
            &self.conn,
            context.tenant_id,
            context.branch_id,
            user,
            "sale.refund",
        )?;
        let sale:Option<(String,String,String)>=self.conn.query_row(
            "SELECT id,receipt_number,completed_at FROM sales WHERE tenant_id=?1 AND branch_id=?2 AND receipt_number=?3 AND status='COMPLETED'",
            params![context.tenant_id.to_string(),context.branch_id.to_string(),receipt_number.trim()],
            |row|Ok((row.get(0)?,row.get(1)?,row.get(2)?))
        ).optional()?;
        let (sale_id, receipt_number, completed_at) =
            sale.ok_or(StoreError::NotFound("completed sale receipt"))?;
        let mut statement=self.conn.prepare(
            "SELECT sl.id,sl.product_name_snapshot,sl.quantity_milli,sl.unit_price_fils,sl.tax_rate_bps,sl.tax_inclusive,sl.tax_category_snapshot,COALESCE((SELECT SUM(rl.quantity_milli) FROM refund_lines rl WHERE rl.sale_line_id=sl.id),0) FROM sale_lines sl WHERE sl.sale_id=?1 ORDER BY sl.rowid"
        )?;
        let raw = statement
            .query_map(params![sale_id], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, i32>(4)?,
                    row.get::<_, i32>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, i64>(7)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        let mut lines = Vec::new();
        for (line_id, name, sold, unit_price, tax_bps, tax_inclusive, tax_category, refunded) in raw
        {
            let remaining = sold.checked_sub(refunded).ok_or_else(|| {
                StoreError::Conflict("refunded quantity exceeds sold quantity".into())
            })?;
            let basis = price_times_quantity(Money(unit_price), QuantityMilli(remaining))?;
            let rule = TaxRule {
                category: Self::tax_category_from_snapshot(&tax_category),
                rate_bps: tax_bps,
                inclusive: tax_inclusive != 0,
            };
            lines.push(RefundableSaleLine {
                sale_line_id: Uuid::parse_str(&line_id)
                    .map_err(|_| StoreError::Validation("invalid sale line identity".into()))?,
                product_name: name,
                sold_quantity: QuantityMilli(sold),
                refunded_quantity: QuantityMilli(refunded),
                remaining_quantity: QuantityMilli(remaining),
                refundable_total: rule.calculate(basis)?.gross,
            });
        }
        Ok(RefundableSale {
            sale_id: SaleId(
                Uuid::parse_str(&sale_id)
                    .map_err(|_| StoreError::Validation("invalid sale identity".into()))?,
            ),
            receipt_number,
            completed_at,
            lines,
        })
    }

    pub fn quote_refund(
        &self,
        context: LocalTerminalContext,
        user: UserId,
        sale_id: SaleId,
        lines: &[RefundLineInput],
    ) -> Result<RefundQuote, StoreError> {
        self.validate_local_session(context, user)?;
        Self::assert_permission_conn(
            &self.conn,
            context.tenant_id,
            context.branch_id,
            user,
            "sale.refund",
        )?;
        if lines.is_empty() {
            return Err(StoreError::Validation("refund has no lines".into()));
        }
        let sale:Option<i32>=self.conn.query_row("SELECT 1 FROM sales WHERE id=?1 AND tenant_id=?2 AND branch_id=?3 AND status='COMPLETED'",params![sale_id.to_string(),context.tenant_id.to_string(),context.branch_id.to_string()],|row|row.get(0)).optional()?;
        if sale.is_none() {
            return Err(StoreError::NotFound("completed sale"));
        }
        let mut seen = HashSet::new();
        let mut subtotal = Money::ZERO;
        let mut tax = Money::ZERO;
        let mut total = Money::ZERO;
        for requested in lines {
            if requested.quantity.0 <= 0 {
                return Err(StoreError::Validation("refund quantity must be > 0".into()));
            }
            if !seen.insert(requested.sale_line_id) {
                return Err(StoreError::Validation(
                    "refund request contains duplicate sale line".into(),
                ));
            }
            let row:Option<(i64,i64,i32,i32,String)>=self.conn.query_row(
                "SELECT quantity_milli,unit_price_fils,tax_rate_bps,tax_inclusive,tax_category_snapshot FROM sale_lines WHERE id=?1 AND sale_id=?2",
                params![requested.sale_line_id.to_string(),sale_id.to_string()],|row|Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?,row.get(4)?))
            ).optional()?;
            let (sold, unit_price, tax_bps, tax_inclusive, tax_category) =
                row.ok_or(StoreError::NotFound("sale line"))?;
            let refunded: i64 = self.conn.query_row(
                "SELECT COALESCE(SUM(quantity_milli),0) FROM refund_lines WHERE sale_line_id=?1",
                params![requested.sale_line_id.to_string()],
                |row| row.get(0),
            )?;
            if refunded
                .checked_add(requested.quantity.0)
                .ok_or(bhaipos_core::MoneyError::Overflow)?
                > sold
            {
                return Err(StoreError::Validation(
                    "refund exceeds remaining refundable quantity".into(),
                ));
            }
            let basis = price_times_quantity(Money(unit_price), requested.quantity)?;
            let calculated = TaxRule {
                category: Self::tax_category_from_snapshot(&tax_category),
                rate_bps: tax_bps,
                inclusive: tax_inclusive != 0,
            }
            .calculate(basis)?;
            subtotal = subtotal.checked_add(calculated.net)?;
            tax = tax.checked_add(calculated.tax)?;
            total = total.checked_add(calculated.gross)?;
        }
        Ok(RefundQuote {
            subtotal,
            tax,
            total,
        })
    }

    pub fn record_cash_movement(
        &mut self,
        req: CashMovementRequest,
    ) -> Result<CashMovementResult, StoreError> {
        let request_sha256 = Self::cash_movement_request_sha256(&req)?;
        if let Some(existing) = self.load_idempotent_result(
            req.tenant_id,
            req.operation_id,
            "CASH_MOVEMENT",
            &request_sha256,
        )? {
            return Ok(existing);
        }
        match req.kind {
            CashMovementKind::NoSale if req.amount.0 != 0 => {
                return Err(StoreError::Validation(
                    "no-sale movement amount must be zero".into(),
                ))
            }
            CashMovementKind::NoSale => {}
            _ if req.amount.0 <= 0 => {
                return Err(StoreError::Validation(
                    "cash movement amount must be positive".into(),
                ))
            }
            _ => {}
        }
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        Self::assert_active_device_tx(&tx, req.tenant_id, req.branch_id, req.device_id)?;
        Self::assert_user_scope(&tx, req.tenant_id, req.user_id)?;
        Self::assert_permission(
            &tx,
            req.tenant_id,
            req.branch_id,
            req.user_id,
            req.kind.permission(),
        )?;
        let ok:Option<i32>=tx.query_row("SELECT 1 FROM cash_sessions WHERE id=?1 AND tenant_id=?2 AND branch_id=?3 AND status='OPEN'",params![req.cash_session_id.to_string(),req.tenant_id.to_string(),req.branch_id.to_string()],|r|r.get(0)).optional()?;
        if ok.is_none() {
            return Err(StoreError::Authorization(
                "cash session scope/status mismatch",
            ));
        }
        let movement_id = Uuid::new_v4();
        tx.execute("INSERT INTO cash_movements(id,tenant_id,branch_id,cash_session_id,device_id,user_id,operation_id,kind,amount_fils,reason,created_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",params![movement_id.to_string(),req.tenant_id.to_string(),req.branch_id.to_string(),req.cash_session_id.to_string(),req.device_id.to_string(),req.user_id.to_string(),req.operation_id.to_string(),req.kind.as_db(),req.amount.0,req.reason,req.now.to_rfc3339()])?;
        let result = CashMovementResult {
            movement_id,
            kind: req.kind,
            amount: req.amount,
        };
        let payload = serde_json::to_string(&result)?;
        Self::append_audit(
            &tx,
            req.tenant_id,
            req.device_id,
            req.user_id,
            "CASH_MOVEMENT_RECORDED",
            "cash_movement",
            &movement_id.to_string(),
            &payload,
            req.now,
        )?;
        tx.execute("INSERT INTO sync_queue(id,tenant_id,branch_id,device_id,operation_id,entity_type,entity_id,mutation_type,payload_json,state,created_at,updated_at) VALUES(?1,?2,?3,?4,?5,'cash_movement',?6,'INSERT',?7,'PENDING',?8,?8)",params![Uuid::new_v4().to_string(),req.tenant_id.to_string(),req.branch_id.to_string(),req.device_id.to_string(),req.operation_id.to_string(),movement_id.to_string(),payload,req.now.to_rfc3339()])?;
        Self::record_idempotent_result(
            &tx,
            req.tenant_id,
            req.operation_id,
            "CASH_MOVEMENT",
            &request_sha256,
            &result,
            req.now,
        )?;
        tx.commit()?;
        Ok(result)
    }

    pub fn cash_session_report(
        &self,
        tenant: TenantId,
        branch: BranchId,
        cash_session_id: Uuid,
    ) -> Result<CashSessionReport, StoreError> {
        Self::cash_session_report_conn(&self.conn, tenant, branch, cash_session_id)
    }

    pub fn close_cash_session(
        &mut self,
        req: CloseCashSessionRequest,
    ) -> Result<CloseCashSessionResult, StoreError> {
        if req.counted_cash.0 < 0 {
            return Err(StoreError::Validation(
                "counted cash cannot be negative".into(),
            ));
        }
        let request_sha256 = Self::close_cash_session_request_sha256(&req)?;
        if let Some(existing) = self.load_idempotent_result(
            req.tenant_id,
            req.operation_id,
            "CASH_SESSION_CLOSE",
            &request_sha256,
        )? {
            return Ok(existing);
        }
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        Self::assert_active_device_tx(&tx, req.tenant_id, req.branch_id, req.device_id)?;
        Self::assert_user_scope(&tx, req.tenant_id, req.user_id)?;
        Self::assert_permission(
            &tx,
            req.tenant_id,
            req.branch_id,
            req.user_id,
            "cash.session.close",
        )?;
        let status: Option<String> = tx
            .query_row(
                "SELECT status FROM cash_sessions WHERE id=?1 AND tenant_id=?2 AND branch_id=?3",
                params![
                    req.cash_session_id.to_string(),
                    req.tenant_id.to_string(),
                    req.branch_id.to_string()
                ],
                |r| r.get(0),
            )
            .optional()?;
        match status.as_deref() {
            Some("OPEN") => {}
            Some(_) => return Err(StoreError::Conflict("cash session is not open".into())),
            None => return Err(StoreError::NotFound("cash session")),
        }
        let pre =
            Self::cash_session_report_conn(&tx, req.tenant_id, req.branch_id, req.cash_session_id)?;
        let variance = req.counted_cash.checked_sub(pre.expected_cash)?;
        tx.execute("UPDATE cash_sessions SET status='CLOSED',closed_at=?2,counted_cash_fils=?3,expected_cash_fils=?4,variance_fils=?5 WHERE id=?1 AND status='OPEN'",params![req.cash_session_id.to_string(),req.now.to_rfc3339(),req.counted_cash.0,pre.expected_cash.0,variance.0])?;
        let variance_case_id = if variance.0 != 0 {
            let id = Uuid::new_v4();
            let abs = variance.checked_abs()?.0;
            let severity = if abs < 5_000 {
                "LOW"
            } else if abs < 20_000 {
                "MEDIUM"
            } else {
                "HIGH"
            };
            tx.execute("INSERT INTO cash_variance_cases(id,tenant_id,branch_id,cash_session_id,expected_fils,counted_fils,variance_fils,severity,status,created_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,'OPEN',?9)",params![id.to_string(),req.tenant_id.to_string(),req.branch_id.to_string(),req.cash_session_id.to_string(),pre.expected_cash.0,req.counted_cash.0,variance.0,severity,req.now.to_rfc3339()])?;
            Some(id)
        } else {
            None
        };
        let report =
            Self::cash_session_report_conn(&tx, req.tenant_id, req.branch_id, req.cash_session_id)?;
        let result = CloseCashSessionResult {
            report,
            variance_case_id,
        };
        let payload = serde_json::to_string(&result)?;
        Self::append_audit(
            &tx,
            req.tenant_id,
            req.device_id,
            req.user_id,
            "CASH_SESSION_CLOSED",
            "cash_session",
            &req.cash_session_id.to_string(),
            &payload,
            req.now,
        )?;
        tx.execute("INSERT INTO sync_queue(id,tenant_id,branch_id,device_id,operation_id,entity_type,entity_id,mutation_type,payload_json,state,created_at,updated_at) VALUES(?1,?2,?3,?4,?5,'cash_session',?6,'CLOSE',?7,'PENDING',?8,?8)",params![Uuid::new_v4().to_string(),req.tenant_id.to_string(),req.branch_id.to_string(),req.device_id.to_string(),req.operation_id.to_string(),req.cash_session_id.to_string(),payload,req.now.to_rfc3339()])?;
        Self::record_idempotent_result(
            &tx,
            req.tenant_id,
            req.operation_id,
            "CASH_SESSION_CLOSE",
            &request_sha256,
            &result,
            req.now,
        )?;
        tx.commit()?;
        Ok(result)
    }

    fn cash_session_report_conn(
        conn: &Connection,
        tenant: TenantId,
        branch: BranchId,
        cash_session_id: Uuid,
    ) -> Result<CashSessionReport, StoreError> {
        let row:Option<(String,i64,Option<i64>,Option<i64>)>=conn.query_row("SELECT status,opening_float_fils,counted_cash_fils,variance_fils FROM cash_sessions WHERE id=?1 AND tenant_id=?2 AND branch_id=?3",params![cash_session_id.to_string(),tenant.to_string(),branch.to_string()],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional()?;
        let (status, opening, counted, variance) =
            row.ok_or(StoreError::NotFound("cash session"))?;
        let cash_sales:i64=conn.query_row("SELECT COALESCE(SUM(sp.amount_fils),0) FROM sale_payments sp JOIN sales s ON s.id=sp.sale_id WHERE s.cash_session_id=?1 AND s.tenant_id=?2 AND s.branch_id=?3 AND sp.tender_kind='CASH'",params![cash_session_id.to_string(),tenant.to_string(),branch.to_string()],|r|r.get(0))?;
        let cash_refunds:i64=conn.query_row("SELECT COALESCE(SUM(rp.amount_fils),0) FROM refund_payments rp JOIN refunds r ON r.id=rp.refund_id WHERE rp.cash_session_id=?1 AND r.tenant_id=?2 AND r.branch_id=?3 AND rp.tender_kind='CASH'",params![cash_session_id.to_string(),tenant.to_string(),branch.to_string()],|r|r.get(0))?;
        let cash_voids:i64=conn.query_row("SELECT COALESCE(SUM(amount_fils),0) FROM sale_void_payment_effects WHERE cash_session_id=?1 AND tenant_id=?2 AND branch_id=?3 AND tender_kind='CASH'",params![cash_session_id.to_string(),tenant.to_string(),branch.to_string()],|r|r.get(0))?;
        let movement_total = |kind: &str| -> Result<i64, StoreError> {
            Ok(conn.query_row("SELECT COALESCE(SUM(amount_fils),0) FROM cash_movements WHERE cash_session_id=?1 AND tenant_id=?2 AND branch_id=?3 AND kind=?4",params![cash_session_id.to_string(),tenant.to_string(),branch.to_string(),kind],|r|r.get(0))?)
        };
        let movement_count = |kind: &str| -> Result<i64, StoreError> {
            Ok(conn.query_row("SELECT COUNT(*) FROM cash_movements WHERE cash_session_id=?1 AND tenant_id=?2 AND branch_id=?3 AND kind=?4",params![cash_session_id.to_string(),tenant.to_string(),branch.to_string(),kind],|r|r.get(0))?)
        };
        let paid_in = movement_total("PAID_IN")?;
        let paid_out = movement_total("PAID_OUT")?;
        let safe_drop = movement_total("SAFE_DROP")?;
        let petty_cash = movement_total("PETTY_CASH")?;
        let no_sale_count = movement_count("NO_SALE")?;
        let expected = Money(opening)
            .checked_add(Money(cash_sales))?
            .checked_sub(Money(cash_refunds))?
            .checked_sub(Money(cash_voids))?
            .checked_add(Money(paid_in))?
            .checked_sub(Money(paid_out))?
            .checked_sub(Money(safe_drop))?
            .checked_sub(Money(petty_cash))?;
        Ok(CashSessionReport {
            cash_session_id,
            status,
            opening_float: Money(opening),
            cash_sales: Money(cash_sales),
            cash_refunds: Money(cash_refunds),
            cash_voids: Money(cash_voids),
            paid_in: Money(paid_in),
            paid_out: Money(paid_out),
            safe_drop: Money(safe_drop),
            petty_cash: Money(petty_cash),
            no_sale_count,
            expected_cash: expected,
            counted_cash: counted.map(Money),
            variance: variance.map(Money),
        })
    }

    pub fn void_sale_payload_sha256(
        sale_id: SaleId,
        cash_session_id: Option<Uuid>,
        reason: &str,
    ) -> String {
        let payload = serde_json::json!({"cash_session_id":cash_session_id.map(|v|v.to_string()),"reason":reason,"sale_id":sale_id.to_string()});
        sha256_hex(
            serde_json::to_string(&payload)
                .expect("void payload serializes")
                .as_bytes(),
        )
    }

    pub fn void_sale(&mut self, req: VoidSaleRequest) -> Result<VoidSaleResult, StoreError> {
        if req.reason.trim().is_empty() {
            return Err(StoreError::Validation("void reason is required".into()));
        }
        let request_sha256 = Self::void_sale_request_sha256(&req)?;
        if let Some(existing) = self.load_idempotent_result(
            req.tenant_id,
            req.operation_id,
            "SALE_VOID",
            &request_sha256,
        )? {
            return Ok(existing);
        }
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        Self::assert_active_device_tx(&tx, req.tenant_id, req.branch_id, req.device_id)?;
        Self::assert_user_scope(&tx, req.tenant_id, req.user_id)?;
        Self::assert_permission(&tx, req.tenant_id, req.branch_id, req.user_id, "sale.void")?;
        let payload_sha =
            Self::void_sale_payload_sha256(req.sale_id, req.cash_session_id, &req.reason);
        let approval_ok:Option<i32>=tx.query_row("SELECT 1 FROM manager_approval_consumptions WHERE id=?1 AND tenant_id=?2 AND device_id=?3 AND operation_id=?4 AND action='sale.void' AND entity_id=?5 AND payload_sha256=?6",params![req.approval_ref.to_string(),req.tenant_id.to_string(),req.device_id.to_string(),req.operation_id.to_string(),req.sale_id.to_string(),payload_sha],|r|r.get(0)).optional()?;
        if approval_ok.is_none() {
            return Err(StoreError::Authorization(
                "void manager approval does not match exact operation",
            ));
        }
        let sale: Option<(String, String, String, i64)> = tx
            .query_row(
                "SELECT tenant_id,branch_id,status,total_fils FROM sales WHERE id=?1",
                params![req.sale_id.to_string()],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .optional()?;
        let (tenant_s, branch_s, status, total) = sale.ok_or(StoreError::NotFound("sale"))?;
        if tenant_s != req.tenant_id.to_string() || branch_s != req.branch_id.to_string() {
            return Err(StoreError::Authorization("sale tenant/branch mismatch"));
        }
        if status != "COMPLETED" {
            return Err(StoreError::Conflict(
                "only completed sales can be voided".into(),
            ));
        }
        let has_cash: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM sale_payments WHERE sale_id=?1 AND tender_kind='CASH')",
            params![req.sale_id.to_string()],
            |r| r.get::<_, i64>(0).map(|v| v != 0),
        )?;
        if has_cash {
            let sid = req.cash_session_id.ok_or_else(|| {
                StoreError::Validation(
                    "cash sale void requires an open cash session for payout".into(),
                )
            })?;
            let ok:Option<i32>=tx.query_row("SELECT 1 FROM cash_sessions WHERE id=?1 AND tenant_id=?2 AND branch_id=?3 AND status='OPEN'",params![sid.to_string(),req.tenant_id.to_string(),req.branch_id.to_string()],|r|r.get(0)).optional()?;
            if ok.is_none() {
                return Err(StoreError::Authorization(
                    "void cash session scope/status mismatch",
                ));
            }
        }
        let void_id = Uuid::new_v4();
        tx.execute("INSERT INTO sale_voids(id,tenant_id,branch_id,sale_id,operation_id,device_id,user_id,approval_ref,reason,reversed_total_fils,created_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",params![void_id.to_string(),req.tenant_id.to_string(),req.branch_id.to_string(),req.sale_id.to_string(),req.operation_id.to_string(),req.device_id.to_string(),req.user_id.to_string(),req.approval_ref.to_string(),req.reason,total,req.now.to_rfc3339()])?;
        let centre_id:String=tx.query_row("SELECT id FROM inventory_centres WHERE tenant_id=?1 AND branch_id=?2 ORDER BY CASE WHEN centre_type='SHOP_FLOOR' THEN 0 ELSE 1 END LIMIT 1",params![req.tenant_id.to_string(),req.branch_id.to_string()],|r|r.get(0)).optional()?.ok_or(StoreError::NotFound("inventory centre"))?;
        {
            let mut st=tx.prepare("SELECT id,product_id,quantity_milli,unit_cost_fils FROM sale_lines WHERE sale_id=?1")?;
            let rows = st.query_map(params![req.sale_id.to_string()], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, i64>(2)?,
                    r.get::<_, i64>(3)?,
                ))
            })?;
            for row in rows {
                let (sale_line_id, product_id, qty, cost) = row?;
                let track: i32 = tx.query_row(
                    "SELECT track_inventory FROM products WHERE id=?1 AND tenant_id=?2",
                    params![product_id, req.tenant_id.to_string()],
                    |r| r.get(0),
                )?;
                if track != 0 {
                    let changed=tx.execute("UPDATE stock_levels SET quantity_milli=quantity_milli+?5,version=version+1 WHERE tenant_id=?1 AND branch_id=?2 AND centre_id=?3 AND product_id=?4",params![req.tenant_id.to_string(),req.branch_id.to_string(),centre_id,product_id,qty])?;
                    if changed == 0 {
                        tx.execute("INSERT INTO stock_levels(tenant_id,branch_id,centre_id,product_id,quantity_milli) VALUES(?1,?2,?3,?4,?5)",params![req.tenant_id.to_string(),req.branch_id.to_string(),centre_id,product_id,qty])?;
                    }
                    let source_id = format!("{}:{}", void_id, sale_line_id);
                    tx.execute("INSERT INTO inventory_movements(id,tenant_id,branch_id,centre_id,product_id,operation_id,movement_type,quantity_milli,unit_cost_fils,source_type,source_id,device_id,user_id,created_at) VALUES(?1,?2,?3,?4,?5,?6,'VOID',?7,?8,'SALE_VOID',?9,?10,?11,?12)",params![Uuid::new_v4().to_string(),req.tenant_id.to_string(),req.branch_id.to_string(),centre_id,product_id,req.operation_id.to_string(),qty,cost,source_id,req.device_id.to_string(),req.user_id.to_string(),req.now.to_rfc3339()])?;
                    Self::apply_cost_projection(
                        &tx,
                        req.tenant_id,
                        req.branch_id,
                        &centre_id,
                        &product_id,
                        QuantityMilli(qty),
                        Money(cost),
                    )?;
                }
            }
        }
        {
            let mut st = tx.prepare(
                "SELECT tender_kind,amount_fils,reference FROM sale_payments WHERE sale_id=?1",
            )?;
            let rows = st.query_map(params![req.sale_id.to_string()], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, Option<String>>(2)?,
                ))
            })?;
            for row in rows {
                let (kind, amount, reference) = row?;
                let cash_session = if kind == "CASH" {
                    req.cash_session_id.map(|v| v.to_string())
                } else {
                    None
                };
                tx.execute("INSERT INTO sale_void_payment_effects(id,tenant_id,branch_id,sale_void_id,tender_kind,amount_fils,cash_session_id,device_id,user_id,reference,created_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",params![Uuid::new_v4().to_string(),req.tenant_id.to_string(),req.branch_id.to_string(),void_id.to_string(),kind,amount,cash_session,req.device_id.to_string(),req.user_id.to_string(),reference,req.now.to_rfc3339()])?;
            }
        }
        tx.execute(
            "UPDATE sales SET status='VOIDED' WHERE id=?1 AND status='COMPLETED'",
            params![req.sale_id.to_string()],
        )?;
        let result = VoidSaleResult {
            void_id,
            sale_id: req.sale_id,
            reversed_total: Money(total),
        };
        let payload = serde_json::to_string(&result)?;
        Self::append_audit(
            &tx,
            req.tenant_id,
            req.device_id,
            req.user_id,
            "SALE_VOIDED",
            "sale_void",
            &void_id.to_string(),
            &payload,
            req.now,
        )?;
        tx.execute("INSERT INTO sync_queue(id,tenant_id,branch_id,device_id,operation_id,entity_type,entity_id,mutation_type,payload_json,state,created_at,updated_at) VALUES(?1,?2,?3,?4,?5,'sale_void',?6,'INSERT',?7,'PENDING',?8,?8)",params![Uuid::new_v4().to_string(),req.tenant_id.to_string(),req.branch_id.to_string(),req.device_id.to_string(),req.operation_id.to_string(),void_id.to_string(),payload,req.now.to_rfc3339()])?;
        Self::record_idempotent_result(
            &tx,
            req.tenant_id,
            req.operation_id,
            "SALE_VOID",
            &request_sha256,
            &result,
            req.now,
        )?;
        tx.commit()?;
        Ok(result)
    }

    pub fn claim_next_print_job(
        &mut self,
        tenant: TenantId,
        branch: BranchId,
        device: DeviceId,
        now: DateTime<Utc>,
    ) -> Result<Option<PrintJobLease>, StoreError> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        Self::assert_active_device_tx(&tx, tenant, branch, device)?;
        let row:Option<PrintJobRow>=tx.query_row("SELECT pj.id,pj.sale_id,pj.document_type,pj.snapshot_sha256,pj.printer_target,pj.attempts,rs.receipt_text,EXISTS(SELECT 1 FROM sale_payments sp WHERE sp.sale_id=pj.sale_id AND sp.tender_kind='CASH') FROM print_jobs pj LEFT JOIN receipt_snapshots rs ON rs.sale_id=pj.sale_id WHERE pj.tenant_id=?1 AND pj.branch_id=?2 AND pj.device_id=?3 AND pj.state='PENDING' ORDER BY pj.created_at,pj.id LIMIT 1",params![tenant.to_string(),branch.to_string(),device.to_string()],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?,r.get(7)?))).optional()?;
        let Some((
            id,
            sale_id,
            document_type,
            snapshot_sha256,
            printer_target,
            attempts,
            receipt_text,
            cash_sale,
        )) = row
        else {
            tx.commit()?;
            return Ok(None);
        };
        if let (Some(expected), Some(text)) = (&snapshot_sha256, &receipt_text) {
            if sha256_hex(text.as_bytes()) != *expected {
                return Err(StoreError::Conflict(
                    "receipt snapshot integrity check failed".into(),
                ));
            }
        }
        if sale_id.is_some() && receipt_text.is_none() {
            return Err(StoreError::Conflict(
                "sale print job has no historical receipt snapshot".into(),
            ));
        }
        let changed=tx.execute("UPDATE print_jobs SET state='PRINTING',attempts=attempts+1,last_error=NULL,updated_at=?2 WHERE id=?1 AND state='PENDING'",params![id,now.to_rfc3339()])?;
        if changed != 1 {
            return Err(StoreError::Conflict("print job lease lost".into()));
        }
        let lease_token = Uuid::new_v4();
        tx.execute(
            "INSERT INTO print_job_leases(print_job_id,lease_token,leased_at) VALUES(?1,?2,?3)",
            params![id, lease_token.to_string(), now.to_rfc3339()],
        )?;
        let lease = PrintJobLease {
            print_job_id: Uuid::parse_str(&id)
                .map_err(|_| StoreError::Validation("invalid print job id".into()))?,
            sale_id: sale_id
                .map(|v| {
                    Uuid::parse_str(&v)
                        .map(SaleId)
                        .map_err(|_| StoreError::Validation("invalid sale id in print job".into()))
                })
                .transpose()?,
            document_type,
            snapshot_sha256,
            receipt_text,
            printer_target,
            lease_token,
            cash_drawer_pulse: cash_sale != 0,
            attempt: attempts + 1,
        };
        tx.commit()?;
        Ok(Some(lease))
    }

    #[allow(
        clippy::too_many_arguments,
        reason = "print completion binds the lease to trusted terminal and error evidence"
    )]
    pub fn finish_print_job(
        &mut self,
        tenant: TenantId,
        branch: BranchId,
        device: DeviceId,
        print_job_id: Uuid,
        lease_token: Uuid,
        success: bool,
        error: Option<&str>,
        now: DateTime<Utc>,
    ) -> Result<(), StoreError> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        Self::assert_active_device_tx(&tx, tenant, branch, device)?;
        if success && error.is_some() {
            return Err(StoreError::Validation(
                "successful print cannot contain error".into(),
            ));
        }
        let state = if success { "PRINTED" } else { "FAILED" };
        let n=tx.execute("UPDATE print_jobs SET state=?6,last_error=?7,updated_at=?8 WHERE id=?1 AND tenant_id=?2 AND branch_id=?3 AND device_id=?4 AND state='PRINTING' AND EXISTS(SELECT 1 FROM print_job_leases pjl WHERE pjl.print_job_id=print_jobs.id AND pjl.lease_token=?5)",params![print_job_id.to_string(),tenant.to_string(),branch.to_string(),device.to_string(),lease_token.to_string(),state,error,now.to_rfc3339()])?;
        if n != 1 {
            return Err(StoreError::Conflict(
                "print job is not held by this device".into(),
            ));
        }
        tx.execute(
            "DELETE FROM print_job_leases WHERE print_job_id=?1 AND lease_token=?2",
            params![print_job_id.to_string(), lease_token.to_string()],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn recover_stale_print_jobs(
        &mut self,
        tenant: TenantId,
        branch: BranchId,
        device: DeviceId,
        stale_before: DateTime<Utc>,
        now: DateTime<Utc>,
    ) -> Result<usize, StoreError> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        Self::assert_active_device_tx(&tx, tenant, branch, device)?;
        let recovered=tx.execute("UPDATE print_jobs SET state='PENDING',last_error='Recovered after interrupted print worker',updated_at=?5 WHERE tenant_id=?1 AND branch_id=?2 AND device_id=?3 AND state='PRINTING' AND EXISTS(SELECT 1 FROM print_job_leases pjl WHERE pjl.print_job_id=print_jobs.id AND pjl.leased_at<=?4)",params![tenant.to_string(),branch.to_string(),device.to_string(),stale_before.to_rfc3339(),now.to_rfc3339()])?;
        tx.execute("DELETE FROM print_job_leases WHERE print_job_id IN (SELECT id FROM print_jobs WHERE tenant_id=?1 AND branch_id=?2 AND device_id=?3 AND state='PENDING' AND last_error='Recovered after interrupted print worker')",params![tenant.to_string(),branch.to_string(),device.to_string()])?;
        tx.commit()?;
        Ok(recovered)
    }

    pub fn requeue_failed_print_job(
        &self,
        tenant: TenantId,
        branch: BranchId,
        device: DeviceId,
        print_job_id: Uuid,
        now: DateTime<Utc>,
    ) -> Result<(), StoreError> {
        self.assert_active_device(&self.conn, tenant, branch, device)?;
        let n=self.conn.execute("UPDATE print_jobs SET state='PENDING',updated_at=?5 WHERE id=?1 AND tenant_id=?2 AND branch_id=?3 AND device_id=?4 AND state='FAILED'",params![print_job_id.to_string(),tenant.to_string(),branch.to_string(),device.to_string(),now.to_rfc3339()])?;
        if n != 1 {
            return Err(StoreError::Conflict(
                "only failed print jobs can be requeued".into(),
            ));
        }
        Ok(())
    }

    pub fn failed_print_jobs(
        &self,
        context: LocalTerminalContext,
        user: UserId,
    ) -> Result<Vec<FailedPrintJob>, StoreError> {
        self.validate_local_session(context, user)?;
        Self::assert_permission_conn(
            &self.conn,
            context.tenant_id,
            context.branch_id,
            user,
            "receipt.reprint",
        )?;
        let mut statement=self.conn.prepare("SELECT pj.id,pj.sale_id,s.receipt_number,pj.document_type,pj.attempts,COALESCE(pj.last_error,'Unknown printer error'),pj.updated_at FROM print_jobs pj LEFT JOIN sales s ON s.id=pj.sale_id AND s.tenant_id=pj.tenant_id AND s.branch_id=pj.branch_id WHERE pj.tenant_id=?1 AND pj.branch_id=?2 AND pj.device_id=?3 AND pj.state='FAILED' ORDER BY pj.updated_at DESC,pj.id LIMIT 100")?;
        let raw = statement
            .query_map(
                params![
                    context.tenant_id.to_string(),
                    context.branch_id.to_string(),
                    context.device_id.to_string()
                ],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, Option<String>>(1)?,
                        row.get::<_, Option<String>>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, i64>(4)?,
                        row.get::<_, String>(5)?,
                        row.get::<_, String>(6)?,
                    ))
                },
            )?
            .collect::<Result<Vec<_>, _>>()?;
        raw.into_iter()
            .map(
                |(id, sale, receipt, document_type, attempts, last_error, updated_at)| {
                    Ok(FailedPrintJob {
                        print_job_id: Uuid::parse_str(&id).map_err(|_| {
                            StoreError::Validation("invalid print job identity".into())
                        })?,
                        sale_id: sale
                            .map(|value| {
                                Uuid::parse_str(&value).map(SaleId).map_err(|_| {
                                    StoreError::Validation(
                                        "invalid sale identity in print job".into(),
                                    )
                                })
                            })
                            .transpose()?,
                        receipt_number: receipt,
                        document_type,
                        attempts,
                        last_error,
                        updated_at,
                    })
                },
            )
            .collect()
    }

    pub fn requeue_failed_print_job_authorized(
        &self,
        context: LocalTerminalContext,
        user: UserId,
        print_job_id: Uuid,
        now: DateTime<Utc>,
    ) -> Result<(), StoreError> {
        self.validate_local_session(context, user)?;
        Self::assert_permission_conn(
            &self.conn,
            context.tenant_id,
            context.branch_id,
            user,
            "receipt.reprint",
        )?;
        self.requeue_failed_print_job(
            context.tenant_id,
            context.branch_id,
            context.device_id,
            print_job_id,
            now,
        )
    }

    pub fn default_printer_profile(
        &self,
        context: LocalTerminalContext,
    ) -> Result<PrinterProfile, StoreError> {
        self.assert_active_device(
            &self.conn,
            context.tenant_id,
            context.branch_id,
            context.device_id,
        )?;
        self.conn.query_row("SELECT transport,target,paper_width_mm,characters_per_line,character_encoding,cut_mode,drawer_pulse_policy FROM printer_profiles WHERE tenant_id=?1 AND branch_id=?2 AND device_id=?3 AND is_default=1 AND active=1",params![context.tenant_id.to_string(),context.branch_id.to_string(),context.device_id.to_string()],|row|Ok(PrinterProfile{transport:row.get(0)?,target:row.get(1)?,paper_width_mm:row.get(2)?,characters_per_line:row.get(3)?,character_encoding:row.get(4)?,cut_mode:row.get(5)?,drawer_pulse_policy:row.get(6)?})).optional()?.ok_or(StoreError::NotFound("default printer profile"))
    }

    pub fn claim_sync_batch(
        &mut self,
        context: LocalTerminalContext,
        credential_secret: &str,
        limit: usize,
        now: DateTime<Utc>,
    ) -> Result<Vec<SyncMutationEnvelope>, StoreError> {
        self.validate_local_device_context(context)?;
        let credential_version = self.authenticate_device(
            context.tenant_id,
            context.branch_id,
            context.device_id,
            credential_secret,
        )?;
        if limit == 0 || limit > 100 {
            return Err(StoreError::Validation(
                "sync batch limit must be between 1 and 100".into(),
            ));
        }
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let stale_before = (now - chrono::Duration::minutes(5)).to_rfc3339();
        tx.execute("UPDATE sync_queue SET state='RETRYING',updated_at=?4 WHERE tenant_id=?1 AND device_id=?2 AND state='SENDING' AND EXISTS(SELECT 1 FROM sync_delivery_leases sdl WHERE sdl.mutation_id=sync_queue.id AND sdl.leased_at<=?3)",params![context.tenant_id.to_string(),context.device_id.to_string(),stale_before,now.to_rfc3339()])?;
        tx.execute("DELETE FROM sync_delivery_leases WHERE mutation_id IN (SELECT id FROM sync_queue WHERE tenant_id=?1 AND device_id=?2 AND state='RETRYING')",params![context.tenant_id.to_string(),context.device_id.to_string()])?;
        let raw = {
            let mut statement=tx.prepare("SELECT sq.id,sq.operation_id,sq.entity_type,sq.entity_id,sq.mutation_type,sq.payload_json FROM sync_queue sq LEFT JOIN sync_retry_schedule srs ON srs.mutation_id=sq.id WHERE sq.tenant_id=?1 AND sq.branch_id=?2 AND sq.device_id=?3 AND sq.state IN ('PENDING','RETRYING') AND (srs.next_attempt_at IS NULL OR srs.next_attempt_at<=?4) ORDER BY sq.created_at,sq.id LIMIT ?5")?;
            let rows = statement.query_map(
                params![
                    context.tenant_id.to_string(),
                    context.branch_id.to_string(),
                    context.device_id.to_string(),
                    now.to_rfc3339(),
                    limit as i64
                ],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, String>(4)?,
                        row.get::<_, String>(5)?,
                    ))
                },
            )?;
            rows.collect::<Result<Vec<_>, _>>()?
        };
        let mut envelopes = Vec::with_capacity(raw.len());
        for (mutation_id, operation_id, entity_type, entity_id, mutation_type, payload_json) in raw
        {
            let lease_token = Uuid::new_v4();
            let changed=tx.execute("UPDATE sync_queue SET state='SENDING',attempts=attempts+1,updated_at=?2 WHERE id=?1 AND state IN ('PENDING','RETRYING')",params![mutation_id,now.to_rfc3339()])?;
            if changed != 1 {
                return Err(StoreError::Conflict("sync mutation lease lost".into()));
            }
            tx.execute("INSERT INTO sync_delivery_leases(mutation_id,lease_token,leased_at) VALUES(?1,?2,?3)",params![mutation_id,lease_token.to_string(),now.to_rfc3339()])?;
            tx.execute(
                "DELETE FROM sync_retry_schedule WHERE mutation_id=?1",
                params![mutation_id],
            )?;
            let material = SyncMutationMaterial {
                mutation_id: Uuid::parse_str(&mutation_id)
                    .map_err(|_| StoreError::Validation("invalid sync mutation identity".into()))?,
                tenant_id: context.tenant_id,
                branch_id: context.branch_id,
                device_id: context.device_id,
                operation_id: OperationId(Uuid::parse_str(&operation_id).map_err(|_| {
                    StoreError::Validation("invalid sync operation identity".into())
                })?),
                entity_type,
                entity_id,
                mutation_type,
                payload_sha256: sha256_hex(payload_json.as_bytes()),
                payload_json,
                credential_version,
            };
            let signature_hex = sign_sync_mutation(credential_secret.as_bytes(), &material);
            envelopes.push(SyncMutationEnvelope {
                material,
                signature_hex,
                lease_token,
            });
        }
        tx.commit()?;
        Ok(envelopes)
    }

    pub fn complete_sync_delivery(
        &mut self,
        context: LocalTerminalContext,
        mutation_id: Uuid,
        lease_token: Uuid,
        outcome: SyncDeliveryOutcome,
        now: DateTime<Utc>,
    ) -> Result<(), StoreError> {
        self.validate_local_device_context(context)?;
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let held:Option<i32>=tx.query_row("SELECT 1 FROM sync_queue sq JOIN sync_delivery_leases sdl ON sdl.mutation_id=sq.id WHERE sq.id=?1 AND sq.tenant_id=?2 AND sq.branch_id=?3 AND sq.device_id=?4 AND sq.state='SENDING' AND sdl.lease_token=?5",params![mutation_id.to_string(),context.tenant_id.to_string(),context.branch_id.to_string(),context.device_id.to_string(),lease_token.to_string()],|row|row.get(0)).optional()?;
        if held.is_none() {
            return Err(StoreError::Conflict(
                "sync mutation lease is stale or mismatched".into(),
            ));
        }
        let (state, error, checkpoint) = match outcome {
            SyncDeliveryOutcome::Hub(result) => {
                if !matches!(result.state.as_str(), "COMMITTED" | "REQUIRES_REVIEW") {
                    return Err(StoreError::Validation("invalid hub mutation state".into()));
                }
                (result.state, None, Some(result.hub_sequence))
            }
            SyncDeliveryOutcome::Retry(error) => ("RETRYING".into(), Some(error), None),
            SyncDeliveryOutcome::PermanentFailure(error) => ("FAILED".into(), Some(error), None),
        };
        tx.execute(
            "UPDATE sync_queue SET state=?2,updated_at=?3 WHERE id=?1",
            params![mutation_id.to_string(), state, now.to_rfc3339()],
        )?;
        tx.execute(
            "DELETE FROM sync_delivery_leases WHERE mutation_id=?1 AND lease_token=?2",
            params![mutation_id.to_string(), lease_token.to_string()],
        )?;
        if let Some(sequence) = checkpoint {
            tx.execute("INSERT INTO device_sync_checkpoints(tenant_id,branch_id,device_id,last_hub_sequence,last_mutation_id,updated_at) VALUES(?1,?2,?3,?4,?5,?6) ON CONFLICT(tenant_id,device_id) DO UPDATE SET last_hub_sequence=MAX(last_hub_sequence,excluded.last_hub_sequence),last_mutation_id=excluded.last_mutation_id,updated_at=excluded.updated_at",params![context.tenant_id.to_string(),context.branch_id.to_string(),context.device_id.to_string(),sequence,mutation_id.to_string(),now.to_rfc3339()])?;
        }
        if let Some(message) = error {
            tx.execute("INSERT INTO sync_delivery_errors(id,mutation_id,state,error,created_at) VALUES(?1,?2,?3,?4,?5)",params![Uuid::new_v4().to_string(),mutation_id.to_string(),state,message,now.to_rfc3339()])?;
            if state == "RETRYING" {
                let attempts: i64 = tx.query_row(
                    "SELECT attempts FROM sync_queue WHERE id=?1",
                    params![mutation_id.to_string()],
                    |row| row.get(0),
                )?;
                let exponent = u32::try_from(attempts.clamp(1, 8)).unwrap_or(8);
                let delay_seconds = (1_i64 << exponent).min(300);
                tx.execute("INSERT INTO sync_retry_schedule(mutation_id,next_attempt_at) VALUES(?1,?2) ON CONFLICT(mutation_id) DO UPDATE SET next_attempt_at=excluded.next_attempt_at",params![mutation_id.to_string(),(now+chrono::Duration::seconds(delay_seconds)).to_rfc3339()])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    pub fn accept_hub_mutation(
        &mut self,
        envelope: &SyncMutationEnvelope,
        credential_secret: &str,
        now: DateTime<Utc>,
    ) -> Result<HubMutationResult, StoreError> {
        let material = &envelope.material;
        let version = self.authenticate_device(
            material.tenant_id,
            material.branch_id,
            material.device_id,
            credential_secret,
        )?;
        if version != material.credential_version {
            return Err(StoreError::Authorization(
                "device credential version mismatch",
            ));
        }
        if !verify_sync_mutation(
            credential_secret.as_bytes(),
            material,
            &envelope.signature_hex,
        ) {
            return Err(StoreError::Authorization("sync mutation signature invalid"));
        }
        if sha256_hex(material.payload_json.as_bytes()) != material.payload_sha256 {
            return Err(StoreError::Conflict("sync payload digest mismatch".into()));
        }
        let request_sha256 = sha256_hex(&serde_json::to_vec(material)?);
        let existing:Option<(i64,String,String)>=self.conn.query_row("SELECT hub_sequence,state,request_sha256 FROM hub_mutations WHERE tenant_id=?1 AND device_id=?2 AND mutation_id=?3",params![material.tenant_id.to_string(),material.device_id.to_string(),material.mutation_id.to_string()],|row|Ok((row.get(0)?,row.get(1)?,row.get(2)?))).optional()?;
        if let Some((hub_sequence, state, stored_request)) = existing {
            if stored_request != request_sha256 {
                return Err(StoreError::Conflict(
                    "sync mutation ID reused with different payload".into(),
                ));
            }
            return Ok(HubMutationResult {
                hub_sequence,
                state,
                replayed: true,
            });
        }
        let duplicate_operation:Option<i64>=self.conn.query_row("SELECT hub_sequence FROM hub_mutations WHERE tenant_id=?1 AND device_id=?2 AND operation_id=?3 AND entity_type=?4 AND entity_id=?5",params![material.tenant_id.to_string(),material.device_id.to_string(),material.operation_id.to_string(),material.entity_type,material.entity_id],|row|row.get(0)).optional()?;
        if duplicate_operation.is_some() {
            return Err(StoreError::Conflict(
                "operation already submitted under a different mutation ID".into(),
            ));
        }
        let state = if matches!(
            material.entity_type.as_str(),
            "sale"
                | "refund"
                | "cash_movement"
                | "cash_session"
                | "sale_void"
                | "inventory_movement"
                | "inventory_receipt"
                | "inventory_waste"
                | "inventory_transfer_dispatch"
                | "inventory_transfer_receipt"
                | "inventory_stocktake"
                | "supplier_payment"
                | "customer_credit"
                | "loyalty_event"
        ) {
            "COMMITTED"
        } else {
            "REQUIRES_REVIEW"
        };
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let result_json =
            serde_json::json!({"accepted":state=="COMMITTED","classification":state}).to_string();
        tx.execute("INSERT INTO hub_mutations(tenant_id,branch_id,device_id,mutation_id,operation_id,entity_type,entity_id,mutation_type,payload_json,payload_sha256,request_sha256,credential_version,state,result_json,received_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15)",params![material.tenant_id.to_string(),material.branch_id.to_string(),material.device_id.to_string(),material.mutation_id.to_string(),material.operation_id.to_string(),material.entity_type,material.entity_id,material.mutation_type,material.payload_json,material.payload_sha256,request_sha256,material.credential_version,state,result_json,now.to_rfc3339()])?;
        let hub_sequence = tx.last_insert_rowid();
        tx.commit()?;
        Ok(HubMutationResult {
            hub_sequence,
            state: state.into(),
            replayed: false,
        })
    }

    pub fn resolve_sync_conflict(
        &mut self,
        context: LocalTerminalContext,
        user: UserId,
        hub_sequence: i64,
        resolution: &str,
        notes: &str,
        now: DateTime<Utc>,
    ) -> Result<Uuid, StoreError> {
        self.validate_local_session(context, user)?;
        Self::assert_permission_conn(
            &self.conn,
            context.tenant_id,
            context.branch_id,
            user,
            "sync.resolve",
        )?;
        if !matches!(resolution, "APPLY" | "COMPENSATE" | "REJECT") {
            return Err(StoreError::Validation(
                "invalid sync conflict resolution".into(),
            ));
        }
        if notes.trim().is_empty() {
            return Err(StoreError::Validation(
                "sync conflict resolution notes are required".into(),
            ));
        }
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let review:Option<i32>=tx.query_row("SELECT 1 FROM hub_mutations WHERE hub_sequence=?1 AND tenant_id=?2 AND state='REQUIRES_REVIEW'",params![hub_sequence,context.tenant_id.to_string()],|row|row.get(0)).optional()?;
        if review.is_none() {
            return Err(StoreError::Conflict(
                "hub mutation is not awaiting review".into(),
            ));
        }
        let id = Uuid::new_v4();
        tx.execute("INSERT INTO sync_conflict_resolutions(id,tenant_id,hub_sequence,resolution,notes,resolved_by_user_id,resolved_at) VALUES(?1,?2,?3,?4,?5,?6,?7)",params![id.to_string(),context.tenant_id.to_string(),hub_sequence,resolution,notes.trim(),user.to_string(),now.to_rfc3339()])?;
        Self::append_audit(
            &tx,
            context.tenant_id,
            context.device_id,
            user,
            "SYNC_CONFLICT_RESOLVED",
            "hub_mutation",
            &hub_sequence.to_string(),
            &serde_json::json!({"resolution":resolution,"notes":notes.trim()}).to_string(),
            now,
        )?;
        tx.commit()?;
        Ok(id)
    }

    pub fn receive_inventory(
        &mut self,
        request: InventoryReceiptRequest,
    ) -> Result<InventoryReceiptResult, StoreError> {
        self.validate_local_session(request.context, request.user_id)?;
        if request.lines.is_empty() {
            return Err(StoreError::Validation(
                "inventory receipt has no lines".into(),
            ));
        }
        let digest = sha256_hex(&serde_json::to_vec(
            &serde_json::json!({"supplier_id":request.supplier_id,"centre_id":request.centre_id,"supplier_document_no":request.supplier_document_no,"lines":request.lines}),
        )?);
        if let Some(result) = self.load_inventory_operation(
            request.context.tenant_id,
            request.operation_id,
            "RECEIVE",
            &digest,
        )? {
            return Ok(result);
        }
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        Self::assert_permission(
            &tx,
            request.context.tenant_id,
            request.context.branch_id,
            request.user_id,
            "inventory.receive",
        )?;
        Self::assert_inventory_centre(
            &tx,
            request.context.tenant_id,
            request.context.branch_id,
            request.centre_id,
        )?;
        let supplier: Option<i32> = tx
            .query_row(
                "SELECT 1 FROM suppliers WHERE id=?1 AND tenant_id=?2",
                params![
                    request.supplier_id.to_string(),
                    request.context.tenant_id.to_string()
                ],
                |row| row.get(0),
            )
            .optional()?;
        if supplier.is_none() {
            return Err(StoreError::Authorization("supplier tenant mismatch"));
        }
        let receipt_id = Uuid::new_v4();
        let mut accepted_total = 0_i64;
        let mut value_total = Money::ZERO;
        let mut discrepancy = false;
        tx.execute("INSERT INTO goods_receipts(id,tenant_id,branch_id,supplier_id,centre_id,operation_id,status,device_id,received_by_user_id,supplier_document_no,received_at) VALUES(?1,?2,?3,?4,?5,?6,'POSTED',?7,?8,?9,?10)",params![receipt_id.to_string(),request.context.tenant_id.to_string(),request.context.branch_id.to_string(),request.supplier_id.to_string(),request.centre_id.to_string(),request.operation_id.to_string(),request.context.device_id.to_string(),request.user_id.to_string(),request.supplier_document_no,request.now.to_rfc3339()])?;
        for line in &request.lines {
            if line.received_quantity.0 <= 0
                || line.rejected_quantity.0 < 0
                || line.damaged_quantity.0 < 0
                || line.unit_cost.0 < 0
            {
                return Err(StoreError::Validation(
                    "invalid receiving quantity or cost".into(),
                ));
            }
            let unavailable = line
                .rejected_quantity
                .0
                .checked_add(line.damaged_quantity.0)
                .ok_or(bhaipos_core::MoneyError::Overflow)?;
            let accepted = line
                .received_quantity
                .0
                .checked_sub(unavailable)
                .ok_or_else(|| {
                    StoreError::Validation(
                        "rejected and damaged quantity exceed received quantity".into(),
                    )
                })?;
            if accepted == 0 {
                return Err(StoreError::Validation(
                    "receiving line has no accepted quantity".into(),
                ));
            }
            Self::assert_product(&tx, request.context.tenant_id, line.product_id)?;
            let lot_id = if line.lot_number.is_some() || line.expires_on.is_some() {
                let id = Uuid::new_v4();
                tx.execute("INSERT INTO inventory_lots(id,tenant_id,product_id,supplier_id,lot_number,expires_on,status,unit_cost_fils,received_at) VALUES(?1,?2,?3,?4,?5,?6,'ACTIVE',?7,?8)",params![id.to_string(),request.context.tenant_id.to_string(),line.product_id.to_string(),request.supplier_id.to_string(),line.lot_number,line.expires_on,line.unit_cost.0,request.now.to_rfc3339()])?;
                Some(id)
            } else {
                None
            };
            let receipt_line_id = Uuid::new_v4();
            discrepancy |= unavailable > 0;
            tx.execute("INSERT INTO goods_receipt_lines(id,receipt_id,product_id,lot_id,received_qty_milli,rejected_qty_milli,damaged_qty_milli,unit_cost_fils,discrepancy_type) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)",params![receipt_line_id.to_string(),receipt_id.to_string(),line.product_id.to_string(),lot_id.map(|id|id.to_string()),line.received_quantity.0,line.rejected_quantity.0,line.damaged_quantity.0,line.unit_cost.0,if unavailable>0{Some("REJECTED_OR_DAMAGED")}else{None}])?;
            let movement_id = Self::append_inventory_effect(
                &tx,
                request.context,
                request.user_id,
                request.operation_id,
                request.centre_id,
                line.product_id,
                QuantityMilli(accepted),
                line.unit_cost,
                "RECEIVING",
                "GOODS_RECEIPT_LINE",
                &receipt_line_id.to_string(),
                lot_id,
                request.now,
            )?;
            let _ = movement_id;
            let valuation = Self::inventory_valuation_tx(
                &tx,
                request.context.tenant_id,
                request.context.branch_id,
                request.centre_id,
                line.product_id,
            )?;
            tx.execute(
                "UPDATE products SET current_cost_fils=?3 WHERE id=?1 AND tenant_id=?2",
                params![
                    line.product_id.to_string(),
                    request.context.tenant_id.to_string(),
                    valuation.weighted_average_cost.0
                ],
            )?;
            tx.execute("INSERT INTO cost_history(id,tenant_id,product_id,cost_fils,effective_from,source,created_at) VALUES(?1,?2,?3,?4,?5,'RECEIVING_WEIGHTED_AVERAGE',?5)",params![Uuid::new_v4().to_string(),request.context.tenant_id.to_string(),line.product_id.to_string(),valuation.weighted_average_cost.0,request.now.to_rfc3339()])?;
            accepted_total = accepted_total
                .checked_add(accepted)
                .ok_or(bhaipos_core::MoneyError::Overflow)?;
            value_total = value_total.checked_add(price_times_quantity(
                line.unit_cost,
                QuantityMilli(accepted),
            )?)?;
        }
        if discrepancy {
            tx.execute(
                "UPDATE goods_receipts SET status='REQUIRES_REVIEW' WHERE id=?1",
                params![receipt_id.to_string()],
            )?;
        }
        let result = InventoryReceiptResult {
            receipt_id,
            accepted_quantity: QuantityMilli(accepted_total),
            inventory_value: value_total,
        };
        Self::record_inventory_operation(
            &tx,
            request.context.tenant_id,
            request.operation_id,
            "RECEIVE",
            &digest,
            &result,
            request.now,
        )?;
        Self::append_audit(
            &tx,
            request.context.tenant_id,
            request.context.device_id,
            request.user_id,
            "INVENTORY_RECEIVED",
            "goods_receipt",
            &receipt_id.to_string(),
            &serde_json::to_string(&result)?,
            request.now,
        )?;
        Self::enqueue_inventory_sync(
            &tx,
            request.context,
            request.operation_id,
            "inventory_receipt",
            &receipt_id.to_string(),
            &result,
            request.now,
        )?;
        tx.commit()?;
        Ok(result)
    }

    #[allow(
        clippy::too_many_arguments,
        reason = "waste evidence keeps inventory authority and valuation inputs explicit"
    )]
    pub fn record_waste(
        &mut self,
        context: LocalTerminalContext,
        user: UserId,
        operation_id: OperationId,
        centre_id: Uuid,
        product_id: ProductId,
        lot_id: Option<Uuid>,
        quantity: QuantityMilli,
        waste_type: &str,
        reason: Option<&str>,
        now: DateTime<Utc>,
    ) -> Result<WasteResult, StoreError> {
        self.validate_local_session(context, user)?;
        let digest = sha256_hex(&serde_json::to_vec(
            &serde_json::json!({"centre_id":centre_id,"product_id":product_id,"lot_id":lot_id,"quantity":quantity,"waste_type":waste_type,"reason":reason}),
        )?);
        if let Some(result) =
            self.load_inventory_operation(context.tenant_id, operation_id, "WASTE", &digest)?
        {
            return Ok(result);
        }
        if quantity.0 <= 0 {
            return Err(StoreError::Validation(
                "waste quantity must be positive".into(),
            ));
        }
        if !matches!(
            waste_type,
            "EXPIRED"
                | "DAMAGED"
                | "SPOILED"
                | "BROKEN"
                | "PRODUCTION"
                | "CUSTOMER_DAMAGE"
                | "OTHER"
        ) {
            return Err(StoreError::Validation("invalid waste type".into()));
        }
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        Self::assert_permission(
            &tx,
            context.tenant_id,
            context.branch_id,
            user,
            "inventory.waste",
        )?;
        Self::assert_inventory_centre(&tx, context.tenant_id, context.branch_id, centre_id)?;
        Self::assert_product(&tx, context.tenant_id, product_id)?;
        let valuation = Self::inventory_valuation_tx(
            &tx,
            context.tenant_id,
            context.branch_id,
            centre_id,
            product_id,
        )?;
        if valuation.quantity.0 < quantity.0 {
            return Err(StoreError::Conflict("insufficient stock for waste".into()));
        }
        let cost_value = price_times_quantity(valuation.weighted_average_cost, quantity)?;
        let waste_id = Uuid::new_v4();
        tx.execute("INSERT INTO waste_events(id,tenant_id,branch_id,centre_id,product_id,lot_id,waste_type,quantity_milli,cost_value_fils,reason,responsible_user_id,operation_id,device_id,created_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14)",params![waste_id.to_string(),context.tenant_id.to_string(),context.branch_id.to_string(),centre_id.to_string(),product_id.to_string(),lot_id.map(|id|id.to_string()),waste_type,quantity.0,cost_value.0,reason,user.to_string(),operation_id.to_string(),context.device_id.to_string(),now.to_rfc3339()])?;
        Self::append_inventory_effect(
            &tx,
            context,
            user,
            operation_id,
            centre_id,
            product_id,
            QuantityMilli(-quantity.0),
            valuation.weighted_average_cost,
            "WASTE",
            "WASTE",
            &waste_id.to_string(),
            lot_id,
            now,
        )?;
        let result = WasteResult {
            waste_id,
            cost_value,
        };
        Self::record_inventory_operation(
            &tx,
            context.tenant_id,
            operation_id,
            "WASTE",
            &digest,
            &result,
            now,
        )?;
        Self::append_audit(
            &tx,
            context.tenant_id,
            context.device_id,
            user,
            "INVENTORY_WASTE_RECORDED",
            "waste",
            &waste_id.to_string(),
            &serde_json::to_string(&result)?,
            now,
        )?;
        Self::enqueue_inventory_sync(
            &tx,
            context,
            operation_id,
            "inventory_waste",
            &waste_id.to_string(),
            &result,
            now,
        )?;
        tx.commit()?;
        Ok(result)
    }

    pub fn inventory_valuation(
        &self,
        context: LocalTerminalContext,
        user: UserId,
        centre_id: Uuid,
        product_id: ProductId,
    ) -> Result<InventoryValuation, StoreError> {
        self.validate_local_session(context, user)?;
        Self::assert_permission_conn(
            &self.conn,
            context.tenant_id,
            context.branch_id,
            user,
            "inventory.reconcile",
        )?;
        Self::inventory_valuation_conn(
            &self.conn,
            context.tenant_id,
            context.branch_id,
            centre_id,
            product_id,
        )
    }

    pub fn expiring_lots(
        &self,
        context: LocalTerminalContext,
        user: UserId,
        on_or_before: &str,
    ) -> Result<Vec<ExpiringLot>, StoreError> {
        self.validate_local_session(context, user)?;
        Self::assert_permission_conn(
            &self.conn,
            context.tenant_id,
            context.branch_id,
            user,
            "inventory.reconcile",
        )?;
        let mut statement=self.conn.prepare("SELECT l.id,l.product_id,l.lot_number,l.expires_on,lb.quantity_milli FROM inventory_lots l JOIN lot_balances lb ON lb.lot_id=l.id WHERE l.tenant_id=?1 AND lb.branch_id=?2 AND l.status='ACTIVE' AND l.expires_on IS NOT NULL AND l.expires_on<=?3 AND lb.quantity_milli>0 ORDER BY l.expires_on,l.id")?;
        let raw = statement
            .query_map(
                params![
                    context.tenant_id.to_string(),
                    context.branch_id.to_string(),
                    on_or_before
                ],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, Option<String>>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, i64>(4)?,
                    ))
                },
            )?
            .collect::<Result<Vec<_>, _>>()?;
        raw.into_iter()
            .map(|(lot, product, lot_number, expires_on, quantity)| {
                Ok(ExpiringLot {
                    lot_id: Uuid::parse_str(&lot)
                        .map_err(|_| StoreError::Validation("invalid lot identity".into()))?,
                    product_id: ProductId(Uuid::parse_str(&product).map_err(|_| {
                        StoreError::Validation("invalid lot product identity".into())
                    })?),
                    lot_number,
                    expires_on,
                    quantity: QuantityMilli(quantity),
                })
            })
            .collect()
    }

    pub fn rebuild_stock_cache(
        &mut self,
        context: LocalTerminalContext,
        user: UserId,
        repair: bool,
        now: DateTime<Utc>,
    ) -> Result<InventoryRebuildResult, StoreError> {
        self.validate_local_session(context, user)?;
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        Self::assert_permission(
            &tx,
            context.tenant_id,
            context.branch_id,
            user,
            "inventory.reconcile",
        )?;
        let movements = {
            let mut statement=tx.prepare("SELECT centre_id,product_id,quantity_milli,unit_cost_fils FROM inventory_movements WHERE tenant_id=?1 AND branch_id=?2 ORDER BY created_at,id")?;
            let rows = statement.query_map(
                params![context.tenant_id.to_string(), context.branch_id.to_string()],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, i64>(3)?,
                    ))
                },
            )?;
            rows.collect::<Result<Vec<_>, _>>()?
        };
        let mut ledger: HashMap<(String, String), (i64, Money)> = HashMap::new();
        for (centre, product, quantity, cost) in movements {
            let entry = ledger.entry((centre, product)).or_insert((0, Money::ZERO));
            entry.0 = entry
                .0
                .checked_add(quantity)
                .ok_or(bhaipos_core::MoneyError::Overflow)?;
            let absolute = QuantityMilli(
                quantity
                    .checked_abs()
                    .ok_or(bhaipos_core::MoneyError::Overflow)?,
            );
            let value = price_times_quantity(Money(cost), absolute)?;
            entry.1 = if quantity < 0 {
                entry.1.checked_sub(value)?
            } else {
                entry.1.checked_add(value)?
            };
        }
        let cached_keys = {
            let mut statement = tx.prepare(
                "SELECT centre_id,product_id FROM stock_levels WHERE tenant_id=?1 AND branch_id=?2",
            )?;
            let rows = statement.query_map(
                params![context.tenant_id.to_string(), context.branch_id.to_string()],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
            )?;
            rows.collect::<Result<Vec<_>, _>>()?
        };
        for key in cached_keys {
            ledger.entry(key).or_insert((0, Money::ZERO));
        }
        let mut mismatches = 0_i64;
        let mut evidence = Vec::new();
        for ((centre, product), (quantity, value)) in ledger {
            let cached:Option<i64>=tx.query_row("SELECT quantity_milli FROM stock_levels WHERE tenant_id=?1 AND branch_id=?2 AND centre_id=?3 AND product_id=?4",params![context.tenant_id.to_string(),context.branch_id.to_string(),centre,product],|row|row.get(0)).optional()?;
            if cached != Some(quantity) {
                mismatches += 1;
                evidence.push(serde_json::json!({"centre_id":centre,"product_id":product,"cached":cached,"ledger":quantity}));
                if repair {
                    tx.execute("INSERT INTO stock_levels(tenant_id,branch_id,centre_id,product_id,quantity_milli,version) VALUES(?1,?2,?3,?4,?5,1) ON CONFLICT(tenant_id,branch_id,centre_id,product_id) DO UPDATE SET quantity_milli=excluded.quantity_milli,version=stock_levels.version+1",params![context.tenant_id.to_string(),context.branch_id.to_string(),centre,product,quantity])?;
                }
            }
            if repair {
                tx.execute("INSERT INTO inventory_cost_balances(tenant_id,branch_id,centre_id,product_id,quantity_milli,total_value_fils,version) VALUES(?1,?2,?3,?4,?5,?6,1) ON CONFLICT(tenant_id,branch_id,centre_id,product_id) DO UPDATE SET quantity_milli=excluded.quantity_milli,total_value_fils=excluded.total_value_fils,version=inventory_cost_balances.version+1",params![context.tenant_id.to_string(),context.branch_id.to_string(),centre,product,quantity,value.0])?;
            }
        }
        let reconciliation_id = Uuid::new_v4();
        tx.execute("INSERT INTO inventory_reconciliation_runs(id,tenant_id,branch_id,device_id,user_id,mismatches,repaired,evidence_json,created_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)",params![reconciliation_id.to_string(),context.tenant_id.to_string(),context.branch_id.to_string(),context.device_id.to_string(),user.to_string(),mismatches,repair as i32,serde_json::to_string(&evidence)?,now.to_rfc3339()])?;
        Self::append_audit(
            &tx,
            context.tenant_id,
            context.device_id,
            user,
            "INVENTORY_RECONCILED",
            "inventory_reconciliation",
            &reconciliation_id.to_string(),
            &serde_json::json!({"mismatches":mismatches,"repaired":repair}).to_string(),
            now,
        )?;
        tx.commit()?;
        Ok(InventoryRebuildResult {
            reconciliation_id,
            mismatches,
            repaired: repair,
        })
    }

    #[allow(
        clippy::too_many_arguments,
        reason = "transfer creation keeps both centres and trusted authority explicit"
    )]
    pub fn create_inventory_transfer(
        &mut self,
        context: LocalTerminalContext,
        user: UserId,
        operation_id: OperationId,
        from_centre: Uuid,
        to_branch: BranchId,
        to_centre: Uuid,
        lines: &[InventoryTransferLineInput],
        now: DateTime<Utc>,
    ) -> Result<InventoryTransferResult, StoreError> {
        self.validate_local_session(context, user)?;
        if lines.is_empty() {
            return Err(StoreError::Validation("transfer has no lines".into()));
        }
        let digest = sha256_hex(&serde_json::to_vec(
            &serde_json::json!({"from_centre":from_centre,"to_branch":to_branch,"to_centre":to_centre,"lines":lines}),
        )?);
        if let Some(result) = self.load_inventory_operation(
            context.tenant_id,
            operation_id,
            "TRANSFER_CREATE",
            &digest,
        )? {
            return Ok(result);
        }
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        Self::assert_permission(
            &tx,
            context.tenant_id,
            context.branch_id,
            user,
            "inventory.transfer",
        )?;
        Self::assert_inventory_centre(&tx, context.tenant_id, context.branch_id, from_centre)?;
        let target: Option<i32> = tx
            .query_row(
                "SELECT 1 FROM inventory_centres WHERE id=?1 AND tenant_id=?2 AND branch_id=?3",
                params![
                    to_centre.to_string(),
                    context.tenant_id.to_string(),
                    to_branch.to_string()
                ],
                |row| row.get(0),
            )
            .optional()?;
        if target.is_none() {
            return Err(StoreError::Authorization(
                "transfer destination scope mismatch",
            ));
        }
        let transfer_id = Uuid::new_v4();
        tx.execute("INSERT INTO inventory_transfers(id,tenant_id,from_branch_id,to_branch_id,from_centre_id,to_centre_id,status,operation_id,created_by_user_id,device_id,created_at) VALUES(?1,?2,?3,?4,?5,?6,'DRAFT',?7,?8,?9,?10)",params![transfer_id.to_string(),context.tenant_id.to_string(),context.branch_id.to_string(),to_branch.to_string(),from_centre.to_string(),to_centre.to_string(),operation_id.to_string(),user.to_string(),context.device_id.to_string(),now.to_rfc3339()])?;
        let mut products = HashSet::new();
        for line in lines {
            if line.quantity.0 <= 0 {
                return Err(StoreError::Validation(
                    "transfer quantity must be positive".into(),
                ));
            }
            if !products.insert((line.product_id, line.lot_id)) {
                return Err(StoreError::Validation(
                    "duplicate product/lot transfer line".into(),
                ));
            }
            Self::assert_product(&tx, context.tenant_id, line.product_id)?;
            tx.execute("INSERT INTO inventory_transfer_lines(id,transfer_id,product_id,lot_id,requested_qty_milli) VALUES(?1,?2,?3,?4,?5)",params![Uuid::new_v4().to_string(),transfer_id.to_string(),line.product_id.to_string(),line.lot_id.map(|id|id.to_string()),line.quantity.0])?;
        }
        let result = InventoryTransferResult {
            transfer_id,
            status: "DRAFT".into(),
        };
        Self::record_inventory_operation(
            &tx,
            context.tenant_id,
            operation_id,
            "TRANSFER_CREATE",
            &digest,
            &result,
            now,
        )?;
        tx.commit()?;
        Ok(result)
    }

    pub fn dispatch_transfer(
        &mut self,
        context: LocalTerminalContext,
        user: UserId,
        transfer_id: Uuid,
        operation_id: OperationId,
        now: DateTime<Utc>,
    ) -> Result<InventoryTransferResult, StoreError> {
        self.validate_local_session(context, user)?;
        let digest = sha256_hex(&serde_json::to_vec(
            &serde_json::json!({"transfer_id":transfer_id}),
        )?);
        if let Some(result) = self.load_inventory_operation(
            context.tenant_id,
            operation_id,
            "TRANSFER_DISPATCH",
            &digest,
        )? {
            return Ok(result);
        }
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        Self::assert_permission(
            &tx,
            context.tenant_id,
            context.branch_id,
            user,
            "inventory.transfer",
        )?;
        let transfer:Option<(String,String)>=tx.query_row("SELECT from_centre_id,status FROM inventory_transfers WHERE id=?1 AND tenant_id=?2 AND from_branch_id=?3",params![transfer_id.to_string(),context.tenant_id.to_string(),context.branch_id.to_string()],|row|Ok((row.get(0)?,row.get(1)?))).optional()?;
        let (centre, status) = transfer.ok_or(StoreError::NotFound("outbound transfer"))?;
        if status != "DRAFT" {
            return Err(StoreError::Conflict(
                "only draft transfers can be dispatched".into(),
            ));
        }
        let centre_id = Uuid::parse_str(&centre)
            .map_err(|_| StoreError::Validation("invalid transfer centre".into()))?;
        let lines = {
            let mut statement=tx.prepare("SELECT id,product_id,lot_id,requested_qty_milli FROM inventory_transfer_lines WHERE transfer_id=?1")?;
            let rows = statement.query_map(params![transfer_id.to_string()], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, i64>(3)?,
                ))
            })?;
            rows.collect::<Result<Vec<_>, _>>()?
        };
        for (line, product, lot, quantity) in lines {
            let product_id = ProductId(
                Uuid::parse_str(&product)
                    .map_err(|_| StoreError::Validation("invalid transfer product".into()))?,
            );
            let valuation = Self::inventory_valuation_tx(
                &tx,
                context.tenant_id,
                context.branch_id,
                centre_id,
                product_id,
            )?;
            if valuation.quantity.0 < quantity {
                return Err(StoreError::Conflict(
                    "insufficient stock to dispatch transfer".into(),
                ));
            }
            Self::append_inventory_effect(
                &tx,
                context,
                user,
                operation_id,
                centre_id,
                product_id,
                QuantityMilli(-quantity),
                valuation.weighted_average_cost,
                "TRANSFER_DISPATCH",
                "TRANSFER_LINE",
                &line,
                lot.map(|value| {
                    Uuid::parse_str(&value)
                        .map_err(|_| StoreError::Validation("invalid transfer lot".into()))
                })
                .transpose()?,
                now,
            )?;
            tx.execute(
                "UPDATE inventory_transfer_lines SET dispatched_qty_milli=?2 WHERE id=?1",
                params![line, quantity],
            )?;
        }
        tx.execute(
            "UPDATE inventory_transfers SET status='DISPATCHED',dispatched_at=?2 WHERE id=?1",
            params![transfer_id.to_string(), now.to_rfc3339()],
        )?;
        let result = InventoryTransferResult {
            transfer_id,
            status: "DISPATCHED".into(),
        };
        Self::record_inventory_operation(
            &tx,
            context.tenant_id,
            operation_id,
            "TRANSFER_DISPATCH",
            &digest,
            &result,
            now,
        )?;
        Self::append_audit(
            &tx,
            context.tenant_id,
            context.device_id,
            user,
            "INVENTORY_TRANSFER_DISPATCHED",
            "inventory_transfer",
            &transfer_id.to_string(),
            &serde_json::to_string(&result)?,
            now,
        )?;
        Self::enqueue_inventory_sync(
            &tx,
            context,
            operation_id,
            "inventory_transfer_dispatch",
            &transfer_id.to_string(),
            &result,
            now,
        )?;
        tx.commit()?;
        Ok(result)
    }

    pub fn receive_transfer(
        &mut self,
        context: LocalTerminalContext,
        user: UserId,
        transfer_id: Uuid,
        operation_id: OperationId,
        lines: &[TransferReceiptLineInput],
        now: DateTime<Utc>,
    ) -> Result<InventoryTransferResult, StoreError> {
        self.validate_local_session(context, user)?;
        if lines.is_empty() {
            return Err(StoreError::Validation(
                "transfer receipt has no lines".into(),
            ));
        }
        let digest = sha256_hex(&serde_json::to_vec(
            &serde_json::json!({"transfer_id":transfer_id,"lines":lines}),
        )?);
        if let Some(result) = self.load_inventory_operation(
            context.tenant_id,
            operation_id,
            "TRANSFER_RECEIVE",
            &digest,
        )? {
            return Ok(result);
        }
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        Self::assert_permission(
            &tx,
            context.tenant_id,
            context.branch_id,
            user,
            "inventory.transfer",
        )?;
        let transfer:Option<(String,String)>=tx.query_row("SELECT to_centre_id,status FROM inventory_transfers WHERE id=?1 AND tenant_id=?2 AND to_branch_id=?3",params![transfer_id.to_string(),context.tenant_id.to_string(),context.branch_id.to_string()],|row|Ok((row.get(0)?,row.get(1)?))).optional()?;
        let (centre, status) = transfer.ok_or(StoreError::NotFound("inbound transfer"))?;
        if !matches!(status.as_str(), "DISPATCHED" | "PARTIALLY_RECEIVED") {
            return Err(StoreError::Conflict("transfer is not receivable".into()));
        }
        let centre_id = Uuid::parse_str(&centre)
            .map_err(|_| StoreError::Validation("invalid transfer centre".into()))?;
        let receipt_id = Uuid::new_v4();
        tx.execute("INSERT INTO inventory_transfer_receipts(id,tenant_id,transfer_id,operation_id,device_id,user_id,received_at) VALUES(?1,?2,?3,?4,?5,?6,?7)",params![receipt_id.to_string(),context.tenant_id.to_string(),transfer_id.to_string(),operation_id.to_string(),context.device_id.to_string(),user.to_string(),now.to_rfc3339()])?;
        let mut unique = HashSet::new();
        for input in lines {
            if !unique.insert(input.transfer_line_id) {
                return Err(StoreError::Validation(
                    "duplicate transfer receipt line".into(),
                ));
            }
            let applied = input
                .received_quantity
                .0
                .checked_add(input.damaged_quantity.0)
                .ok_or(bhaipos_core::MoneyError::Overflow)?;
            if input.received_quantity.0 < 0 || input.damaged_quantity.0 < 0 || applied <= 0 {
                return Err(StoreError::Validation(
                    "invalid transfer receipt quantity".into(),
                ));
            }
            let row:Option<(String,Option<String>,i64,i64,i64)>=tx.query_row("SELECT product_id,lot_id,dispatched_qty_milli,received_qty_milli,damaged_qty_milli FROM inventory_transfer_lines WHERE id=?1 AND transfer_id=?2",params![input.transfer_line_id.to_string(),transfer_id.to_string()],|row|Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?,row.get(4)?))).optional()?;
            let (product, lot, dispatched, received, damaged) =
                row.ok_or(StoreError::NotFound("transfer line"))?;
            if received
                .checked_add(damaged)
                .and_then(|v| v.checked_add(applied))
                .ok_or(bhaipos_core::MoneyError::Overflow)?
                > dispatched
            {
                return Err(StoreError::Validation(
                    "transfer receipt exceeds dispatched quantity".into(),
                ));
            }
            let product_id = ProductId(
                Uuid::parse_str(&product)
                    .map_err(|_| StoreError::Validation("invalid transfer product".into()))?,
            );
            let unit_cost:i64=tx.query_row("SELECT unit_cost_fils FROM inventory_movements WHERE source_type='TRANSFER_LINE' AND source_id=?1 AND movement_type='TRANSFER_DISPATCH'",params![input.transfer_line_id.to_string()],|row|row.get(0))?;
            if input.received_quantity.0 > 0 {
                Self::append_inventory_effect(
                    &tx,
                    context,
                    user,
                    operation_id,
                    centre_id,
                    product_id,
                    input.received_quantity,
                    Money(unit_cost),
                    "TRANSFER_RECEIPT",
                    "TRANSFER_RECEIPT_LINE",
                    &input.transfer_line_id.to_string(),
                    lot.as_ref()
                        .map(|value| {
                            Uuid::parse_str(value)
                                .map_err(|_| StoreError::Validation("invalid transfer lot".into()))
                        })
                        .transpose()?,
                    now,
                )?;
            }
            tx.execute("UPDATE inventory_transfer_lines SET received_qty_milli=received_qty_milli+?2,damaged_qty_milli=damaged_qty_milli+?3,discrepancy_note=COALESCE(?4,discrepancy_note) WHERE id=?1",params![input.transfer_line_id.to_string(),input.received_quantity.0,input.damaged_quantity.0,input.note])?;
            tx.execute("INSERT INTO inventory_transfer_receipt_lines(id,receipt_id,transfer_line_id,received_qty_milli,damaged_qty_milli,note) VALUES(?1,?2,?3,?4,?5,?6)",params![Uuid::new_v4().to_string(),receipt_id.to_string(),input.transfer_line_id.to_string(),input.received_quantity.0,input.damaged_quantity.0,input.note])?;
        }
        let remaining:i64=tx.query_row("SELECT COUNT(*) FROM inventory_transfer_lines WHERE transfer_id=?1 AND received_qty_milli+damaged_qty_milli<dispatched_qty_milli",params![transfer_id.to_string()],|row|row.get(0))?;
        let next = if remaining == 0 {
            "RECEIVED"
        } else {
            "PARTIALLY_RECEIVED"
        };
        tx.execute("UPDATE inventory_transfers SET status=?2,completed_at=CASE WHEN ?2='RECEIVED' THEN ?3 ELSE completed_at END WHERE id=?1",params![transfer_id.to_string(),next,now.to_rfc3339()])?;
        let result = InventoryTransferResult {
            transfer_id,
            status: next.into(),
        };
        Self::record_inventory_operation(
            &tx,
            context.tenant_id,
            operation_id,
            "TRANSFER_RECEIVE",
            &digest,
            &result,
            now,
        )?;
        Self::append_audit(
            &tx,
            context.tenant_id,
            context.device_id,
            user,
            "INVENTORY_TRANSFER_RECEIVED",
            "inventory_transfer_receipt",
            &receipt_id.to_string(),
            &serde_json::to_string(&result)?,
            now,
        )?;
        Self::enqueue_inventory_sync(
            &tx,
            context,
            operation_id,
            "inventory_transfer_receipt",
            &receipt_id.to_string(),
            &result,
            now,
        )?;
        tx.commit()?;
        Ok(result)
    }

    pub fn create_stocktake(
        &mut self,
        context: LocalTerminalContext,
        user: UserId,
        centre_id: Uuid,
        product_ids: &[ProductId],
        now: DateTime<Utc>,
    ) -> Result<Uuid, StoreError> {
        self.validate_local_session(context, user)?;
        if product_ids.is_empty() {
            return Err(StoreError::Validation(
                "stocktake scope has no products".into(),
            ));
        }
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        Self::assert_permission(
            &tx,
            context.tenant_id,
            context.branch_id,
            user,
            "inventory.stocktake",
        )?;
        Self::assert_inventory_centre(&tx, context.tenant_id, context.branch_id, centre_id)?;
        let stocktake_id = Uuid::new_v4();
        tx.execute("INSERT INTO stocktakes(id,tenant_id,branch_id,centre_id,status,snapshot_at,created_by_user_id,created_at) VALUES(?1,?2,?3,?4,'COUNTING',?5,?6,?5)",params![stocktake_id.to_string(),context.tenant_id.to_string(),context.branch_id.to_string(),centre_id.to_string(),now.to_rfc3339(),user.to_string()])?;
        let mut unique = HashSet::new();
        for product in product_ids {
            if !unique.insert(*product) {
                return Err(StoreError::Validation(
                    "duplicate product in stocktake scope".into(),
                ));
            }
            Self::assert_product(&tx, context.tenant_id, *product)?;
            let expected:i64=tx.query_row("SELECT COALESCE(quantity_milli,0) FROM stock_levels WHERE tenant_id=?1 AND branch_id=?2 AND centre_id=?3 AND product_id=?4",params![context.tenant_id.to_string(),context.branch_id.to_string(),centre_id.to_string(),product.to_string()],|row|row.get(0)).optional()?.unwrap_or(0);
            tx.execute("INSERT INTO stocktake_lines(id,stocktake_id,product_id,expected_qty_milli,status) VALUES(?1,?2,?3,?4,'OPEN')",params![Uuid::new_v4().to_string(),stocktake_id.to_string(),product.to_string(),expected])?;
        }
        tx.commit()?;
        Ok(stocktake_id)
    }

    pub fn count_stocktake_line(
        &mut self,
        context: LocalTerminalContext,
        user: UserId,
        stocktake_id: Uuid,
        product_id: ProductId,
        counted: QuantityMilli,
        now: DateTime<Utc>,
    ) -> Result<(), StoreError> {
        self.validate_local_session(context, user)?;
        if counted.0 < 0 {
            return Err(StoreError::Validation(
                "stocktake count cannot be negative".into(),
            ));
        }
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        Self::assert_permission(
            &tx,
            context.tenant_id,
            context.branch_id,
            user,
            "inventory.stocktake",
        )?;
        let line:Option<String>=tx.query_row("SELECT sl.id FROM stocktake_lines sl JOIN stocktakes s ON s.id=sl.stocktake_id WHERE s.id=?1 AND s.tenant_id=?2 AND s.branch_id=?3 AND s.status='COUNTING' AND sl.product_id=?4",params![stocktake_id.to_string(),context.tenant_id.to_string(),context.branch_id.to_string(),product_id.to_string()],|row|row.get(0)).optional()?;
        let line = line.ok_or(StoreError::NotFound("open stocktake line"))?;
        tx.execute("INSERT INTO stocktake_count_events(id,tenant_id,stocktake_id,stocktake_line_id,counted_qty_milli,device_id,user_id,counted_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",params![Uuid::new_v4().to_string(),context.tenant_id.to_string(),stocktake_id.to_string(),line,counted.0,context.device_id.to_string(),user.to_string(),now.to_rfc3339()])?;
        tx.execute(
            "UPDATE stocktake_lines SET counted_qty_milli=?2,status='COUNTED' WHERE id=?1",
            params![line, counted.0],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn approve_stocktake(
        &mut self,
        context: LocalTerminalContext,
        user: UserId,
        stocktake_id: Uuid,
        operation_id: OperationId,
        now: DateTime<Utc>,
    ) -> Result<StocktakeApprovalResult, StoreError> {
        self.validate_local_session(context, user)?;
        let digest = sha256_hex(&serde_json::to_vec(
            &serde_json::json!({"stocktake_id":stocktake_id}),
        )?);
        if let Some(result) = self.load_inventory_operation(
            context.tenant_id,
            operation_id,
            "STOCKTAKE_APPROVE",
            &digest,
        )? {
            return Ok(result);
        }
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        Self::assert_permission(
            &tx,
            context.tenant_id,
            context.branch_id,
            user,
            "inventory.stocktake",
        )?;
        let header:Option<(String,String)>=tx.query_row("SELECT centre_id,snapshot_at FROM stocktakes WHERE id=?1 AND tenant_id=?2 AND branch_id=?3 AND status='COUNTING'",params![stocktake_id.to_string(),context.tenant_id.to_string(),context.branch_id.to_string()],|row|Ok((row.get(0)?,row.get(1)?))).optional()?;
        let (centre, snapshot) =
            header.ok_or(StoreError::Conflict("stocktake is not approvable".into()))?;
        let centre_id = Uuid::parse_str(&centre)
            .map_err(|_| StoreError::Validation("invalid stocktake centre".into()))?;
        let lines = {
            let mut statement=tx.prepare("SELECT id,product_id,expected_qty_milli,counted_qty_milli FROM stocktake_lines WHERE stocktake_id=?1")?;
            let rows = statement.query_map(params![stocktake_id.to_string()], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, Option<i64>>(3)?,
                ))
            })?;
            rows.collect::<Result<Vec<_>, _>>()?
        };
        let mut adjustments = 0_i64;
        let mut evidence = Vec::new();
        for (line, product, expected, counted) in lines {
            let counted = counted.ok_or_else(|| {
                StoreError::Validation("every stocktake line requires a count".into())
            })?;
            let after:i64=tx.query_row("SELECT COALESCE(SUM(quantity_milli),0) FROM inventory_movements WHERE tenant_id=?1 AND branch_id=?2 AND centre_id=?3 AND product_id=?4 AND created_at>?5",params![context.tenant_id.to_string(),context.branch_id.to_string(),centre_id.to_string(),product,snapshot],|row|row.get(0))?;
            let reconciled = expected
                .checked_add(after)
                .ok_or(bhaipos_core::MoneyError::Overflow)?;
            let variance = counted
                .checked_sub(reconciled)
                .ok_or(bhaipos_core::MoneyError::Overflow)?;
            if variance != 0 {
                let product_id = ProductId(
                    Uuid::parse_str(&product)
                        .map_err(|_| StoreError::Validation("invalid stocktake product".into()))?,
                );
                let valuation = Self::inventory_valuation_tx(
                    &tx,
                    context.tenant_id,
                    context.branch_id,
                    centre_id,
                    product_id,
                )?;
                Self::append_inventory_effect(
                    &tx,
                    context,
                    user,
                    operation_id,
                    centre_id,
                    product_id,
                    QuantityMilli(variance),
                    valuation.weighted_average_cost,
                    "STOCKTAKE",
                    "STOCKTAKE_LINE",
                    &line,
                    None,
                    now,
                )?;
                adjustments += 1;
            }
            tx.execute("UPDATE stocktake_lines SET movements_after_snapshot_milli=?2,reconciled_expected_qty_milli=?3,variance_qty_milli=?4,status='APPROVED' WHERE id=?1",params![line,after,reconciled,variance])?;
            evidence.push(serde_json::json!({"line_id":line,"expected":expected,"movements_after_snapshot":after,"counted":counted,"variance":variance}));
        }
        tx.execute("UPDATE stocktakes SET status='APPROVED',approved_by_user_id=?2,operation_id=?3,approved_at=?4 WHERE id=?1",params![stocktake_id.to_string(),user.to_string(),operation_id.to_string(),now.to_rfc3339()])?;
        tx.execute("INSERT INTO stocktake_approvals(stocktake_id,tenant_id,operation_id,approved_by_user_id,device_id,evidence_json,approved_at) VALUES(?1,?2,?3,?4,?5,?6,?7)",params![stocktake_id.to_string(),context.tenant_id.to_string(),operation_id.to_string(),user.to_string(),context.device_id.to_string(),serde_json::to_string(&evidence)?,now.to_rfc3339()])?;
        let result = StocktakeApprovalResult {
            stocktake_id,
            adjustments,
        };
        Self::record_inventory_operation(
            &tx,
            context.tenant_id,
            operation_id,
            "STOCKTAKE_APPROVE",
            &digest,
            &result,
            now,
        )?;
        Self::append_audit(
            &tx,
            context.tenant_id,
            context.device_id,
            user,
            "STOCKTAKE_APPROVED",
            "stocktake",
            &stocktake_id.to_string(),
            &serde_json::to_string(&result)?,
            now,
        )?;
        Self::enqueue_inventory_sync(
            &tx,
            context,
            operation_id,
            "inventory_stocktake",
            &stocktake_id.to_string(),
            &result,
            now,
        )?;
        tx.commit()?;
        Ok(result)
    }

    pub fn receipt_snapshot(&self, sale: SaleId) -> Result<(String, String), StoreError> {
        self.conn
            .query_row(
                "SELECT receipt_text,receipt_sha256 FROM receipt_snapshots WHERE sale_id=?1",
                params![sale.to_string()],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?
            .ok_or(StoreError::NotFound("receipt snapshot"))
    }
    pub fn stock_quantity(
        &self,
        tenant: TenantId,
        branch: BranchId,
        product: ProductId,
    ) -> Result<i64, StoreError> {
        Ok(self.conn.query_row("SELECT COALESCE(SUM(quantity_milli),0) FROM stock_levels WHERE tenant_id=?1 AND branch_id=?2 AND product_id=?3",params![tenant.to_string(),branch.to_string(),product.to_string()],|r|r.get(0))?)
    }
    pub fn unknown_barcode_count(
        &self,
        tenant: TenantId,
        branch: BranchId,
        barcode: &str,
    ) -> Result<i64, StoreError> {
        Ok(self.conn.query_row("SELECT scan_count FROM unknown_barcodes WHERE tenant_id=?1 AND branch_id=?2 AND barcode=?3",params![tenant.to_string(),branch.to_string(),barcode],|r|r.get(0)).optional()?.unwrap_or(0))
    }
    pub fn audit_chain_valid(
        &self,
        tenant: TenantId,
        device: DeviceId,
    ) -> Result<bool, StoreError> {
        Self::audit_chain_valid_on(&self.conn, tenant, device)
    }
    fn audit_chain_valid_on(
        conn: &Connection,
        tenant: TenantId,
        device: DeviceId,
    ) -> Result<bool, StoreError> {
        let tenant_id = tenant.to_string();
        let device_id = device.to_string();
        let mut stmt=conn.prepare("SELECT actor_user_id,event_type,entity_type,entity_id,payload_json,previous_hash,event_hash,created_at FROM audit_events WHERE tenant_id=?1 AND device_id=?2")?;
        let rows = stmt.query_map(params![tenant.to_string(), device.to_string()], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, String>(5)?,
                r.get::<_, String>(6)?,
                r.get::<_, String>(7)?,
            ))
        })?;
        let events = rows.collect::<Result<Vec<_>, _>>()?;
        let mut successors: HashMap<&str, Vec<usize>> = HashMap::new();
        for (index, event) in events.iter().enumerate() {
            successors.entry(event.5.as_str()).or_default().push(index);
        }
        let mut previous = String::new();
        let mut visited = HashSet::new();
        while visited.len() < events.len() {
            let Some(candidates) = successors.get(previous.as_str()) else {
                return Ok(false);
            };
            if candidates.len() != 1 {
                return Ok(false);
            }
            let index = candidates[0];
            if !visited.insert(index) {
                return Ok(false);
            }
            let (
                actor,
                event_type,
                entity_type,
                entity_id,
                payload,
                stored_previous,
                hash,
                created,
            ) = &events[index];
            let created_at = DateTime::parse_from_rfc3339(created)
                .map_err(|_| StoreError::Validation("invalid audit timestamp".into()))?
                .with_timezone(&Utc);
            let material = AuditMaterial {
                tenant_id: &tenant_id,
                device_id: &device_id,
                actor_user_id: actor,
                event_type,
                entity_type,
                entity_id,
                payload_json: payload,
                created_at,
                previous_hash: stored_previous,
            };
            if compute_audit_hash(&material) != *hash {
                return Ok(false);
            }
            previous = hash.clone();
        }
        Ok(true)
    }

    fn canonical_payments(
        payments: &[PaymentInput],
    ) -> Vec<(String, i64, Option<i64>, Option<String>)> {
        let mut normalized = payments
            .iter()
            .map(|payment| {
                let reference = payment
                    .reference
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(|value| value.to_owned());
                (
                    Self::tender_kind_name(&payment.kind).to_owned(),
                    payment.amount.0,
                    payment.tendered.map(|value| value.0),
                    reference,
                )
            })
            .collect::<Vec<_>>();
        normalized.sort();
        normalized
    }
    fn tender_kind_name(kind: &TenderKind) -> &'static str {
        match kind {
            TenderKind::Cash => "CASH",
            TenderKind::Card => "CARD",
            TenderKind::BenefitPay => "BENEFIT_PAY",
            TenderKind::BankTransfer => "BANK_TRANSFER",
            TenderKind::CustomerCredit => "CUSTOMER_CREDIT",
            TenderKind::Custom => "CUSTOM",
        }
    }
    fn checkout_request_sha256(req: &CheckoutRequest) -> Result<String, StoreError> {
        let normalized = (
            "CHECKOUT:v1",
            req.tenant_id.to_string(),
            req.branch_id.to_string(),
            req.device_id.to_string(),
            req.register_id.to_string(),
            req.user_id.to_string(),
            req.cart_id.to_string(),
            req.cash_session_id.map(|value| value.to_string()),
            Self::canonical_payments(&req.payments),
        );
        Ok(sha256_hex(&serde_json::to_vec(&normalized)?))
    }
    fn refund_request_sha256(req: &RefundRequest) -> Result<String, StoreError> {
        let mut lines = req
            .lines
            .iter()
            .map(|line| (line.sale_line_id.to_string(), line.quantity.0))
            .collect::<Vec<_>>();
        lines.sort();
        let normalized = (
            "REFUND:v1",
            req.tenant_id.to_string(),
            req.branch_id.to_string(),
            req.device_id.to_string(),
            req.user_id.to_string(),
            req.sale_id.to_string(),
            req.reason.trim(),
            lines,
            req.cash_session_id.map(|value| value.to_string()),
            Self::canonical_payments(&req.payments),
        );
        Ok(sha256_hex(&serde_json::to_vec(&normalized)?))
    }
    fn cash_movement_request_sha256(req: &CashMovementRequest) -> Result<String, StoreError> {
        let normalized = (
            "CASH_MOVEMENT:v1",
            req.tenant_id.to_string(),
            req.branch_id.to_string(),
            req.device_id.to_string(),
            req.user_id.to_string(),
            req.cash_session_id.to_string(),
            req.kind.as_db(),
            req.amount.0,
            req.reason.as_deref().map(str::trim),
        );
        Ok(sha256_hex(&serde_json::to_vec(&normalized)?))
    }
    fn close_cash_session_request_sha256(
        req: &CloseCashSessionRequest,
    ) -> Result<String, StoreError> {
        let normalized = (
            "CASH_SESSION_CLOSE:v1",
            req.tenant_id.to_string(),
            req.branch_id.to_string(),
            req.device_id.to_string(),
            req.user_id.to_string(),
            req.cash_session_id.to_string(),
            req.counted_cash.0,
        );
        Ok(sha256_hex(&serde_json::to_vec(&normalized)?))
    }
    fn void_sale_request_sha256(req: &VoidSaleRequest) -> Result<String, StoreError> {
        let normalized = (
            "SALE_VOID:v1",
            req.tenant_id.to_string(),
            req.branch_id.to_string(),
            req.device_id.to_string(),
            req.user_id.to_string(),
            req.sale_id.to_string(),
            req.cash_session_id.map(|value| value.to_string()),
            req.approval_ref.to_string(),
            req.reason.trim(),
        );
        Ok(sha256_hex(&serde_json::to_vec(&normalized)?))
    }
    fn load_idempotent_result<T: DeserializeOwned>(
        &self,
        tenant: TenantId,
        op: OperationId,
        action: &str,
        request_sha256: &str,
    ) -> Result<Option<T>, StoreError> {
        let binding:Option<(String,String)>=self.conn.query_row(
            "SELECT action,request_sha256 FROM idempotency_operations WHERE tenant_id=?1 AND operation_id=?2",
            params![tenant.to_string(),op.to_string()],
            |row|Ok((row.get(0)?,row.get(1)?)),
        ).optional()?;
        if let Some((existing_action, existing_sha256)) = binding {
            if existing_action != action || existing_sha256 != request_sha256 {
                return Err(StoreError::Conflict(
                    "idempotency operation reused with a different action or payload".into(),
                ));
            }
            let result_json:Option<String>=self.conn.query_row(
                "SELECT result_json FROM idempotency_results WHERE tenant_id=?1 AND operation_id=?2 AND action=?3",
                params![tenant.to_string(),op.to_string(),action],
                |row|row.get(0),
            ).optional()?;
            let result_json = result_json.ok_or_else(|| {
                StoreError::Conflict(
                    "idempotency operation exists without a committed result".into(),
                )
            })?;
            return Ok(Some(serde_json::from_str(&result_json)?));
        }
        let legacy_result: Option<i32> = self
            .conn
            .query_row(
                "SELECT 1 FROM idempotency_results WHERE tenant_id=?1 AND operation_id=?2 LIMIT 1",
                params![tenant.to_string(), op.to_string()],
                |row| row.get(0),
            )
            .optional()?;
        if legacy_result.is_some() {
            return Err(StoreError::Conflict(
                "legacy idempotency result has no request digest; manual review required".into(),
            ));
        }
        Ok(None)
    }
    fn record_idempotent_result<T: Serialize>(
        tx: &Transaction<'_>,
        tenant: TenantId,
        op: OperationId,
        action: &str,
        request_sha256: &str,
        result: &T,
        now: DateTime<Utc>,
    ) -> Result<(), StoreError> {
        tx.execute(
            "INSERT INTO idempotency_operations(tenant_id,operation_id,action,request_sha256,created_at) VALUES(?1,?2,?3,?4,?5)",
            params![tenant.to_string(),op.to_string(),action,request_sha256,now.to_rfc3339()],
        )?;
        tx.execute(
            "INSERT INTO idempotency_results(tenant_id,operation_id,action,result_json,committed_at) VALUES(?1,?2,?3,?4,?5)",
            params![tenant.to_string(),op.to_string(),action,serde_json::to_string(result)?,now.to_rfc3339()],
        )?;
        Ok(())
    }
    fn assert_active_device(
        &self,
        conn: &Connection,
        tenant: TenantId,
        branch: BranchId,
        device: DeviceId,
    ) -> Result<(), StoreError> {
        let row: Option<(String, String, String)> = conn
            .query_row(
                "SELECT tenant_id,branch_id,status FROM devices WHERE id=?1",
                params![device.to_string()],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()?;
        match row {
            Some((t, b, s))
                if t == tenant.to_string() && b == branch.to_string() && s == "ACTIVE" =>
            {
                Ok(())
            }
            Some(_) => Err(StoreError::Authorization(
                "device is not active in requested tenant/branch",
            )),
            None => Err(StoreError::Authorization("unknown device")),
        }
    }
    fn assert_active_device_tx(
        tx: &Transaction<'_>,
        tenant: TenantId,
        branch: BranchId,
        device: DeviceId,
    ) -> Result<(), StoreError> {
        let row: Option<(String, String, String)> = tx
            .query_row(
                "SELECT tenant_id,branch_id,status FROM devices WHERE id=?1",
                params![device.to_string()],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()?;
        match row {
            Some((t, b, s))
                if t == tenant.to_string() && b == branch.to_string() && s == "ACTIVE" =>
            {
                Ok(())
            }
            _ => Err(StoreError::Authorization(
                "device is not active in requested tenant/branch",
            )),
        }
    }
    fn assert_user_scope(
        tx: &Transaction<'_>,
        tenant: TenantId,
        user: UserId,
    ) -> Result<(), StoreError> {
        let ok: Option<i32> = tx
            .query_row(
                "SELECT 1 FROM users WHERE id=?1 AND tenant_id=?2 AND status='ACTIVE'",
                params![user.to_string(), tenant.to_string()],
                |r| r.get(0),
            )
            .optional()?;
        if ok.is_some() {
            Ok(())
        } else {
            Err(StoreError::Authorization("user not active in tenant"))
        }
    }
    fn assert_permission(
        tx: &Transaction<'_>,
        tenant: TenantId,
        branch: BranchId,
        user: UserId,
        permission: &str,
    ) -> Result<(), StoreError> {
        let ok:Option<i32>=tx.query_row("SELECT 1 FROM user_roles ur JOIN roles ro ON ro.id=ur.role_id JOIN role_permissions rp ON rp.role_id=ro.id WHERE ur.user_id=?1 AND ro.tenant_id=?2 AND rp.permission_code=?3 AND (ur.branch_id IS NULL OR ur.branch_id=?4) LIMIT 1",params![user.to_string(),tenant.to_string(),permission,branch.to_string()],|r|r.get(0)).optional()?;
        if ok.is_some() {
            Ok(())
        } else {
            Err(StoreError::Authorization("permission denied"))
        }
    }
    fn assert_permission_conn(
        conn: &Connection,
        tenant: TenantId,
        branch: BranchId,
        user: UserId,
        permission: &str,
    ) -> Result<(), StoreError> {
        let ok:Option<i32>=conn.query_row("SELECT 1 FROM user_roles ur JOIN roles ro ON ro.id=ur.role_id JOIN role_permissions rp ON rp.role_id=ro.id WHERE ur.user_id=?1 AND ro.tenant_id=?2 AND rp.permission_code=?3 AND (ur.branch_id IS NULL OR ur.branch_id=?4) LIMIT 1",params![user.to_string(),tenant.to_string(),permission,branch.to_string()],|r|r.get(0)).optional()?;
        if ok.is_some() {
            Ok(())
        } else {
            Err(StoreError::Authorization("permission denied"))
        }
    }
    fn validate_local_device_context(
        &self,
        context: LocalTerminalContext,
    ) -> Result<(), StoreError> {
        let bound:Option<i32>=self.conn.query_row("SELECT 1 FROM local_terminal_binding WHERE singleton=1 AND tenant_id=?1 AND branch_id=?2 AND device_id=?3 AND register_id=?4",params![context.tenant_id.to_string(),context.branch_id.to_string(),context.device_id.to_string(),context.register_id.to_string()],|row|row.get(0)).optional()?;
        if bound.is_none() {
            return Err(StoreError::Authorization("terminal binding mismatch"));
        }
        self.assert_active_device(
            &self.conn,
            context.tenant_id,
            context.branch_id,
            context.device_id,
        )
    }
    fn assert_inventory_centre(
        tx: &Transaction<'_>,
        tenant: TenantId,
        branch: BranchId,
        centre: Uuid,
    ) -> Result<(), StoreError> {
        let ok: Option<i32> = tx
            .query_row(
                "SELECT 1 FROM inventory_centres WHERE id=?1 AND tenant_id=?2 AND branch_id=?3",
                params![centre.to_string(), tenant.to_string(), branch.to_string()],
                |row| row.get(0),
            )
            .optional()?;
        if ok.is_some() {
            Ok(())
        } else {
            Err(StoreError::Authorization("inventory centre scope mismatch"))
        }
    }
    fn assert_product(
        tx: &Transaction<'_>,
        tenant: TenantId,
        product: ProductId,
    ) -> Result<(), StoreError> {
        let ok: Option<i32> = tx
            .query_row(
                "SELECT 1 FROM products WHERE id=?1 AND tenant_id=?2",
                params![product.to_string(), tenant.to_string()],
                |row| row.get(0),
            )
            .optional()?;
        if ok.is_some() {
            Ok(())
        } else {
            Err(StoreError::Authorization("product tenant mismatch"))
        }
    }
    #[allow(
        clippy::too_many_arguments,
        reason = "ledger append receives complete immutable movement evidence"
    )]
    fn append_inventory_effect(
        tx: &Transaction<'_>,
        context: LocalTerminalContext,
        user: UserId,
        operation_id: OperationId,
        centre: Uuid,
        product: ProductId,
        quantity: QuantityMilli,
        unit_cost: Money,
        movement_type: &str,
        source_type: &str,
        source_id: &str,
        lot_id: Option<Uuid>,
        now: DateTime<Utc>,
    ) -> Result<Uuid, StoreError> {
        if quantity.0 == 0 {
            return Err(StoreError::Validation(
                "inventory movement quantity cannot be zero".into(),
            ));
        }
        Self::assert_inventory_centre(tx, context.tenant_id, context.branch_id, centre)?;
        Self::assert_product(tx, context.tenant_id, product)?;
        let movement_id = Uuid::new_v4();
        tx.execute("INSERT INTO inventory_movements(id,tenant_id,branch_id,centre_id,product_id,operation_id,movement_type,quantity_milli,unit_cost_fils,source_type,source_id,device_id,user_id,created_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14)",params![movement_id.to_string(),context.tenant_id.to_string(),context.branch_id.to_string(),centre.to_string(),product.to_string(),operation_id.to_string(),movement_type,quantity.0,unit_cost.0,source_type,source_id,context.device_id.to_string(),user.to_string(),now.to_rfc3339()])?;
        tx.execute("INSERT INTO stock_levels(tenant_id,branch_id,centre_id,product_id,quantity_milli,version) VALUES(?1,?2,?3,?4,?5,1) ON CONFLICT(tenant_id,branch_id,centre_id,product_id) DO UPDATE SET quantity_milli=stock_levels.quantity_milli+excluded.quantity_milli,version=stock_levels.version+1",params![context.tenant_id.to_string(),context.branch_id.to_string(),centre.to_string(),product.to_string(),quantity.0])?;
        Self::apply_cost_projection(
            tx,
            context.tenant_id,
            context.branch_id,
            &centre.to_string(),
            &product.to_string(),
            quantity,
            unit_cost,
        )?;
        if let Some(lot) = lot_id {
            let valid: Option<i32> = tx
                .query_row(
                    "SELECT 1 FROM inventory_lots WHERE id=?1 AND tenant_id=?2 AND product_id=?3",
                    params![
                        lot.to_string(),
                        context.tenant_id.to_string(),
                        product.to_string()
                    ],
                    |row| row.get(0),
                )
                .optional()?;
            if valid.is_none() {
                return Err(StoreError::Authorization("inventory lot scope mismatch"));
            }
            if quantity.0 < 0 {
                let available:i64=tx.query_row("SELECT COALESCE(quantity_milli,0) FROM lot_balances WHERE tenant_id=?1 AND branch_id=?2 AND centre_id=?3 AND lot_id=?4",params![context.tenant_id.to_string(),context.branch_id.to_string(),centre.to_string(),lot.to_string()],|row|row.get(0)).optional()?.unwrap_or(0);
                if available
                    .checked_add(quantity.0)
                    .ok_or(bhaipos_core::MoneyError::Overflow)?
                    < 0
                {
                    return Err(StoreError::Conflict("insufficient lot quantity".into()));
                }
            }
            tx.execute("INSERT INTO inventory_movement_lots(movement_id,lot_id,quantity_milli) VALUES(?1,?2,?3)",params![movement_id.to_string(),lot.to_string(),quantity.0])?;
            tx.execute("INSERT INTO lot_balances(tenant_id,branch_id,centre_id,lot_id,quantity_milli) VALUES(?1,?2,?3,?4,?5) ON CONFLICT(tenant_id,branch_id,centre_id,lot_id) DO UPDATE SET quantity_milli=lot_balances.quantity_milli+excluded.quantity_milli",params![context.tenant_id.to_string(),context.branch_id.to_string(),centre.to_string(),lot.to_string(),quantity.0])?;
        }
        Ok(movement_id)
    }
    fn apply_cost_projection(
        tx: &Transaction<'_>,
        tenant: TenantId,
        branch: BranchId,
        centre: &str,
        product: &str,
        quantity: QuantityMilli,
        unit_cost: Money,
    ) -> Result<(), StoreError> {
        let absolute = QuantityMilli(
            quantity
                .0
                .checked_abs()
                .ok_or(bhaipos_core::MoneyError::Overflow)?,
        );
        let value = price_times_quantity(unit_cost, absolute)?;
        let signed_value = if quantity.0 < 0 {
            Money::ZERO.checked_sub(value)?
        } else {
            value
        };
        tx.execute("INSERT INTO inventory_cost_balances(tenant_id,branch_id,centre_id,product_id,quantity_milli,total_value_fils,version) VALUES(?1,?2,?3,?4,?5,?6,1) ON CONFLICT(tenant_id,branch_id,centre_id,product_id) DO UPDATE SET quantity_milli=inventory_cost_balances.quantity_milli+excluded.quantity_milli,total_value_fils=inventory_cost_balances.total_value_fils+excluded.total_value_fils,version=inventory_cost_balances.version+1",params![tenant.to_string(),branch.to_string(),centre,product,quantity.0,signed_value.0])?;
        tx.execute("UPDATE inventory_cost_balances SET total_value_fils=0 WHERE tenant_id=?1 AND branch_id=?2 AND centre_id=?3 AND product_id=?4 AND quantity_milli=0",params![tenant.to_string(),branch.to_string(),centre,product])?;
        Ok(())
    }
    fn inventory_valuation_conn(
        conn: &Connection,
        tenant: TenantId,
        branch: BranchId,
        centre: Uuid,
        product: ProductId,
    ) -> Result<InventoryValuation, StoreError> {
        let (quantity,value):(i64,i64)=conn.query_row("SELECT quantity_milli,total_value_fils FROM inventory_cost_balances WHERE tenant_id=?1 AND branch_id=?2 AND centre_id=?3 AND product_id=?4",params![tenant.to_string(),branch.to_string(),centre.to_string(),product.to_string()],|row|Ok((row.get(0)?,row.get(1)?))).optional()?.unwrap_or((0,0));
        let average = if quantity > 0 {
            let numerator = (value as i128)
                .checked_mul(1000)
                .ok_or(bhaipos_core::MoneyError::Overflow)?;
            let rounded = (numerator + (quantity as i128 / 2)) / (quantity as i128);
            Money(i64::try_from(rounded).map_err(|_| bhaipos_core::MoneyError::Overflow)?)
        } else {
            Money::ZERO
        };
        Ok(InventoryValuation {
            quantity: QuantityMilli(quantity),
            total_value: Money(value),
            weighted_average_cost: average,
        })
    }
    fn inventory_valuation_tx(
        tx: &Transaction<'_>,
        tenant: TenantId,
        branch: BranchId,
        centre: Uuid,
        product: ProductId,
    ) -> Result<InventoryValuation, StoreError> {
        let (quantity,value):(i64,i64)=tx.query_row("SELECT quantity_milli,total_value_fils FROM inventory_cost_balances WHERE tenant_id=?1 AND branch_id=?2 AND centre_id=?3 AND product_id=?4",params![tenant.to_string(),branch.to_string(),centre.to_string(),product.to_string()],|row|Ok((row.get(0)?,row.get(1)?))).optional()?.unwrap_or((0,0));
        let average = if quantity > 0 {
            let numerator = (value as i128)
                .checked_mul(1000)
                .ok_or(bhaipos_core::MoneyError::Overflow)?;
            let rounded = (numerator + (quantity as i128 / 2)) / (quantity as i128);
            Money(i64::try_from(rounded).map_err(|_| bhaipos_core::MoneyError::Overflow)?)
        } else {
            Money::ZERO
        };
        Ok(InventoryValuation {
            quantity: QuantityMilli(quantity),
            total_value: Money(value),
            weighted_average_cost: average,
        })
    }
    fn load_inventory_operation<T: DeserializeOwned>(
        &self,
        tenant: TenantId,
        operation: OperationId,
        action: &str,
        request_sha256: &str,
    ) -> Result<Option<T>, StoreError> {
        let global:Option<(String,String)>=self.conn.query_row("SELECT action,request_sha256 FROM idempotency_operations WHERE tenant_id=?1 AND operation_id=?2",params![tenant.to_string(),operation.to_string()],|row|Ok((row.get(0)?,row.get(1)?))).optional()?;
        let expected_action = format!("INVENTORY:{action}");
        if let Some((stored_action, stored_hash)) = &global {
            if stored_action != &expected_action || stored_hash != request_sha256 {
                return Err(StoreError::Conflict(
                    "operation ID already bound to a different action or payload".into(),
                ));
            }
        }
        let row:Option<(String,String)>=self.conn.query_row("SELECT request_sha256,result_json FROM inventory_operation_results WHERE tenant_id=?1 AND operation_id=?2 AND action=?3",params![tenant.to_string(),operation.to_string(),action],|row|Ok((row.get(0)?,row.get(1)?))).optional()?;
        match row {
            Some((stored, json)) if stored == request_sha256 => {
                Ok(Some(serde_json::from_str(&json)?))
            }
            Some(_) => Err(StoreError::Conflict(
                "inventory operation ID reused with different payload".into(),
            )),
            None if global.is_some() => Err(StoreError::Conflict(
                "operation binding exists without inventory result; recovery review required"
                    .into(),
            )),
            None => Ok(None),
        }
    }
    fn record_inventory_operation<T: Serialize>(
        tx: &Transaction<'_>,
        tenant: TenantId,
        operation: OperationId,
        action: &str,
        request_sha256: &str,
        result: &T,
        now: DateTime<Utc>,
    ) -> Result<(), StoreError> {
        tx.execute("INSERT INTO idempotency_operations(tenant_id,operation_id,action,request_sha256,created_at) VALUES(?1,?2,?3,?4,?5)",params![tenant.to_string(),operation.to_string(),format!("INVENTORY:{action}"),request_sha256,now.to_rfc3339()])?;
        tx.execute("INSERT INTO inventory_operation_results(tenant_id,operation_id,action,request_sha256,result_json,committed_at) VALUES(?1,?2,?3,?4,?5,?6)",params![tenant.to_string(),operation.to_string(),action,request_sha256,serde_json::to_string(result)?,now.to_rfc3339()])?;
        Ok(())
    }
    fn enqueue_inventory_sync<T: Serialize>(
        tx: &Transaction<'_>,
        context: LocalTerminalContext,
        operation: OperationId,
        entity_type: &str,
        entity_id: &str,
        result: &T,
        now: DateTime<Utc>,
    ) -> Result<(), StoreError> {
        tx.execute("INSERT INTO sync_queue(id,tenant_id,branch_id,device_id,operation_id,entity_type,entity_id,mutation_type,payload_json,state,created_at,updated_at) VALUES(?1,?2,?3,?4,?5,?6,?7,'INSERT',?8,'PENDING',?9,?9)",params![Uuid::new_v4().to_string(),context.tenant_id.to_string(),context.branch_id.to_string(),context.device_id.to_string(),operation.to_string(),entity_type,entity_id,serde_json::to_string(result)?,now.to_rfc3339()])?;
        Ok(())
    }
    fn tax_category_from_snapshot(value: &str) -> TaxCategory {
        match value {
            "ZERO" => TaxCategory::ZeroRated,
            "EXEMPT" => TaxCategory::Exempt,
            "OUT_OF_SCOPE" => TaxCategory::OutOfScope,
            _ => TaxCategory::StandardRated,
        }
    }
    fn business_date_for(
        tx: &Transaction<'_>,
        tenant: TenantId,
        branch: BranchId,
        now: DateTime<Utc>,
    ) -> Result<(String, String, u8), StoreError> {
        let (code,close):(String,u8)=tx.query_row("SELECT b.code,t.business_close_hour FROM branches b JOIN tenants t ON t.id=b.tenant_id WHERE b.id=?1 AND b.tenant_id=?2",params![branch.to_string(),tenant.to_string()],|r|Ok((r.get(0)?,r.get(1)?)))?;
        let tz = FixedOffset::east_opt(3 * 3600).unwrap();
        let local = now.with_timezone(&tz);
        let date = if local.hour() < (close as u32) {
            (local - chrono::Duration::days(1)).date_naive()
        } else {
            local.date_naive()
        };
        Ok((
            format!("{:04}-{:02}-{:02}", date.year(), date.month(), date.day()),
            code,
            close,
        ))
    }
    fn next_receipt_sequence(
        tx: &Transaction<'_>,
        tenant: TenantId,
        branch: BranchId,
        date: &str,
    ) -> Result<i64, StoreError> {
        tx.execute("INSERT INTO receipt_sequences(tenant_id,branch_id,business_date,last_sequence) VALUES(?1,?2,?3,1) ON CONFLICT(tenant_id,branch_id,business_date) DO UPDATE SET last_sequence=last_sequence+1",params![tenant.to_string(),branch.to_string(),date])?;
        Ok(tx.query_row("SELECT last_sequence FROM receipt_sequences WHERE tenant_id=?1 AND branch_id=?2 AND business_date=?3",params![tenant.to_string(),branch.to_string(),date],|r|r.get(0))?)
    }
    fn render_receipt(
        tx: &Transaction<'_>,
        sale: SaleId,
        receipt_no: &str,
        subtotal: Money,
        tax: Money,
        total: Money,
        change: Money,
    ) -> Result<String, StoreError> {
        let mut out = format!("BHAIPOS\nReceipt: {receipt_no}\n------------------------------\n");
        let mut st=tx.prepare("SELECT product_name_snapshot,quantity_milli,unit_price_fils,gross_fils FROM sale_lines WHERE sale_id=?1 ORDER BY id")?;
        let rows = st.query_map(params![sale.to_string()], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, i64>(2)?,
                r.get::<_, i64>(3)?,
            ))
        })?;
        for rr in rows {
            let (n, q, p, g) = rr?;
            let q_abs = (q as i128).abs();
            let q_text = format!(
                "{}{}.{:03}",
                if q < 0 { "-" } else { "" },
                q_abs / 1000,
                q_abs % 1000
            );
            out.push_str(&format!(
                "{}  {} x {} = {}\n",
                n,
                q_text,
                Money(p),
                Money(g)
            ));
        }
        out.push_str("------------------------------\n");
        out.push_str(&format!(
            "Net: {} BHD\nVAT: {} BHD\nTOTAL: {} BHD\n",
            subtotal, tax, total
        ));
        if change.0 > 0 {
            out.push_str(&format!("Change: {} BHD\n", change));
        }
        Ok(out)
    }
    #[allow(
        clippy::too_many_arguments,
        reason = "audit append receives complete tamper-evident event material"
    )]
    fn append_audit(
        tx: &Transaction<'_>,
        tenant: TenantId,
        device: DeviceId,
        user: UserId,
        event_type: &str,
        entity_type: &str,
        entity_id: &str,
        payload: &str,
        now: DateTime<Utc>,
    ) -> Result<(), StoreError> {
        let tenant_id = tenant.to_string();
        let device_id = device.to_string();
        let event_count: i64 = tx.query_row(
            "SELECT COUNT(*) FROM audit_events WHERE tenant_id=?1 AND device_id=?2",
            params![&tenant_id, &device_id],
            |row| row.get(0),
        )?;
        if event_count > 0 && !Self::audit_chain_valid_on(tx, tenant, device)? {
            return Err(StoreError::Conflict(
                "audit chain topology is inconsistent".into(),
            ));
        }
        let mut statement=tx.prepare("SELECT event.event_hash FROM audit_events event WHERE event.tenant_id=?1 AND event.device_id=?2 AND NOT EXISTS (SELECT 1 FROM audit_events successor WHERE successor.tenant_id=event.tenant_id AND successor.device_id=event.device_id AND successor.previous_hash=event.event_hash)")?;
        let tips = statement
            .query_map(params![&tenant_id, &device_id], |row| {
                row.get::<_, String>(0)
            })?
            .collect::<Result<Vec<_>, _>>()?;
        let previous = match (event_count, tips.as_slice()) {
            (0, []) => String::new(),
            (_, [tip]) => tip.clone(),
            _ => {
                return Err(StoreError::Conflict(
                    "audit chain topology is inconsistent".into(),
                ))
            }
        };
        let user_id = user.to_string();
        let material = AuditMaterial {
            tenant_id: &tenant_id,
            device_id: &device_id,
            actor_user_id: &user_id,
            event_type,
            entity_type,
            entity_id,
            payload_json: payload,
            created_at: now,
            previous_hash: &previous,
        };
        let hash = compute_audit_hash(&material);
        tx.execute("INSERT INTO audit_events(id,tenant_id,device_id,actor_user_id,event_type,entity_type,entity_id,payload_json,previous_hash,event_hash,created_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",params![Uuid::new_v4().to_string(),tenant_id,device_id,user_id,event_type,entity_type,entity_id,payload,previous,hash,now.to_rfc3339()])?;
        Ok(())
    }
}

#[derive(Debug)]
struct CartLine {
    product_id: String,
    name: String,
    sku: String,
    barcode: Option<String>,
    qty: i64,
    price: i64,
    cost: i64,
    tax_category: String,
    tax_bps: i32,
    tax_inclusive: bool,
}
#[derive(Debug)]
struct PreparedRefundLine {
    sale_line_id: Uuid,
    product_id: String,
    quantity: QuantityMilli,
    unit_cost: i64,
    net: Money,
    tax: Money,
    gross: Money,
}
