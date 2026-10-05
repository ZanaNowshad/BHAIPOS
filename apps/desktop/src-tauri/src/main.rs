#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod credential_store;
mod printer;

use bhaipos_core::{
    render_esc_pos, CartId, CutMode, DrawerPulsePolicy, EscPosProfile, Money, OperationId,
    QuantityMilli, ReceiptEncoding, SaleId, TenderKind,
};
use bhaipos_store::{
    BackgroundJobEnqueueRequest, BackgroundJobFinishOutcome, BackgroundJobResult,
    BackupCreateRequest as StoreBackupCreateRequest,
    BackupScheduleRequest as StoreBackupScheduleRequest, BackupScheduleResult, CartSnapshot,
    CashMovementKind, CashMovementRequest as StoreCashMovementRequest, CashMovementResult,
    CashSessionReport, CheckoutRequest as StoreCheckoutRequest, CheckoutResult,
    CloseCashSessionRequest as StoreCloseCashSessionRequest, CloseCashSessionResult,
    FailedPrintJob, HeldCartSummary, LocalBootstrapRequest as StoreBootstrapRequest,
    LocalBootstrapResult, LocalTerminalContext, OperationalAlertResult, OperationalAlertSummary,
    PaymentInput, RefundLineInput, RefundQuote, RefundRequest as StoreRefundRequest, RefundResult,
    RefundableSale, RestoreBackupRequest as StoreRestoreBackupRequest, RestorePreview,
    RestoreResult, Store,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Mutex;
use tauri::{Manager, State};
use uuid::Uuid;

#[derive(Clone, Copy)]
struct AuthenticatedSession {
    terminal: LocalTerminalContext,
    user_id: bhaipos_core::UserId,
    cash_session_id: Option<Uuid>,
}

struct AppState {
    store: Mutex<Store>,
    terminal: Mutex<Option<LocalTerminalContext>>,
    session: Mutex<Option<AuthenticatedSession>>,
    last_backup_schedule_tick: Mutex<Option<DateTime<Utc>>>,
    backup_directory: PathBuf,
}

#[derive(Serialize)]
struct Health {
    ready: bool,
    database: String,
    schema: &'static str,
    initialized: bool,
    authenticated: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct BootstrapRequest {
    business_name: String,
    branch_code: String,
    branch_name: String,
    register_code: String,
    register_name: String,
    device_label: String,
    owner_employee_no: String,
    owner_name: String,
    owner_pin: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct LoginRequest {
    employee_no: String,
    pin: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct LoginResponse {
    display_name: String,
    cash_session_id: Option<String>,
    can_view_alerts: bool,
    can_manage_alerts: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct OpenCashSessionRequest {
    cash_session_id: String,
    opening_float_fils: i64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct IdResponse {
    id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ScanBarcodeRequest {
    cart_id: String,
    barcode: String,
    quantity_milli: i64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CartRequest {
    cart_id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct HoldCartRequest {
    cart_id: String,
    note: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PaymentRequest {
    kind: TenderKind,
    amount_fils: i64,
    tendered_fils: Option<i64>,
    reference: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CheckoutRequest {
    cart_id: String,
    operation_id: String,
    payments: Vec<PaymentRequest>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RequeuePrintJobRequest {
    print_job_id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct FindRefundableSaleRequest {
    receipt_number: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RefundQuoteRequest {
    sale_id: String,
    lines: Vec<RefundLineRequest>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RefundLineRequest {
    sale_line_id: String,
    quantity_milli: i64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RefundRequest {
    operation_id: String,
    sale_id: String,
    reason: String,
    lines: Vec<RefundLineRequest>,
    payments: Vec<PaymentRequest>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CashMovementRequest {
    operation_id: String,
    kind: CashMovementKind,
    amount_fils: i64,
    reason: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CloseCashSessionRequest {
    operation_id: String,
    counted_cash_fils: i64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct TransitionOperationalAlertRequest {
    operation_id: String,
    alert_id: String,
    new_status: String,
    note: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateBackupRequest {
    operation_id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ConfigureBackupScheduleRequest {
    operation_id: String,
    interval_minutes: i64,
    retention_count: i64,
    enabled: bool,
    first_run_at: String,
    authorization_valid_days: i64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PreviewRestoreRequest {
    backup_id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RestoreBackupRequest {
    operation_id: String,
    backup_id: String,
    expected_sha256: String,
}

fn command_error(error: impl std::fmt::Display) -> String {
    error.to_string()
}

fn parse_uuid(value: &str, field: &str) -> Result<Uuid, String> {
    Uuid::parse_str(value).map_err(|_| format!("invalid {field}"))
}

fn require_session(state: &State<'_, AppState>) -> Result<AuthenticatedSession, String> {
    let session = state
        .session
        .lock()
        .map_err(|_| "session state poisoned".to_string())?
        .as_ref()
        .copied()
        .ok_or_else(|| "authentication required".to_string())?;
    state
        .store
        .lock()
        .map_err(|_| "database state poisoned".to_string())?
        .validate_local_session(session.terminal, session.user_id)
        .map_err(command_error)?;
    Ok(session)
}

fn payments(input: Vec<PaymentRequest>) -> Vec<PaymentInput> {
    input
        .into_iter()
        .map(|payment| PaymentInput {
            kind: payment.kind,
            amount: Money(payment.amount_fils),
            tendered: payment.tendered_fils.map(Money),
            reference: payment.reference,
        })
        .collect()
}

#[tauri::command]
fn health(state: State<'_, AppState>) -> Result<Health, String> {
    let store = state
        .store
        .lock()
        .map_err(|_| "database state poisoned".to_string())?;
    let check: String = store
        .connection()
        .query_row("PRAGMA quick_check", [], |row| row.get(0))
        .map_err(command_error)?;
    let initialized = state
        .terminal
        .lock()
        .map_err(|_| "terminal state poisoned".to_string())?
        .is_some();
    let authenticated = state
        .session
        .lock()
        .map_err(|_| "session state poisoned".to_string())?
        .is_some();
    Ok(Health {
        ready: check == "ok",
        database: check,
        schema: bhaipos_store::LATEST_SCHEMA,
        initialized,
        authenticated,
    })
}

#[tauri::command]
fn bootstrap_local_business(
    state: State<'_, AppState>,
    request: BootstrapRequest,
) -> Result<LocalBootstrapResult, String> {
    let terminal = LocalTerminalContext {
        tenant_id: bhaipos_core::TenantId::new(),
        branch_id: bhaipos_core::BranchId::new(),
        device_id: bhaipos_core::DeviceId::new(),
        register_id: bhaipos_core::RegisterId::new(),
    };
    let device_credential_secret = format!("{}{}", Uuid::new_v4(), Uuid::new_v4());
    credential_store::write_device_secret(terminal.device_id.0, &device_credential_secret)?;
    let mut store = state
        .store
        .lock()
        .map_err(|_| "database state poisoned".to_string())?;
    let result = store.bootstrap_local_business(StoreBootstrapRequest {
        business_name: request.business_name,
        branch_code: request.branch_code,
        branch_name: request.branch_name,
        register_code: request.register_code,
        register_name: request.register_name,
        device_label: request.device_label,
        owner_employee_no: request.owner_employee_no,
        owner_name: request.owner_name,
        owner_pin: request.owner_pin,
        terminal,
        device_credential_secret,
        now: Utc::now(),
    });
    let result = match result {
        Ok(result) => result,
        Err(error) => {
            let _ = credential_store::delete_device_secret(terminal.device_id.0);
            return Err(command_error(error));
        }
    };
    *state
        .terminal
        .lock()
        .map_err(|_| "terminal state poisoned".to_string())? = Some(result.terminal);
    Ok(result)
}

#[tauri::command]
fn login(state: State<'_, AppState>, request: LoginRequest) -> Result<LoginResponse, String> {
    let terminal = state
        .terminal
        .lock()
        .map_err(|_| "terminal state poisoned".to_string())?
        .as_ref()
        .copied()
        .ok_or_else(|| "local business is not initialized".to_string())?;
    let store = state
        .store
        .lock()
        .map_err(|_| "database state poisoned".to_string())?;
    let user_id = store
        .authenticate_employee_pin(
            terminal.tenant_id,
            &request.employee_no,
            &request.pin,
            Utc::now(),
            5,
            15,
        )
        .map_err(command_error)?;
    let display_name: String = store
        .connection()
        .query_row(
            "SELECT display_name FROM users WHERE id=?1 AND tenant_id=?2",
            [user_id.to_string(), terminal.tenant_id.to_string()],
            |row| row.get(0),
        )
        .map_err(command_error)?;
    let cash_session_id = store
        .active_cash_session_for(terminal, user_id)
        .map_err(command_error)?;
    let can_view_alerts = store
        .user_has_permission(terminal, user_id, "alert.view")
        .map_err(command_error)?;
    let can_manage_alerts = store
        .user_has_permission(terminal, user_id, "alert.manage")
        .map_err(command_error)?;
    *state
        .session
        .lock()
        .map_err(|_| "session state poisoned".to_string())? = Some(AuthenticatedSession {
        terminal,
        user_id,
        cash_session_id,
    });
    Ok(LoginResponse {
        display_name,
        cash_session_id: cash_session_id.map(|id| id.to_string()),
        can_view_alerts,
        can_manage_alerts,
    })
}

#[tauri::command]
fn logout(state: State<'_, AppState>) -> Result<(), String> {
    *state
        .session
        .lock()
        .map_err(|_| "session state poisoned".to_string())? = None;
    Ok(())
}

#[tauri::command]
fn rotate_local_device_credential(state: State<'_, AppState>) -> Result<i64, String> {
    let session = require_session(&state)?;
    let old_secret = credential_store::read_device_secret(session.terminal.device_id.0)?;
    let new_secret = format!("{}{}", Uuid::new_v4(), Uuid::new_v4());
    credential_store::write_device_secret(session.terminal.device_id.0, &new_secret)?;
    let result = state
        .store
        .lock()
        .map_err(|_| "database state poisoned".to_string())?
        .rotate_local_device_credential(session.terminal, session.user_id, &new_secret, Utc::now());
    match result {
        Ok(version) => Ok(version),
        Err(error) => {
            credential_store::write_device_secret(session.terminal.device_id.0,&old_secret).map_err(|restore|format!("credential rotation failed ({error}); OS credential rollback also failed ({restore})"))?;
            Err(command_error(error))
        }
    }
}

#[tauri::command]
fn open_cash_session(
    state: State<'_, AppState>,
    request: OpenCashSessionRequest,
) -> Result<IdResponse, String> {
    let mut session = require_session(&state)?;
    let id = parse_uuid(&request.cash_session_id, "cash session ID")?;
    state
        .store
        .lock()
        .map_err(|_| "database state poisoned".to_string())?
        .open_cash_session(
            id,
            session.terminal.tenant_id,
            session.terminal.branch_id,
            session.terminal.register_id,
            session.user_id,
            session.terminal.device_id,
            Money(request.opening_float_fils),
            Utc::now(),
        )
        .map_err(command_error)?;
    session.cash_session_id = Some(id);
    *state
        .session
        .lock()
        .map_err(|_| "session state poisoned".to_string())? = Some(session);
    Ok(IdResponse { id: id.to_string() })
}

#[tauri::command]
fn create_cart(state: State<'_, AppState>) -> Result<IdResponse, String> {
    let session = require_session(&state)?;
    let id = CartId::new();
    state
        .store
        .lock()
        .map_err(|_| "database state poisoned".to_string())?
        .create_cart(
            id,
            session.terminal.tenant_id,
            session.terminal.branch_id,
            session.terminal.device_id,
            session.user_id,
            Utc::now(),
        )
        .map_err(command_error)?;
    Ok(IdResponse { id: id.to_string() })
}

#[tauri::command]
fn cart_snapshot(state: State<'_, AppState>, request: CartRequest) -> Result<CartSnapshot, String> {
    let session = require_session(&state)?;
    state
        .store
        .lock()
        .map_err(|_| "database state poisoned".to_string())?
        .cart_snapshot(
            session.terminal,
            session.user_id,
            CartId(parse_uuid(&request.cart_id, "cart ID")?),
        )
        .map_err(command_error)
}

#[tauri::command]
fn scan_barcode(
    state: State<'_, AppState>,
    request: ScanBarcodeRequest,
) -> Result<CartSnapshot, String> {
    let session = require_session(&state)?;
    let cart = CartId(parse_uuid(&request.cart_id, "cart ID")?);
    let store = state
        .store
        .lock()
        .map_err(|_| "database state poisoned".to_string())?;
    store
        .add_barcode_to_cart(
            session.terminal.tenant_id,
            session.terminal.branch_id,
            cart,
            &request.barcode,
            QuantityMilli(request.quantity_milli),
            Utc::now(),
        )
        .map_err(command_error)?;
    store
        .cart_snapshot(session.terminal, session.user_id, cart)
        .map_err(command_error)
}

#[tauri::command]
fn hold_cart(state: State<'_, AppState>, request: HoldCartRequest) -> Result<(), String> {
    let session = require_session(&state)?;
    state
        .store
        .lock()
        .map_err(|_| "database state poisoned".to_string())?
        .hold_cart(
            session.terminal,
            session.user_id,
            CartId(parse_uuid(&request.cart_id, "cart ID")?),
            request.note.as_deref(),
            Utc::now(),
        )
        .map_err(command_error)
}

#[tauri::command]
fn list_held_carts(state: State<'_, AppState>) -> Result<Vec<HeldCartSummary>, String> {
    let session = require_session(&state)?;
    state
        .store
        .lock()
        .map_err(|_| "database state poisoned".to_string())?
        .held_carts(session.terminal, session.user_id)
        .map_err(command_error)
}

#[tauri::command]
fn restore_cart(state: State<'_, AppState>, request: CartRequest) -> Result<CartSnapshot, String> {
    let session = require_session(&state)?;
    let cart = CartId(parse_uuid(&request.cart_id, "cart ID")?);
    let store = state
        .store
        .lock()
        .map_err(|_| "database state poisoned".to_string())?;
    store
        .restore_cart(session.terminal, session.user_id, cart, Utc::now())
        .map_err(command_error)?;
    store
        .cart_snapshot(session.terminal, session.user_id, cart)
        .map_err(command_error)
}

#[tauri::command]
fn checkout(
    state: State<'_, AppState>,
    request: CheckoutRequest,
) -> Result<CheckoutResult, String> {
    let session = require_session(&state)?;
    state
        .store
        .lock()
        .map_err(|_| "database state poisoned".to_string())?
        .checkout(StoreCheckoutRequest {
            tenant_id: session.terminal.tenant_id,
            branch_id: session.terminal.branch_id,
            device_id: session.terminal.device_id,
            register_id: session.terminal.register_id,
            user_id: session.user_id,
            cart_id: CartId(parse_uuid(&request.cart_id, "cart ID")?),
            operation_id: OperationId(parse_uuid(&request.operation_id, "operation ID")?),
            cash_session_id: session.cash_session_id,
            payments: payments(request.payments),
            now: Utc::now(),
        })
        .map_err(command_error)
}

#[tauri::command]
fn requeue_failed_print_job(
    state: State<'_, AppState>,
    request: RequeuePrintJobRequest,
) -> Result<(), String> {
    let session = require_session(&state)?;
    state
        .store
        .lock()
        .map_err(|_| "database state poisoned".to_string())?
        .requeue_failed_print_job_authorized(
            session.terminal,
            session.user_id,
            parse_uuid(&request.print_job_id, "print job ID")?,
            Utc::now(),
        )
        .map_err(command_error)
}

#[tauri::command]
fn list_failed_print_jobs(state: State<'_, AppState>) -> Result<Vec<FailedPrintJob>, String> {
    let session = require_session(&state)?;
    state
        .store
        .lock()
        .map_err(|_| "database state poisoned".to_string())?
        .failed_print_jobs(session.terminal, session.user_id)
        .map_err(command_error)
}

#[tauri::command]
fn find_refundable_sale(
    state: State<'_, AppState>,
    request: FindRefundableSaleRequest,
) -> Result<RefundableSale, String> {
    let session = require_session(&state)?;
    state
        .store
        .lock()
        .map_err(|_| "database state poisoned".to_string())?
        .find_refundable_sale(session.terminal, session.user_id, &request.receipt_number)
        .map_err(command_error)
}

#[tauri::command]
fn quote_refund(
    state: State<'_, AppState>,
    request: RefundQuoteRequest,
) -> Result<RefundQuote, String> {
    let session = require_session(&state)?;
    let lines = request
        .lines
        .into_iter()
        .map(|line| {
            Ok(RefundLineInput {
                sale_line_id: parse_uuid(&line.sale_line_id, "sale line ID")?,
                quantity: QuantityMilli(line.quantity_milli),
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    state
        .store
        .lock()
        .map_err(|_| "database state poisoned".to_string())?
        .quote_refund(
            session.terminal,
            session.user_id,
            SaleId(parse_uuid(&request.sale_id, "sale ID")?),
            &lines,
        )
        .map_err(command_error)
}

#[tauri::command]
fn refund(state: State<'_, AppState>, request: RefundRequest) -> Result<RefundResult, String> {
    let session = require_session(&state)?;
    let lines = request
        .lines
        .into_iter()
        .map(|line| {
            Ok(RefundLineInput {
                sale_line_id: parse_uuid(&line.sale_line_id, "sale line ID")?,
                quantity: QuantityMilli(line.quantity_milli),
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    state
        .store
        .lock()
        .map_err(|_| "database state poisoned".to_string())?
        .refund(StoreRefundRequest {
            tenant_id: session.terminal.tenant_id,
            branch_id: session.terminal.branch_id,
            device_id: session.terminal.device_id,
            user_id: session.user_id,
            operation_id: OperationId(parse_uuid(&request.operation_id, "operation ID")?),
            sale_id: SaleId(parse_uuid(&request.sale_id, "sale ID")?),
            reason: request.reason,
            lines,
            cash_session_id: session.cash_session_id,
            payments: payments(request.payments),
            now: Utc::now(),
        })
        .map_err(command_error)
}

#[tauri::command]
fn record_cash_movement(
    state: State<'_, AppState>,
    request: CashMovementRequest,
) -> Result<CashMovementResult, String> {
    let session = require_session(&state)?;
    let cash_session_id = session
        .cash_session_id
        .ok_or_else(|| "open cash session required".to_string())?;
    state
        .store
        .lock()
        .map_err(|_| "database state poisoned".to_string())?
        .record_cash_movement(StoreCashMovementRequest {
            tenant_id: session.terminal.tenant_id,
            branch_id: session.terminal.branch_id,
            device_id: session.terminal.device_id,
            user_id: session.user_id,
            cash_session_id,
            operation_id: OperationId(parse_uuid(&request.operation_id, "operation ID")?),
            kind: request.kind,
            amount: Money(request.amount_fils),
            reason: request.reason,
            now: Utc::now(),
        })
        .map_err(command_error)
}

#[tauri::command]
fn cash_session_report(state: State<'_, AppState>) -> Result<CashSessionReport, String> {
    let session = require_session(&state)?;
    let cash_session_id = session
        .cash_session_id
        .ok_or_else(|| "open cash session required".to_string())?;
    state
        .store
        .lock()
        .map_err(|_| "database state poisoned".to_string())?
        .cash_session_report(
            session.terminal.tenant_id,
            session.terminal.branch_id,
            cash_session_id,
        )
        .map_err(command_error)
}

#[tauri::command]
fn close_cash_session(
    state: State<'_, AppState>,
    request: CloseCashSessionRequest,
) -> Result<CloseCashSessionResult, String> {
    let mut session = require_session(&state)?;
    let cash_session_id = session
        .cash_session_id
        .ok_or_else(|| "open cash session required".to_string())?;
    let result = state
        .store
        .lock()
        .map_err(|_| "database state poisoned".to_string())?
        .close_cash_session(StoreCloseCashSessionRequest {
            tenant_id: session.terminal.tenant_id,
            branch_id: session.terminal.branch_id,
            device_id: session.terminal.device_id,
            user_id: session.user_id,
            cash_session_id,
            operation_id: OperationId(parse_uuid(&request.operation_id, "operation ID")?),
            counted_cash: Money(request.counted_cash_fils),
            now: Utc::now(),
        })
        .map_err(command_error)?;
    session.cash_session_id = None;
    *state
        .session
        .lock()
        .map_err(|_| "session state poisoned".to_string())? = Some(session);
    Ok(result)
}

#[tauri::command]
fn list_operational_alerts(
    state: State<'_, AppState>,
) -> Result<Vec<OperationalAlertSummary>, String> {
    let session = require_session(&state)?;
    state
        .store
        .lock()
        .map_err(|_| "database state poisoned".to_string())?
        .active_operational_alerts(session.terminal, session.user_id, 200)
        .map_err(command_error)
}

#[tauri::command]
fn transition_operational_alert(
    state: State<'_, AppState>,
    request: TransitionOperationalAlertRequest,
) -> Result<OperationalAlertResult, String> {
    let session = require_session(&state)?;
    state
        .store
        .lock()
        .map_err(|_| "database state poisoned".to_string())?
        .transition_operational_alert(
            session.terminal,
            session.user_id,
            OperationId(parse_uuid(&request.operation_id, "operation ID")?),
            parse_uuid(&request.alert_id, "alert ID")?,
            &request.new_status,
            Some(session.user_id),
            request.note.as_deref(),
            Utc::now(),
        )
        .map_err(command_error)
}

#[tauri::command]
fn create_verified_backup(
    state: State<'_, AppState>,
    request: CreateBackupRequest,
) -> Result<BackgroundJobResult, String> {
    let session = require_session(&state)?;
    state
        .store
        .lock()
        .map_err(|_| "database state poisoned".to_string())?
        .enqueue_background_job(BackgroundJobEnqueueRequest {
            context: session.terminal,
            user_id: session.user_id,
            operation_id: OperationId(parse_uuid(&request.operation_id, "operation id")?),
            job_type: "BACKUP_CREATE".into(),
            payload_json: serde_json::json!({
                "backup_type": "MANUAL",
                "app_version": env!("CARGO_PKG_VERSION"),
            })
            .to_string(),
            progress_total: Some(1),
            cancellable: true,
            max_attempts: 3,
            not_before: None,
            now: Utc::now(),
        })
        .map_err(command_error)
}

#[tauri::command]
fn configure_backup_schedule(
    state: State<'_, AppState>,
    request: ConfigureBackupScheduleRequest,
) -> Result<BackupScheduleResult, String> {
    let session = require_session(&state)?;
    let first_run_at = DateTime::parse_from_rfc3339(&request.first_run_at)
        .map_err(|_| "invalid first run time".to_string())?
        .with_timezone(&Utc);
    state
        .store
        .lock()
        .map_err(|_| "database state poisoned".to_string())?
        .configure_backup_schedule(StoreBackupScheduleRequest {
            context: session.terminal,
            user_id: session.user_id,
            operation_id: OperationId(parse_uuid(&request.operation_id, "operation id")?),
            interval_minutes: request.interval_minutes,
            retention_count: request.retention_count,
            enabled: request.enabled,
            first_run_at,
            authorization_valid_days: request.authorization_valid_days,
            now: Utc::now(),
        })
        .map_err(command_error)
}

#[tauri::command]
fn preview_verified_restore(
    state: State<'_, AppState>,
    request: PreviewRestoreRequest,
) -> Result<RestorePreview, String> {
    let session = require_session(&state)?;
    state
        .store
        .lock()
        .map_err(|_| "database state poisoned".to_string())?
        .preview_restore(
            session.terminal,
            session.user_id,
            parse_uuid(&request.backup_id, "backup id")?,
        )
        .map_err(command_error)
}

#[tauri::command]
fn restore_verified_backup(
    state: State<'_, AppState>,
    request: RestoreBackupRequest,
) -> Result<RestoreResult, String> {
    let session = require_session(&state)?;
    state
        .store
        .lock()
        .map_err(|_| "database state poisoned".to_string())?
        .restore_verified_backup(StoreRestoreBackupRequest {
            context: session.terminal,
            user_id: session.user_id,
            operation_id: OperationId(parse_uuid(&request.operation_id, "operation id")?),
            backup_id: parse_uuid(&request.backup_id, "backup id")?,
            expected_sha256: request.expected_sha256,
            safety_backup_directory: state.backup_directory.clone(),
            app_version: env!("CARGO_PKG_VERSION").into(),
            now: Utc::now(),
        })
        .map_err(command_error)
}

fn print_worker_cycle(handle: &tauri::AppHandle) -> Result<(), String> {
    let state = handle.state::<AppState>();
    let terminal = state
        .terminal
        .lock()
        .map_err(|_| "terminal state poisoned".to_string())?
        .as_ref()
        .copied();
    let Some(terminal) = terminal else {
        return Ok(());
    };
    let now = Utc::now();
    let (profile, lease) = {
        let mut store = state
            .store
            .lock()
            .map_err(|_| "database state poisoned".to_string())?;
        store
            .recover_stale_print_jobs(
                terminal.tenant_id,
                terminal.branch_id,
                terminal.device_id,
                now - chrono::Duration::minutes(5),
                now,
            )
            .map_err(command_error)?;
        let profile = store
            .default_printer_profile(terminal)
            .map_err(command_error)?;
        let lease = store
            .claim_next_print_job(
                terminal.tenant_id,
                terminal.branch_id,
                terminal.device_id,
                now,
            )
            .map_err(command_error)?;
        (profile, lease)
    };
    let Some(lease) = lease else { return Ok(()) };
    let outcome = (|| {
        let receipt = lease
            .receipt_text
            .as_deref()
            .ok_or_else(|| "print job has no historical receipt representation".to_string())?;
        let encoding = match profile.character_encoding.as_str() {
            "ASCII" => ReceiptEncoding::Ascii,
            "UTF8" => ReceiptEncoding::Utf8,
            _ => return Err("unsupported receipt character encoding".into()),
        };
        let cut_mode = match profile.cut_mode.as_str() {
            "NONE" => CutMode::None,
            "PARTIAL" => CutMode::Partial,
            "FULL" => CutMode::Full,
            _ => return Err("unsupported receipt cut mode".into()),
        };
        let drawer_pulse_policy = match profile.drawer_pulse_policy.as_str() {
            "NEVER" => DrawerPulsePolicy::Never,
            "CASH_SALE" => DrawerPulsePolicy::CashSale,
            _ => return Err("unsupported drawer pulse policy".into()),
        };
        let escpos = render_esc_pos(
            receipt,
            EscPosProfile {
                paper_width_mm: u8::try_from(profile.paper_width_mm)
                    .map_err(|_| "invalid paper width")?,
                characters_per_line: u8::try_from(profile.characters_per_line)
                    .map_err(|_| "invalid line width")?,
                encoding,
                cut_mode,
                drawer_pulse_policy,
            },
            lease.cash_drawer_pulse,
        )
        .map_err(|error| error.to_string())?;
        let target = lease
            .printer_target
            .as_deref()
            .or(profile.target.as_deref());
        printer::print_bytes(
            &profile.transport,
            target,
            &format!("BHAIPOS {}", lease.document_type),
            &escpos,
        )
    })();
    let mut store = state
        .store
        .lock()
        .map_err(|_| "database state poisoned".to_string())?;
    store
        .finish_print_job(
            terminal.tenant_id,
            terminal.branch_id,
            terminal.device_id,
            lease.print_job_id,
            lease.lease_token,
            outcome.is_ok(),
            outcome.as_ref().err().map(String::as_str),
            Utc::now(),
        )
        .map_err(command_error)
}

#[derive(Deserialize)]
struct BackupJobPayload {
    backup_type: String,
    app_version: String,
    schedule_id: Option<Uuid>,
}

fn background_worker_cycle(handle: &tauri::AppHandle) -> Result<(), String> {
    let state = handle.state::<AppState>();
    let terminal = state
        .terminal
        .lock()
        .map_err(|_| "terminal state poisoned".to_string())?
        .as_ref()
        .copied();
    let Some(terminal) = terminal else {
        return Ok(());
    };
    let session = state
        .session
        .lock()
        .map_err(|_| "session state poisoned".to_string())?
        .as_ref()
        .copied();
    let now = Utc::now();
    let should_tick_schedule = {
        let mut last_tick = state
            .last_backup_schedule_tick
            .lock()
            .map_err(|_| "backup schedule state poisoned".to_string())?;
        let due = last_tick.map_or(true, |previous| {
            now.signed_duration_since(previous).num_seconds() >= 60
        });
        if due {
            *last_tick = Some(now);
        }
        due
    };
    let lease = {
        let mut store = state
            .store
            .lock()
            .map_err(|_| "database state poisoned".to_string())?;
        if should_tick_schedule {
            store
                .enqueue_due_backup_jobs(
                    terminal,
                    OperationId::new(),
                    env!("CARGO_PKG_VERSION"),
                    now,
                )
                .map_err(command_error)?;
        }
        if let Some(session) = session {
            if should_tick_schedule {
                store
                    .recover_expired_background_jobs(
                        session.terminal,
                        session.user_id,
                        OperationId::new(),
                        now,
                    )
                    .map_err(command_error)?;
            }
            store
                .claim_next_background_job(
                    session.terminal,
                    session.user_id,
                    OperationId::new(),
                    1_800,
                    now,
                )
                .map_err(command_error)?
        } else {
            store
                .claim_next_scheduled_backup_job(terminal, OperationId::new(), 1_800, now)
                .map_err(command_error)?
        }
    };
    let Some(lease) = lease else { return Ok(()) };
    let authority_user = lease.user_id;
    let outcome = if lease.job_type == "BACKUP_CREATE" {
        match serde_json::from_str::<BackupJobPayload>(&lease.payload_json) {
            Err(error) => BackgroundJobFinishOutcome::RequiresReview {
                error: format!("invalid backup job payload: {error}"),
            },
            Ok(payload) => {
                let scheduled = payload.backup_type.eq_ignore_ascii_case("SCHEDULED");
                if scheduled && payload.schedule_id.is_none() {
                    BackgroundJobFinishOutcome::RequiresReview {
                        error: "scheduled backup job is missing schedule identity".into(),
                    }
                } else {
                    let cancel_requested = state
                        .store
                        .lock()
                        .map_err(|_| "database state poisoned".to_string())?
                        .heartbeat_background_job(
                            terminal,
                            authority_user,
                            OperationId::new(),
                            lease.job_id,
                            lease.lease_token,
                            0,
                            Some(1),
                            1_800,
                            Utc::now(),
                        )
                        .map_err(command_error)?
                        .cancel_requested;
                    if cancel_requested {
                        BackgroundJobFinishOutcome::Cancelled { result_json: None }
                    } else {
                        let completed_at = Utc::now();
                        let mut store = state
                            .store
                            .lock()
                            .map_err(|_| "database state poisoned".to_string())?;
                        let result = store.create_verified_backup(StoreBackupCreateRequest {
                            context: terminal,
                            user_id: authority_user,
                            operation_id: OperationId(lease.job_id),
                            backup_type: payload.backup_type,
                            destination_directory: state.backup_directory.clone(),
                            app_version: payload.app_version,
                            now: completed_at,
                        });
                        match result {
                            Ok(result) if scheduled => {
                                let schedule_id = payload.schedule_id.ok_or_else(|| {
                                    "scheduled backup job is missing schedule identity".to_string()
                                })?;
                                let retention = store
                                    .record_scheduled_backup_output(
                                        terminal,
                                        authority_user,
                                        schedule_id,
                                        lease.job_id,
                                        result.backup_id,
                                        completed_at,
                                    )
                                    .and_then(|()| {
                                        store.prune_scheduled_backups(
                                            terminal,
                                            authority_user,
                                            OperationId(lease.job_id),
                                            schedule_id,
                                            &state.backup_directory,
                                            completed_at,
                                        )
                                    });
                                match retention {
                                    Ok(retention) if retention.failed == 0 => {
                                        BackgroundJobFinishOutcome::Succeeded {
                                            result_json: serde_json::json!({
                                                "backup":result,
                                                "retention":retention,
                                            })
                                            .to_string(),
                                        }
                                    }
                                    Ok(retention) => BackgroundJobFinishOutcome::RequiresReview {
                                        error: format!(
                                            "backup completed but {} retention deletion(s) require review",
                                            retention.failed
                                        ),
                                    },
                                    Err(error) => BackgroundJobFinishOutcome::RequiresReview {
                                        error: format!(
                                            "backup completed but retention processing failed: {error}"
                                        ),
                                    },
                                }
                            }
                            Ok(result) => BackgroundJobFinishOutcome::Succeeded {
                                result_json: serde_json::to_string(&result)
                                    .map_err(command_error)?,
                            },
                            Err(error) => BackgroundJobFinishOutcome::RequiresReview {
                                error: error.to_string(),
                            },
                        }
                    }
                }
            }
        }
    } else {
        BackgroundJobFinishOutcome::RequiresReview {
            error: format!("no deterministic handler for job type {}", lease.job_type),
        }
    };
    state
        .store
        .lock()
        .map_err(|_| "database state poisoned".to_string())?
        .finish_background_job(
            terminal,
            authority_user,
            OperationId::new(),
            lease.job_id,
            lease.lease_token,
            outcome,
            Utc::now(),
        )
        .map_err(command_error)?;
    Ok(())
}

fn main() {
    tauri::Builder::default()
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&data_dir)?;
            let db_path = data_dir.join("bhaipos.sqlite3");
            let store = Store::open(db_path.to_string_lossy().as_ref())
                .map_err(|error| std::io::Error::other(error.to_string()))?;
            let terminal = store
                .local_terminal_context()
                .map_err(|error| std::io::Error::other(error.to_string()))?;
            app.manage(AppState {
                store: Mutex::new(store),
                terminal: Mutex::new(terminal),
                session: Mutex::new(None),
                last_backup_schedule_tick: Mutex::new(None),
                backup_directory: data_dir.join("backups"),
            });
            let handle = app.handle().clone();
            std::thread::spawn(move || loop {
                if let Err(error) = print_worker_cycle(&handle) {
                    eprintln!("BHAIPOS print worker: {error}");
                }
                std::thread::sleep(std::time::Duration::from_secs(2));
            });
            let background_handle = app.handle().clone();
            std::thread::spawn(move || loop {
                if let Err(error) = background_worker_cycle(&background_handle) {
                    eprintln!("BHAIPOS background worker: {error}");
                }
                std::thread::sleep(std::time::Duration::from_secs(2));
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            health,
            bootstrap_local_business,
            login,
            logout,
            rotate_local_device_credential,
            open_cash_session,
            create_cart,
            cart_snapshot,
            scan_barcode,
            hold_cart,
            list_held_carts,
            restore_cart,
            checkout,
            list_failed_print_jobs,
            requeue_failed_print_job,
            find_refundable_sale,
            quote_refund,
            refund,
            record_cash_movement,
            cash_session_report,
            close_cash_session,
            list_operational_alerts,
            transition_operational_alert,
            create_verified_backup,
            configure_backup_schedule,
            preview_verified_restore,
            restore_verified_backup,
        ])
        .run(tauri::generate_context!())
        .expect("BHAIPOS desktop runtime failed");
}
