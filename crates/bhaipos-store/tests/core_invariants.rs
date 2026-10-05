use bhaipos_core::{
    compute_audit_hash, hash_pin, sha256_hex, sign_approval, ApprovalBinding, AuditMaterial,
    BranchId, CartId, DeviceId, Money, OperationId, ProductId, QuantityMilli, RegisterId, TenantId,
    TenderKind, UserId,
};
use bhaipos_store::{
    BackgroundJobEnqueueRequest, BackgroundJobFinishOutcome, CashMovementKind, CashMovementRequest,
    CheckoutRequest, CloseCashSessionRequest, LocalBootstrapRequest, NewProduct, PaymentInput,
    RefundLineInput, RefundRequest, Store, StoreError, SyncDeliveryOutcome,
};
use chrono::{DateTime, Utc};
use rusqlite::params;
use uuid::Uuid;

fn t(s: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc)
}

struct Fixture {
    store: Store,
    tenant: TenantId,
    branch: BranchId,
    user: UserId,
    device: DeviceId,
    register: RegisterId,
    centre: Uuid,
    product: ProductId,
    cash_session: Uuid,
}

fn fixture() -> Fixture {
    let mut store = Store::in_memory().unwrap();
    let tenant = TenantId::new();
    let branch = BranchId::new();
    let user = UserId::new();
    let device = DeviceId::new();
    let register = RegisterId::new();
    let centre = Uuid::new_v4();
    let product = ProductId::new();
    let cash_session = Uuid::new_v4();
    let now = t("2026-09-29T00:30:00Z");
    store.create_tenant(tenant, "BHAI Retail", 3, now).unwrap();
    store
        .create_branch(branch, tenant, "MAIN", "Main Branch", now)
        .unwrap();
    store
        .create_user(user, tenant, "Cashier", &hash_pin("1234").unwrap(), now)
        .unwrap();
    let role = Uuid::new_v4();
    store.create_role(role, tenant, "cashier").unwrap();
    for (code, desc) in [
        ("sale.checkout", "Complete a sale"),
        ("sale.hold", "Hold a cart"),
        ("sale.refund", "Refund a sale"),
        ("sale.void", "Void a sale"),
        ("receipt.reprint", "Reprint receipts"),
        ("sync.resolve", "Resolve sync conflicts"),
        ("device.rotate", "Rotate device credential"),
        ("device.enroll", "Enroll terminal"),
        ("inventory.receive", "Receive inventory"),
        ("inventory.transfer", "Transfer inventory"),
        ("inventory.stocktake", "Stocktake"),
        ("inventory.waste", "Record waste"),
        ("inventory.reconcile", "Reconcile inventory"),
        ("procurement.manage", "Manage procurement"),
        ("procurement.approve", "Approve procurement"),
        ("supplier.invoice.post", "Post supplier invoice"),
        ("supplier.payment.post", "Post supplier payment"),
        ("supplier.statement.view", "View supplier statement"),
        ("supplier.return", "Return to supplier"),
        ("customer.manage", "Manage customers"),
        ("loyalty.adjust", "Adjust loyalty"),
        ("customer.credit.manage", "Manage credit"),
        ("customer.credit.collect", "Collect credit"),
        ("customer.credit.view", "View credit"),
        ("production.manage", "Manage production"),
        ("expense.manage", "Create and submit expenses"),
        ("expense.approve", "Approve or reject expenses"),
        ("expense.pay", "Pay approved expenses"),
        ("expense.report", "View operating profit reports"),
        ("delivery.manage", "Create delivery orders and workers"),
        ("delivery.dispatch", "Progress and dispatch deliveries"),
        ("delivery.collect", "Record delivery collections"),
        ("delivery.settle", "Settle courier cash custody"),
        ("employee.manage", "Manage employee records"),
        ("attendance.clock", "Record attendance events"),
        ("attendance.manage", "Resolve attendance exceptions"),
        ("attendance.view", "View attendance reports"),
        ("alert.create", "Create operational alerts"),
        ("alert.manage", "Assign and transition operational alerts"),
        ("alert.view", "View operational alerts"),
        ("job.enqueue", "Enqueue background jobs"),
        ("job.execute", "Execute background jobs"),
        ("job.manage", "Cancel and recover background jobs"),
        ("job.view", "View background jobs"),
        ("cash.session.open", "Open cash session"),
        ("cash.session.close", "Close cash session"),
        ("cash.movement.paid_in", "Paid in"),
        ("cash.movement.paid_out", "Paid out"),
        ("cash.movement.safe_drop", "Safe drop"),
        ("cash.drawer.no_sale", "Open drawer without sale"),
    ] {
        store.define_permission(code, desc).unwrap();
        store.grant_permission(role, code).unwrap();
    }
    store.assign_role(user, role, Some(branch)).unwrap();
    store
        .create_device(
            device,
            tenant,
            branch,
            "POS-01",
            "device-secret-0123456789",
            now,
        )
        .unwrap();
    store
        .create_register(register, tenant, branch, "R1", "Register 1")
        .unwrap();
    store.connection().execute("INSERT INTO local_terminal_binding(singleton,tenant_id,branch_id,device_id,register_id,installed_at) VALUES(1,?1,?2,?3,?4,?5)",params![tenant.to_string(),branch.to_string(),device.to_string(),register.to_string(),now.to_rfc3339()]).unwrap();
    store
        .create_inventory_centre(centre, tenant, branch, "FLOOR", "Shop Floor")
        .unwrap();
    store
        .create_product(
            product,
            NewProduct {
                tenant_id: tenant,
                sku: "SKU-1",
                name: "Milk 1L",
                barcode: "6290000000015",
                price: Money(1100),
                cost: Money(700),
                tax_rate_bps: 1000,
                tax_inclusive: true,
                track_inventory: true,
                allow_decimal_qty: false,
            },
            now,
        )
        .unwrap();
    store
        .set_branch_assortment(tenant, branch, product, "CORE", true)
        .unwrap();
    let supplier = Uuid::new_v4();
    store.connection().execute("INSERT INTO suppliers(id,tenant_id,name,active,created_at) VALUES(?1,?2,'Opening Stock',1,?3)",params![supplier.to_string(),tenant.to_string(),now.to_rfc3339()]).unwrap();
    let context = bhaipos_store::LocalTerminalContext {
        tenant_id: tenant,
        branch_id: branch,
        device_id: device,
        register_id: register,
    };
    store
        .receive_inventory(bhaipos_store::InventoryReceiptRequest {
            context,
            user_id: user,
            operation_id: OperationId::new(),
            supplier_id: supplier,
            centre_id: centre,
            supplier_document_no: Some("OPENING".into()),
            lines: vec![bhaipos_store::InventoryReceiptLineInput {
                product_id: product,
                received_quantity: QuantityMilli(10_000),
                rejected_quantity: QuantityMilli(0),
                damaged_quantity: QuantityMilli(0),
                unit_cost: Money(700),
                lot_number: None,
                expires_on: None,
            }],
            now,
        })
        .unwrap();
    store
        .connection()
        .execute(
            "UPDATE sync_queue SET state='COMMITTED' WHERE entity_type='inventory_receipt'",
            [],
        )
        .unwrap();
    store
        .open_cash_session(
            cash_session,
            tenant,
            branch,
            register,
            user,
            device,
            Money(20_000),
            now,
        )
        .unwrap();
    Fixture {
        store,
        tenant,
        branch,
        user,
        device,
        register,
        centre,
        product,
        cash_session,
    }
}

fn cart_with_one(f: &Fixture, now: DateTime<Utc>) -> CartId {
    let c = CartId::new();
    f.store
        .create_cart(c, f.tenant, f.branch, f.device, f.user, now)
        .unwrap();
    f.store
        .add_barcode_to_cart(
            f.tenant,
            f.branch,
            c,
            "6290000000015",
            QuantityMilli::ONE,
            now,
        )
        .unwrap();
    c
}

#[test]
fn local_bootstrap_is_atomic_persistent_and_offline_login_capable() {
    let mut store = Store::in_memory().unwrap();
    let now = t("2026-09-29T01:00:00Z");
    let terminal = bhaipos_store::LocalTerminalContext {
        tenant_id: TenantId::new(),
        branch_id: BranchId::new(),
        device_id: DeviceId::new(),
        register_id: RegisterId::new(),
    };
    let request = || LocalBootstrapRequest {
        business_name: "BHAI Retail".into(),
        branch_code: "MAIN".into(),
        branch_name: "Main Branch".into(),
        register_code: "R1".into(),
        register_name: "Register 1".into(),
        device_label: "POS-01".into(),
        owner_employee_no: "OWNER-1".into(),
        owner_name: "Owner".into(),
        owner_pin: "1234".into(),
        terminal,
        device_credential_secret: "bootstrap-device-secret-012345".into(),
        now,
    };
    let created = store.bootstrap_local_business(request()).unwrap();
    assert_eq!(
        store.local_terminal_context().unwrap(),
        Some(created.terminal)
    );
    assert_eq!(
        store
            .authenticate_employee_pin(created.terminal.tenant_id, "OWNER-1", "1234", now, 5, 15)
            .unwrap(),
        created.owner_user_id
    );
    store
        .validate_local_session(created.terminal, created.owner_user_id)
        .unwrap();
    assert_eq!(
        store
            .authenticate_device(
                created.terminal.tenant_id,
                created.terminal.branch_id,
                created.terminal.device_id,
                "bootstrap-device-secret-012345"
            )
            .unwrap(),
        1
    );
    let printer = store.default_printer_profile(created.terminal).unwrap();
    assert_eq!(
        (
            printer.paper_width_mm,
            printer.characters_per_line,
            printer.drawer_pulse_policy.as_str()
        ),
        (80, 48, "CASH_SALE")
    );
    assert!(matches!(
        store.bootstrap_local_business(request()).unwrap_err(),
        StoreError::Conflict(_)
    ));
    let tenant_count: i64 = store
        .connection()
        .query_row("SELECT COUNT(*) FROM tenants", [], |row| row.get(0))
        .unwrap();
    assert_eq!(tenant_count, 1);
    store
        .set_device_status(created.terminal.device_id, "REVOKED", now)
        .unwrap();
    assert!(matches!(
        store.validate_local_session(created.terminal, created.owner_user_id),
        Err(StoreError::Authorization(_))
    ));
}

#[test]
fn cash_session_open_replay_returns_existing_only_for_the_same_payload() {
    let f = fixture();
    let now = t("2026-09-29T01:00:00Z");
    f.store
        .open_cash_session(
            f.cash_session,
            f.tenant,
            f.branch,
            f.register,
            f.user,
            f.device,
            Money(20_000),
            now,
        )
        .unwrap();
    let error = f
        .store
        .open_cash_session(
            f.cash_session,
            f.tenant,
            f.branch,
            f.register,
            f.user,
            f.device,
            Money(21_000),
            now,
        )
        .unwrap_err();
    assert!(matches!(error, StoreError::Conflict(_)));
    let context = bhaipos_store::LocalTerminalContext {
        tenant_id: f.tenant,
        branch_id: f.branch,
        device_id: f.device,
        register_id: f.register,
    };
    assert_eq!(
        f.store.active_cash_session_for(context, f.user).unwrap(),
        Some(f.cash_session)
    );
}

#[test]
fn cart_snapshot_totals_and_hold_restore_are_store_authoritative() {
    let f = fixture();
    let now = t("2026-09-29T01:00:00Z");
    let cart = cart_with_one(&f, now);
    let context = bhaipos_store::LocalTerminalContext {
        tenant_id: f.tenant,
        branch_id: f.branch,
        device_id: f.device,
        register_id: f.register,
    };
    let snapshot = f.store.cart_snapshot(context, f.user, cart).unwrap();
    assert_eq!(snapshot.candidate_subtotal, Money(1000));
    assert_eq!(snapshot.candidate_tax, Money(100));
    assert_eq!(snapshot.candidate_total, Money(1100));
    f.store
        .hold_cart(context, f.user, cart, Some("customer returning"), now)
        .unwrap();
    let held = f.store.held_carts(context, f.user).unwrap();
    assert_eq!(held.len(), 1);
    assert_eq!(held[0].cart_id, cart);
    f.store.restore_cart(context, f.user, cart, now).unwrap();
    assert_eq!(
        f.store.cart_snapshot(context, f.user, cart).unwrap().status,
        "ACTIVE"
    );
}

#[test]
fn atomic_checkout_is_idempotent_and_decrements_stock_once() {
    let mut f = fixture();
    let now = t("2026-09-29T01:00:00Z");
    let cart = cart_with_one(&f, now);
    let op = OperationId::new();
    let req = || CheckoutRequest {
        tenant_id: f.tenant,
        branch_id: f.branch,
        device_id: f.device,
        register_id: f.register,
        user_id: f.user,
        cart_id: cart,
        operation_id: op,
        cash_session_id: Some(f.cash_session),
        payments: vec![PaymentInput {
            kind: TenderKind::Cash,
            amount: Money(1100),
            tendered: Some(Money(2000)),
            reference: None,
        }],
        now,
    };
    let first = f.store.checkout(req()).unwrap();
    let second = f.store.checkout(req()).unwrap();
    assert_eq!(first, second);
    assert_eq!(first.change, Money(900));
    assert_eq!(
        f.store
            .stock_quantity(f.tenant, f.branch, f.product)
            .unwrap(),
        9000
    );
    let sale_count: i64 = f
        .store
        .connection()
        .query_row("SELECT COUNT(*) FROM sales", [], |r| r.get(0))
        .unwrap();
    assert_eq!(sale_count, 1);
    let print_jobs: i64 = f
        .store
        .connection()
        .query_row(
            "SELECT COUNT(*) FROM print_jobs WHERE sale_id=?1 AND state='PENDING'",
            params![first.sale_id.to_string()],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(print_jobs, 1);
    assert!(f.store.audit_chain_valid(f.tenant, f.device).unwrap());
}

#[test]
fn checkout_replay_rejects_a_changed_payload() {
    let mut f = fixture();
    let now = t("2026-09-29T01:00:00Z");
    let cart = cart_with_one(&f, now);
    let op = OperationId::new();
    f.store
        .checkout(CheckoutRequest {
            tenant_id: f.tenant,
            branch_id: f.branch,
            device_id: f.device,
            register_id: f.register,
            user_id: f.user,
            cart_id: cart,
            operation_id: op,
            cash_session_id: Some(f.cash_session),
            payments: vec![PaymentInput {
                kind: TenderKind::Cash,
                amount: Money(1100),
                tendered: Some(Money(1100)),
                reference: None,
            }],
            now,
        })
        .unwrap();

    let err = f
        .store
        .checkout(CheckoutRequest {
            tenant_id: f.tenant,
            branch_id: f.branch,
            device_id: f.device,
            register_id: f.register,
            user_id: f.user,
            cart_id: cart,
            operation_id: op,
            cash_session_id: Some(f.cash_session),
            payments: vec![PaymentInput {
                kind: TenderKind::Cash,
                amount: Money(1100),
                tendered: Some(Money(2000)),
                reference: None,
            }],
            now,
        })
        .unwrap_err();
    assert!(matches!(err, StoreError::Conflict(_)));
    let sale_count: i64 = f
        .store
        .connection()
        .query_row(
            "SELECT COUNT(*) FROM sales WHERE operation_id=?1",
            params![op.to_string()],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(sale_count, 1);
}

#[test]
fn checkout_money_overflow_fails_before_any_sale_is_committed() {
    let mut f = fixture();
    let now = t("2026-09-29T01:00:00Z");
    f.store
        .connection()
        .execute(
            "UPDATE products SET base_price_fils=?2,tax_rate_bps=0 WHERE id=?1",
            params![f.product.to_string(), i64::MAX],
        )
        .unwrap();
    let cart = CartId::new();
    f.store
        .create_cart(cart, f.tenant, f.branch, f.device, f.user, now)
        .unwrap();
    f.store
        .add_barcode_to_cart(
            f.tenant,
            f.branch,
            cart,
            "6290000000015",
            QuantityMilli::ONE,
            now,
        )
        .unwrap();
    f.store
        .add_barcode_to_cart(
            f.tenant,
            f.branch,
            cart,
            "6290000000015",
            QuantityMilli::ONE,
            now,
        )
        .unwrap();
    let error = f
        .store
        .checkout(CheckoutRequest {
            tenant_id: f.tenant,
            branch_id: f.branch,
            device_id: f.device,
            register_id: f.register,
            user_id: f.user,
            cart_id: cart,
            operation_id: OperationId::new(),
            cash_session_id: Some(f.cash_session),
            payments: vec![],
            now,
        })
        .unwrap_err();
    assert!(matches!(
        error,
        StoreError::Money(bhaipos_core::MoneyError::Overflow)
    ));
    let sale_count: i64 = f
        .store
        .connection()
        .query_row("SELECT COUNT(*) FROM sales", [], |row| row.get(0))
        .unwrap();
    assert_eq!(sale_count, 0);
}

#[test]
fn zero_value_item_can_be_sold_and_returned_without_fabricated_payment() {
    let mut f = fixture();
    let now = t("2026-09-29T01:00:00Z");
    f.store
        .connection()
        .execute(
            "UPDATE products SET base_price_fils=0,tax_rate_bps=0 WHERE id=?1",
            params![f.product.to_string()],
        )
        .unwrap();
    let cart = cart_with_one(&f, now);
    let sale = f
        .store
        .checkout(CheckoutRequest {
            tenant_id: f.tenant,
            branch_id: f.branch,
            device_id: f.device,
            register_id: f.register,
            user_id: f.user,
            cart_id: cart,
            operation_id: OperationId::new(),
            cash_session_id: None,
            payments: vec![],
            now,
        })
        .unwrap();
    assert_eq!(sale.total, Money::ZERO);
    let payment_count: i64 = f
        .store
        .connection()
        .query_row(
            "SELECT COUNT(*) FROM sale_payments WHERE sale_id=?1",
            params![sale.sale_id.to_string()],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(payment_count, 0);
    let line: String = f
        .store
        .connection()
        .query_row(
            "SELECT id FROM sale_lines WHERE sale_id=?1",
            params![sale.sale_id.to_string()],
            |row| row.get(0),
        )
        .unwrap();
    let refund = f
        .store
        .refund(RefundRequest {
            tenant_id: f.tenant,
            branch_id: f.branch,
            device_id: f.device,
            user_id: f.user,
            operation_id: OperationId::new(),
            sale_id: sale.sale_id,
            reason: "free item return".into(),
            lines: vec![RefundLineInput {
                sale_line_id: Uuid::parse_str(&line).unwrap(),
                quantity: QuantityMilli::ONE,
            }],
            cash_session_id: None,
            payments: vec![],
            now,
        })
        .unwrap();
    assert_eq!(refund.total, Money::ZERO);
    assert_eq!(
        f.store
            .stock_quantity(f.tenant, f.branch, f.product)
            .unwrap(),
        10_000
    );
}

#[test]
fn operation_id_is_payload_bound_and_cannot_cross_actions() {
    let mut f = fixture();
    let now = t("2026-09-29T01:00:00Z");
    let op = OperationId::new();
    f.store
        .record_cash_movement(CashMovementRequest {
            tenant_id: f.tenant,
            branch_id: f.branch,
            device_id: f.device,
            user_id: f.user,
            cash_session_id: f.cash_session,
            operation_id: op,
            kind: CashMovementKind::PaidIn,
            amount: Money(500),
            reason: Some("float top-up".into()),
            now,
        })
        .unwrap();

    let changed = f
        .store
        .record_cash_movement(CashMovementRequest {
            tenant_id: f.tenant,
            branch_id: f.branch,
            device_id: f.device,
            user_id: f.user,
            cash_session_id: f.cash_session,
            operation_id: op,
            kind: CashMovementKind::PaidIn,
            amount: Money(600),
            reason: Some("float top-up".into()),
            now,
        })
        .unwrap_err();
    assert!(matches!(changed, StoreError::Conflict(_)));

    let cart = cart_with_one(&f, now);
    let cross_action = f
        .store
        .checkout(CheckoutRequest {
            tenant_id: f.tenant,
            branch_id: f.branch,
            device_id: f.device,
            register_id: f.register,
            user_id: f.user,
            cart_id: cart,
            operation_id: op,
            cash_session_id: Some(f.cash_session),
            payments: vec![PaymentInput {
                kind: TenderKind::Cash,
                amount: Money(1100),
                tendered: Some(Money(1100)),
                reference: None,
            }],
            now,
        })
        .unwrap_err();
    assert!(matches!(cross_action, StoreError::Conflict(_)));

    let movement_count: i64 = f
        .store
        .connection()
        .query_row(
            "SELECT COUNT(*) FROM cash_movements WHERE operation_id=?1",
            params![op.to_string()],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(movement_count, 1);
}

#[test]
fn split_tender_must_exactly_equal_server_total() {
    let mut f = fixture();
    let now = t("2026-09-29T01:00:00Z");
    let cart = cart_with_one(&f, now);
    let ok = f
        .store
        .checkout(CheckoutRequest {
            tenant_id: f.tenant,
            branch_id: f.branch,
            device_id: f.device,
            register_id: f.register,
            user_id: f.user,
            cart_id: cart,
            operation_id: OperationId::new(),
            cash_session_id: Some(f.cash_session),
            payments: vec![
                PaymentInput {
                    kind: TenderKind::Cash,
                    amount: Money(600),
                    tendered: Some(Money(600)),
                    reference: None,
                },
                PaymentInput {
                    kind: TenderKind::BenefitPay,
                    amount: Money(500),
                    tendered: None,
                    reference: Some("BP-REF".into()),
                },
            ],
            now,
        })
        .unwrap();
    assert_eq!(ok.total, Money(1100));
    let evidence:String=f.store.connection().query_row("SELECT evidence_status FROM sale_payments WHERE sale_id=?1 AND tender_kind='BENEFITPAY'",params![ok.sale_id.to_string()],|r|r.get(0)).unwrap();
    assert_eq!(evidence, "RECORDED_NOT_SETTLED");
}

#[test]
fn revoked_device_fails_closed() {
    let mut f = fixture();
    let now = t("2026-09-29T01:00:00Z");
    let cart = cart_with_one(&f, now);
    f.store.set_device_status(f.device, "REVOKED", now).unwrap();
    let err = f
        .store
        .checkout(CheckoutRequest {
            tenant_id: f.tenant,
            branch_id: f.branch,
            device_id: f.device,
            register_id: f.register,
            user_id: f.user,
            cart_id: cart,
            operation_id: OperationId::new(),
            cash_session_id: Some(f.cash_session),
            payments: vec![PaymentInput {
                kind: TenderKind::Cash,
                amount: Money(1100),
                tendered: Some(Money(1100)),
                reference: None,
            }],
            now,
        })
        .unwrap_err();
    assert!(matches!(err, StoreError::Authorization(_)));
}

#[test]
fn tenant_scope_cannot_be_crossed() {
    let mut f = fixture();
    let now = t("2026-09-29T01:00:00Z");
    let cart = cart_with_one(&f, now);
    let other = TenantId::new();
    f.store.create_tenant(other, "Other", 3, now).unwrap();
    let err = f
        .store
        .checkout(CheckoutRequest {
            tenant_id: other,
            branch_id: f.branch,
            device_id: f.device,
            register_id: f.register,
            user_id: f.user,
            cart_id: cart,
            operation_id: OperationId::new(),
            cash_session_id: Some(f.cash_session),
            payments: vec![PaymentInput {
                kind: TenderKind::Cash,
                amount: Money(1100),
                tendered: Some(Money(1100)),
                reference: None,
            }],
            now,
        })
        .unwrap_err();
    assert!(matches!(err, StoreError::Authorization(_)));
}

#[test]
fn partial_refund_cannot_exceed_remaining_quantity() {
    let mut f = fixture();
    let now = t("2026-09-29T01:00:00Z");
    let cart = cart_with_one(&f, now);
    let sale = f
        .store
        .checkout(CheckoutRequest {
            tenant_id: f.tenant,
            branch_id: f.branch,
            device_id: f.device,
            register_id: f.register,
            user_id: f.user,
            cart_id: cart,
            operation_id: OperationId::new(),
            cash_session_id: Some(f.cash_session),
            payments: vec![PaymentInput {
                kind: TenderKind::Cash,
                amount: Money(1100),
                tendered: Some(Money(1100)),
                reference: None,
            }],
            now,
        })
        .unwrap();
    let line: String = f
        .store
        .connection()
        .query_row(
            "SELECT id FROM sale_lines WHERE sale_id=?1",
            params![sale.sale_id.to_string()],
            |r| r.get(0),
        )
        .unwrap();
    let line_id = Uuid::parse_str(&line).unwrap();
    let r1 = f
        .store
        .refund(RefundRequest {
            tenant_id: f.tenant,
            branch_id: f.branch,
            device_id: f.device,
            user_id: f.user,
            operation_id: OperationId::new(),
            sale_id: sale.sale_id,
            reason: "customer return".into(),
            lines: vec![RefundLineInput {
                sale_line_id: line_id,
                quantity: QuantityMilli(500),
            }],
            cash_session_id: Some(f.cash_session),
            payments: vec![PaymentInput {
                kind: TenderKind::Cash,
                amount: Money(550),
                tendered: None,
                reference: None,
            }],
            now,
        })
        .unwrap();
    assert_eq!(r1.total, Money(550));
    let err = f
        .store
        .refund(RefundRequest {
            tenant_id: f.tenant,
            branch_id: f.branch,
            device_id: f.device,
            user_id: f.user,
            operation_id: OperationId::new(),
            sale_id: sale.sale_id,
            reason: "too much".into(),
            lines: vec![RefundLineInput {
                sale_line_id: line_id,
                quantity: QuantityMilli(600),
            }],
            cash_session_id: Some(f.cash_session),
            payments: vec![PaymentInput {
                kind: TenderKind::Cash,
                amount: Money(660),
                tendered: None,
                reference: None,
            }],
            now,
        })
        .unwrap_err();
    assert!(matches!(err, StoreError::Validation(_)));
    assert_eq!(
        f.store
            .stock_quantity(f.tenant, f.branch, f.product)
            .unwrap(),
        9500
    );
}

#[test]
fn duplicate_sale_line_in_one_refund_is_rejected_atomically() {
    let mut f = fixture();
    let now = t("2026-09-29T01:00:00Z");
    let cart = cart_with_one(&f, now);
    let sale = f
        .store
        .checkout(CheckoutRequest {
            tenant_id: f.tenant,
            branch_id: f.branch,
            device_id: f.device,
            register_id: f.register,
            user_id: f.user,
            cart_id: cart,
            operation_id: OperationId::new(),
            cash_session_id: Some(f.cash_session),
            payments: vec![PaymentInput {
                kind: TenderKind::Cash,
                amount: Money(1100),
                tendered: Some(Money(1100)),
                reference: None,
            }],
            now,
        })
        .unwrap();
    let line: String = f
        .store
        .connection()
        .query_row(
            "SELECT id FROM sale_lines WHERE sale_id=?1",
            params![sale.sale_id.to_string()],
            |row| row.get(0),
        )
        .unwrap();
    let line_id = Uuid::parse_str(&line).unwrap();
    let error = f
        .store
        .refund(RefundRequest {
            tenant_id: f.tenant,
            branch_id: f.branch,
            device_id: f.device,
            user_id: f.user,
            operation_id: OperationId::new(),
            sale_id: sale.sale_id,
            reason: "duplicate request".into(),
            lines: vec![
                RefundLineInput {
                    sale_line_id: line_id,
                    quantity: QuantityMilli(600),
                },
                RefundLineInput {
                    sale_line_id: line_id,
                    quantity: QuantityMilli(600),
                },
            ],
            cash_session_id: Some(f.cash_session),
            payments: vec![PaymentInput {
                kind: TenderKind::Cash,
                amount: Money(1320),
                tendered: None,
                reference: None,
            }],
            now,
        })
        .unwrap_err();
    assert!(matches!(error, StoreError::Validation(_)));
    let refunds: i64 = f
        .store
        .connection()
        .query_row("SELECT COUNT(*) FROM refunds", [], |row| row.get(0))
        .unwrap();
    assert_eq!(refunds, 0);
    assert_eq!(
        f.store
            .stock_quantity(f.tenant, f.branch, f.product)
            .unwrap(),
        9000
    );
}

#[test]
fn refund_lookup_and_quote_use_historical_values_and_remaining_quantity() {
    let mut f = fixture();
    let now = t("2026-09-29T01:00:00Z");
    let cart = cart_with_one(&f, now);
    let sale = f
        .store
        .checkout(CheckoutRequest {
            tenant_id: f.tenant,
            branch_id: f.branch,
            device_id: f.device,
            register_id: f.register,
            user_id: f.user,
            cart_id: cart,
            operation_id: OperationId::new(),
            cash_session_id: Some(f.cash_session),
            payments: vec![PaymentInput {
                kind: TenderKind::Cash,
                amount: Money(1100),
                tendered: Some(Money(1100)),
                reference: None,
            }],
            now,
        })
        .unwrap();
    f.store
        .connection()
        .execute(
            "UPDATE products SET name='Changed later',base_price_fils=9999 WHERE id=?1",
            params![f.product.to_string()],
        )
        .unwrap();
    let context = bhaipos_store::LocalTerminalContext {
        tenant_id: f.tenant,
        branch_id: f.branch,
        device_id: f.device,
        register_id: f.register,
    };
    let found = f
        .store
        .find_refundable_sale(context, f.user, &sale.receipt_number)
        .unwrap();
    assert_eq!(found.lines.len(), 1);
    assert_eq!(found.lines[0].product_name, "Milk 1L");
    assert_eq!(found.lines[0].refundable_total, Money(1100));
    let quote = f
        .store
        .quote_refund(
            context,
            f.user,
            sale.sale_id,
            &[RefundLineInput {
                sale_line_id: found.lines[0].sale_line_id,
                quantity: QuantityMilli(500),
            }],
        )
        .unwrap();
    assert_eq!(quote.subtotal, Money(500));
    assert_eq!(quote.tax, Money(50));
    assert_eq!(quote.total, Money(550));
}

#[test]
fn failed_print_job_can_be_inspected_and_requeued_by_authorized_cashier() {
    let mut f = fixture();
    let now = t("2026-09-29T01:00:00Z");
    let cart = cart_with_one(&f, now);
    f.store
        .checkout(CheckoutRequest {
            tenant_id: f.tenant,
            branch_id: f.branch,
            device_id: f.device,
            register_id: f.register,
            user_id: f.user,
            cart_id: cart,
            operation_id: OperationId::new(),
            cash_session_id: Some(f.cash_session),
            payments: vec![PaymentInput {
                kind: TenderKind::Cash,
                amount: Money(1100),
                tendered: Some(Money(1100)),
                reference: None,
            }],
            now,
        })
        .unwrap();
    let leased = f
        .store
        .claim_next_print_job(f.tenant, f.branch, f.device, now)
        .unwrap()
        .unwrap();
    f.store
        .finish_print_job(
            f.tenant,
            f.branch,
            f.device,
            leased.print_job_id,
            leased.lease_token,
            false,
            Some("spooler unavailable"),
            now,
        )
        .unwrap();
    let context = bhaipos_store::LocalTerminalContext {
        tenant_id: f.tenant,
        branch_id: f.branch,
        device_id: f.device,
        register_id: f.register,
    };
    let failed = f.store.failed_print_jobs(context, f.user).unwrap();
    assert_eq!(failed.len(), 1);
    assert_eq!(failed[0].last_error, "spooler unavailable");
    assert!(failed[0].receipt_number.is_some());
    f.store
        .requeue_failed_print_job_authorized(context, f.user, leased.print_job_id, now)
        .unwrap();
    let state: String = f
        .store
        .connection()
        .query_row(
            "SELECT state FROM print_jobs WHERE id=?1",
            params![leased.print_job_id.to_string()],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(state, "PENDING");
}

#[test]
fn unknown_barcode_is_recorded_for_resolution() {
    let f = fixture();
    let now = t("2026-09-29T01:00:00Z");
    let cart = CartId::new();
    f.store
        .create_cart(cart, f.tenant, f.branch, f.device, f.user, now)
        .unwrap();
    assert!(f
        .store
        .add_barcode_to_cart(
            f.tenant,
            f.branch,
            cart,
            "9999999999999",
            QuantityMilli::ONE,
            now
        )
        .is_err());
    assert_eq!(
        f.store
            .unknown_barcode_count(f.tenant, f.branch, "9999999999999")
            .unwrap(),
        1
    );
    assert_eq!(
        f.store
            .connection()
            .query_row(
                "SELECT COUNT(*) FROM operational_alerts WHERE tenant_id=?1 AND branch_id=?2 AND alert_type='UNKNOWN_BARCODE' AND entity_type='unknown_barcode' AND entity_id='9999999999999' AND status='NEW'",
                params![f.tenant.to_string(), f.branch.to_string()],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
        1
    );
}

#[test]
fn checkout_that_creates_negative_stock_persists_one_active_alert() {
    let mut f = fixture();
    let now = t("2026-09-29T01:00:00Z");
    let cart = CartId::new();
    f.store
        .create_cart(cart, f.tenant, f.branch, f.device, f.user, now)
        .unwrap();
    f.store
        .add_barcode_to_cart(
            f.tenant,
            f.branch,
            cart,
            "6290000000015",
            QuantityMilli(11_000),
            now,
        )
        .unwrap();
    let operation_id = OperationId::new();
    let request = || CheckoutRequest {
        tenant_id: f.tenant,
        branch_id: f.branch,
        device_id: f.device,
        register_id: f.register,
        user_id: f.user,
        cart_id: cart,
        operation_id,
        cash_session_id: Some(f.cash_session),
        payments: vec![PaymentInput {
            kind: TenderKind::Cash,
            amount: Money(12_100),
            tendered: Some(Money(12_100)),
            reference: None,
        }],
        now,
    };
    let sale = f.store.checkout(request()).unwrap();
    assert_eq!(f.store.checkout(request()).unwrap(), sale);
    assert_eq!(
        f.store
            .stock_quantity(f.tenant, f.branch, f.product)
            .unwrap(),
        -1_000
    );
    assert_eq!(
        f.store
            .connection()
            .query_row(
                "SELECT COUNT(*) FROM operational_alerts WHERE tenant_id=?1 AND branch_id=?2 AND alert_type='NEGATIVE_STOCK' AND entity_type='stock_level' AND status='NEW'",
                params![f.tenant.to_string(), f.branch.to_string()],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
        1
    );
}

#[test]
fn historical_receipt_snapshot_does_not_change_with_catalog() {
    let mut f = fixture();
    let now = t("2026-09-29T01:00:00Z");
    let cart = cart_with_one(&f, now);
    let sale = f
        .store
        .checkout(CheckoutRequest {
            tenant_id: f.tenant,
            branch_id: f.branch,
            device_id: f.device,
            register_id: f.register,
            user_id: f.user,
            cart_id: cart,
            operation_id: OperationId::new(),
            cash_session_id: Some(f.cash_session),
            payments: vec![PaymentInput {
                kind: TenderKind::Cash,
                amount: Money(1100),
                tendered: Some(Money(1100)),
                reference: None,
            }],
            now,
        })
        .unwrap();
    let before = f.store.receipt_snapshot(sale.sale_id).unwrap();
    f.store
        .connection()
        .execute(
            "UPDATE products SET name='Renamed',base_price_fils=9999 WHERE id=?1",
            params![f.product.to_string()],
        )
        .unwrap();
    let after = f.store.receipt_snapshot(sale.sale_id).unwrap();
    assert_eq!(before, after);
}

#[test]
fn late_night_sale_uses_previous_business_date() {
    let mut f = fixture();
    let now = t("2026-09-28T22:30:00Z"); // 01:30 Bahrain on Sep 29, before 03:00 close
    let cart = cart_with_one(&f, now);
    let sale = f
        .store
        .checkout(CheckoutRequest {
            tenant_id: f.tenant,
            branch_id: f.branch,
            device_id: f.device,
            register_id: f.register,
            user_id: f.user,
            cart_id: cart,
            operation_id: OperationId::new(),
            cash_session_id: Some(f.cash_session),
            payments: vec![PaymentInput {
                kind: TenderKind::Cash,
                amount: Money(1100),
                tendered: Some(Money(1100)),
                reference: None,
            }],
            now,
        })
        .unwrap();
    let date: String = f
        .store
        .connection()
        .query_row(
            "SELECT business_date FROM sales WHERE id=?1",
            params![sale.sale_id.to_string()],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(date, "2026-09-28");
}

#[test]
fn audit_chain_detects_tampering() {
    let mut f = fixture();
    let now = t("2026-09-29T01:00:00Z");
    let cart = cart_with_one(&f, now);
    f.store
        .checkout(CheckoutRequest {
            tenant_id: f.tenant,
            branch_id: f.branch,
            device_id: f.device,
            register_id: f.register,
            user_id: f.user,
            cart_id: cart,
            operation_id: OperationId::new(),
            cash_session_id: Some(f.cash_session),
            payments: vec![PaymentInput {
                kind: TenderKind::Cash,
                amount: Money(1100),
                tendered: Some(Money(1100)),
                reference: None,
            }],
            now,
        })
        .unwrap();
    assert!(f.store.audit_chain_valid(f.tenant, f.device).unwrap());
    assert!(f
        .store
        .connection()
        .execute("UPDATE audit_events SET payload_json='tampered'", [])
        .is_err());
    f.store
        .connection()
        .execute("DROP TRIGGER immutable_audit_events_update", [])
        .unwrap();
    f.store
        .connection()
        .execute("UPDATE audit_events SET payload_json='tampered'", [])
        .unwrap();
    assert!(!f.store.audit_chain_valid(f.tenant, f.device).unwrap());
}

#[test]
fn audit_chain_uses_hash_links_instead_of_timestamp_or_uuid_order() {
    let f = fixture();
    let now = t("2026-09-29T01:00:00Z");
    let tenant_id = f.tenant.to_string();
    let device_id = f.device.to_string();
    let user_id = f.user.to_string();
    let previous:String=f.store.connection().query_row("SELECT event_hash FROM audit_events WHERE tenant_id=?1 AND device_id=?2 ORDER BY rowid DESC LIMIT 1",params![tenant_id,device_id],|row|row.get(0)).unwrap();
    let first = AuditMaterial {
        tenant_id: &tenant_id,
        device_id: &device_id,
        actor_user_id: &user_id,
        event_type: "FIRST",
        entity_type: "test",
        entity_id: "first",
        payload_json: "{}",
        created_at: now,
        previous_hash: &previous,
    };
    let first_hash = compute_audit_hash(&first);
    let second = AuditMaterial {
        tenant_id: &tenant_id,
        device_id: &device_id,
        actor_user_id: &user_id,
        event_type: "SECOND",
        entity_type: "test",
        entity_id: "second",
        payload_json: "{}",
        created_at: now,
        previous_hash: &first_hash,
    };
    let second_hash = compute_audit_hash(&second);
    f.store.connection().execute("INSERT INTO audit_events(id,tenant_id,device_id,actor_user_id,event_type,entity_type,entity_id,payload_json,previous_hash,event_hash,created_at) VALUES(?1,?2,?3,?4,'FIRST','test','first','{}',?5,?6,?7)",params!["ffffffff-ffff-ffff-ffff-ffffffffffff",tenant_id,device_id,user_id,previous,first_hash,now.to_rfc3339()]).unwrap();
    f.store.connection().execute("INSERT INTO audit_events(id,tenant_id,device_id,actor_user_id,event_type,entity_type,entity_id,payload_json,previous_hash,event_hash,created_at) VALUES(?1,?2,?3,?4,'SECOND','test','second','{}',?5,?6,?7)",params!["00000000-0000-0000-0000-000000000000",tenant_id,device_id,user_id,first_hash,second_hash,now.to_rfc3339()]).unwrap();

    assert!(f.store.audit_chain_valid(f.tenant, f.device).unwrap());
}

#[test]
fn audit_append_fails_closed_when_the_existing_chain_is_forked() {
    let mut f = fixture();
    let now = t("2026-09-29T01:00:00Z");
    let tenant_id = f.tenant.to_string();
    let device_id = f.device.to_string();
    let user_id = f.user.to_string();
    let root = AuditMaterial {
        tenant_id: &tenant_id,
        device_id: &device_id,
        actor_user_id: &user_id,
        event_type: "ROOT",
        entity_type: "test",
        entity_id: "root",
        payload_json: "{}",
        created_at: now,
        previous_hash: "",
    };
    let root_hash = compute_audit_hash(&root);
    f.store.connection().execute("INSERT INTO audit_events(id,tenant_id,device_id,actor_user_id,event_type,entity_type,entity_id,payload_json,previous_hash,event_hash,created_at) VALUES(?1,?2,?3,?4,'ROOT','test','root','{}','',?5,?6)",params![Uuid::new_v4().to_string(),tenant_id,device_id,user_id,root_hash,now.to_rfc3339()]).unwrap();
    for (event_id, event_type) in [("left", "LEFT"), ("right", "RIGHT")] {
        let event = AuditMaterial {
            tenant_id: &tenant_id,
            device_id: &device_id,
            actor_user_id: &user_id,
            event_type,
            entity_type: "test",
            entity_id: event_id,
            payload_json: "{}",
            created_at: now,
            previous_hash: &root_hash,
        };
        let event_hash = compute_audit_hash(&event);
        f.store.connection().execute("INSERT INTO audit_events(id,tenant_id,device_id,actor_user_id,event_type,entity_type,entity_id,payload_json,previous_hash,event_hash,created_at) VALUES(?1,?2,?3,?4,?5,'test',?6,'{}',?7,?8,?9)",params![Uuid::new_v4().to_string(),tenant_id,device_id,user_id,event_type,event_id,root_hash,event_hash,now.to_rfc3339()]).unwrap();
    }

    assert!(!f.store.audit_chain_valid(f.tenant, f.device).unwrap());
    let error = f
        .store
        .record_cash_movement(CashMovementRequest {
            tenant_id: f.tenant,
            branch_id: f.branch,
            device_id: f.device,
            user_id: f.user,
            cash_session_id: f.cash_session,
            operation_id: OperationId::new(),
            kind: CashMovementKind::PaidIn,
            amount: Money(100),
            reason: Some("test".into()),
            now,
        })
        .unwrap_err();
    assert!(
        matches!(error,StoreError::Conflict(message) if message.contains("audit chain topology"))
    );
    let movement_count: i64 = f
        .store
        .connection()
        .query_row("SELECT COUNT(*) FROM cash_movements", [], |row| row.get(0))
        .unwrap();
    assert_eq!(movement_count, 0);
}

#[test]
fn permissions_fail_closed_when_not_explicitly_granted() {
    let mut f = fixture();
    let now = t("2026-09-29T01:00:00Z");
    let unauthorized = UserId::new();
    f.store
        .create_user(
            unauthorized,
            f.tenant,
            "No Role",
            &hash_pin("9999").unwrap(),
            now,
        )
        .unwrap();
    let cart = CartId::new();
    f.store
        .create_cart(cart, f.tenant, f.branch, f.device, unauthorized, now)
        .unwrap();
    f.store
        .add_barcode_to_cart(
            f.tenant,
            f.branch,
            cart,
            "6290000000015",
            QuantityMilli::ONE,
            now,
        )
        .unwrap();
    let err = f
        .store
        .checkout(CheckoutRequest {
            tenant_id: f.tenant,
            branch_id: f.branch,
            device_id: f.device,
            register_id: f.register,
            user_id: unauthorized,
            cart_id: cart,
            operation_id: OperationId::new(),
            cash_session_id: Some(f.cash_session),
            payments: vec![PaymentInput {
                kind: TenderKind::Cash,
                amount: Money(1100),
                tendered: Some(Money(1100)),
                reference: None,
            }],
            now,
        })
        .unwrap_err();
    assert!(matches!(err, StoreError::Authorization(_)));
}

#[test]
fn pin_lockout_is_enforced_and_expires() {
    let f = fixture();
    let now = t("2026-09-29T01:00:00Z");
    assert!(f
        .store
        .authenticate_pin(f.tenant, f.user, "0000", now, 3, 15)
        .is_err());
    assert!(f
        .store
        .authenticate_pin(f.tenant, f.user, "0000", now, 3, 15)
        .is_err());
    assert!(f
        .store
        .authenticate_pin(f.tenant, f.user, "0000", now, 3, 15)
        .is_err());
    assert!(f
        .store
        .authenticate_pin(
            f.tenant,
            f.user,
            "1234",
            now + chrono::Duration::minutes(1),
            3,
            15
        )
        .is_err());
    assert!(f
        .store
        .authenticate_pin(
            f.tenant,
            f.user,
            "1234",
            now + chrono::Duration::minutes(16),
            3,
            15
        )
        .is_ok());
}

#[test]
fn device_credential_rotation_preserves_identity_and_revocation_is_terminal() {
    let mut f = fixture();
    let now = t("2026-09-29T01:00:00Z");
    let context = bhaipos_store::LocalTerminalContext {
        tenant_id: f.tenant,
        branch_id: f.branch,
        device_id: f.device,
        register_id: f.register,
    };
    assert_eq!(
        f.store
            .rotate_local_device_credential(context, f.user, "device-secret-v2-012345", now)
            .unwrap(),
        2
    );
    f.store.set_device_status(f.device, "REVOKED", now).unwrap();
    assert!(f
        .store
        .rotate_local_device_credential(context, f.user, "device-secret-v3-012345", now)
        .is_err());
    let count: i64 = f
        .store
        .connection()
        .query_row(
            "SELECT COUNT(*) FROM devices WHERE id=?1",
            params![f.device.to_string()],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 1);
}

#[test]
fn terminal_enrollment_grant_is_expiring_single_use_and_returns_new_identity() {
    let mut f = fixture();
    let now = t("2026-09-29T01:00:00Z");
    let context = bhaipos_store::LocalTerminalContext {
        tenant_id: f.tenant,
        branch_id: f.branch,
        device_id: f.device,
        register_id: f.register,
    };
    let grant = f
        .store
        .issue_device_enrollment(context, f.user, "POS-02", 15, now)
        .unwrap();
    let enrolled = f
        .store
        .activate_device_enrollment(
            grant.grant_id,
            &grant.enrollment_token,
            now + chrono::Duration::minutes(1),
        )
        .unwrap();
    assert_ne!(enrolled.device_id, f.device);
    assert_eq!(enrolled.credential_version, 1);
    assert_eq!(
        f.store
            .authenticate_device(
                f.tenant,
                f.branch,
                enrolled.device_id,
                &enrolled.credential_secret
            )
            .unwrap(),
        1
    );
    assert!(matches!(
        f.store.activate_device_enrollment(
            grant.grant_id,
            &grant.enrollment_token,
            now + chrono::Duration::minutes(2)
        ),
        Err(StoreError::Conflict(_))
    ));
    let expired = f
        .store
        .issue_device_enrollment(context, f.user, "POS-03", 5, now)
        .unwrap();
    assert!(matches!(
        f.store.activate_device_enrollment(
            expired.grant_id,
            &expired.enrollment_token,
            now + chrono::Duration::minutes(6)
        ),
        Err(StoreError::Authorization(_))
    ));
}

#[test]
fn manager_approval_is_action_payload_device_bound_and_single_use() {
    let mut f = fixture();
    let now = t("2026-09-29T01:00:00Z");
    let manager = UserId::new();
    f.store
        .create_user(
            manager,
            f.tenant,
            "Manager",
            &hash_pin("7777").unwrap(),
            now,
        )
        .unwrap();
    let role = Uuid::new_v4();
    f.store.create_role(role, f.tenant, "manager").unwrap();
    f.store
        .define_permission("manager.approve", "Approve sensitive actions")
        .unwrap();
    f.store.grant_permission(role, "manager.approve").unwrap();
    f.store.assign_role(manager, role, Some(f.branch)).unwrap();
    let secret = b"test-only-secret";
    let binding = ApprovalBinding {
        tenant_id: f.tenant,
        approver_user_id: manager,
        device_id: f.device,
        operation_id: OperationId::new(),
        action: "price.override".into(),
        entity_id: f.product.to_string(),
        payload_sha256: sha256_hex(br#"{"price_fils":950}"#),
        nonce: Uuid::new_v4().to_string(),
        expires_at: now + chrono::Duration::minutes(2),
    };
    let sig = sign_approval(secret, &binding);
    f.store
        .consume_manager_approval(secret, &binding, &sig, "manager.approve", now)
        .unwrap();
    let replay = f
        .store
        .consume_manager_approval(secret, &binding, &sig, "manager.approve", now)
        .unwrap_err();
    assert!(matches!(replay, StoreError::Conflict(_)));
    let mut altered = binding.clone();
    altered.payload_sha256 = sha256_hex(br#"{"price_fils":500}"#);
    let altered_err = f
        .store
        .consume_manager_approval(secret, &altered, &sig, "manager.approve", now)
        .unwrap_err();
    assert!(matches!(altered_err, StoreError::Authorization(_)));
}

#[test]
fn cash_session_report_accounts_for_refunds_movements_and_close_variance() {
    let mut f = fixture();
    let now = t("2026-09-29T01:00:00Z");
    let cart = cart_with_one(&f, now);
    let sale = f
        .store
        .checkout(CheckoutRequest {
            tenant_id: f.tenant,
            branch_id: f.branch,
            device_id: f.device,
            register_id: f.register,
            user_id: f.user,
            cart_id: cart,
            operation_id: OperationId::new(),
            cash_session_id: Some(f.cash_session),
            payments: vec![PaymentInput {
                kind: TenderKind::Cash,
                amount: Money(1100),
                tendered: Some(Money(1100)),
                reference: None,
            }],
            now,
        })
        .unwrap();
    let line: String = f
        .store
        .connection()
        .query_row(
            "SELECT id FROM sale_lines WHERE sale_id=?1",
            params![sale.sale_id.to_string()],
            |r| r.get(0),
        )
        .unwrap();
    f.store
        .refund(RefundRequest {
            tenant_id: f.tenant,
            branch_id: f.branch,
            device_id: f.device,
            user_id: f.user,
            operation_id: OperationId::new(),
            sale_id: sale.sale_id,
            reason: "half return".into(),
            lines: vec![RefundLineInput {
                sale_line_id: Uuid::parse_str(&line).unwrap(),
                quantity: QuantityMilli(500),
            }],
            cash_session_id: Some(f.cash_session),
            payments: vec![PaymentInput {
                kind: TenderKind::Cash,
                amount: Money(550),
                tendered: None,
                reference: None,
            }],
            now,
        })
        .unwrap();

    for (kind, amount) in [
        (CashMovementKind::PaidIn, Money(2000)),
        (CashMovementKind::PaidOut, Money(500)),
        (CashMovementKind::SafeDrop, Money(10_000)),
        (CashMovementKind::NoSale, Money::ZERO),
    ] {
        f.store
            .record_cash_movement(CashMovementRequest {
                tenant_id: f.tenant,
                branch_id: f.branch,
                device_id: f.device,
                user_id: f.user,
                cash_session_id: f.cash_session,
                operation_id: OperationId::new(),
                kind,
                amount,
                reason: Some("test".into()),
                now,
            })
            .unwrap();
    }
    let report = f
        .store
        .cash_session_report(f.tenant, f.branch, f.cash_session)
        .unwrap();
    assert_eq!(report.opening_float, Money(20_000));
    assert_eq!(report.cash_sales, Money(1100));
    assert_eq!(report.cash_refunds, Money(550));
    assert_eq!(report.paid_in, Money(2000));
    assert_eq!(report.paid_out, Money(500));
    assert_eq!(report.safe_drop, Money(10_000));
    assert_eq!(report.no_sale_count, 1);
    assert_eq!(report.expected_cash, Money(12_050));

    let op = OperationId::new();
    let req = || CloseCashSessionRequest {
        tenant_id: f.tenant,
        branch_id: f.branch,
        device_id: f.device,
        user_id: f.user,
        cash_session_id: f.cash_session,
        operation_id: op,
        counted_cash: Money(12_000),
        now,
    };
    let closed = f.store.close_cash_session(req()).unwrap();
    assert_eq!(closed.report.status, "CLOSED");
    assert_eq!(closed.report.expected_cash, Money(12_050));
    assert_eq!(closed.report.variance, Some(Money(-50)));
    assert!(closed.variance_case_id.is_some());
    let replay = f.store.close_cash_session(req()).unwrap();
    assert_eq!(closed, replay);
    assert_eq!(
        f.store
            .connection()
            .query_row(
                "SELECT COUNT(*) FROM operational_alerts WHERE tenant_id=?1 AND branch_id=?2 AND alert_type='CASH_VARIANCE' AND entity_type='cash_variance_case' AND entity_id=?3 AND status='NEW'",
                params![
                    f.tenant.to_string(),
                    f.branch.to_string(),
                    closed.variance_case_id.unwrap().to_string()
                ],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
        1
    );
}

#[test]
fn cash_movement_replay_is_idempotent_and_no_sale_cannot_move_money() {
    let mut f = fixture();
    let now = t("2026-09-29T01:00:00Z");
    let op = OperationId::new();
    let req = || CashMovementRequest {
        tenant_id: f.tenant,
        branch_id: f.branch,
        device_id: f.device,
        user_id: f.user,
        cash_session_id: f.cash_session,
        operation_id: op,
        kind: CashMovementKind::PaidIn,
        amount: Money(500),
        reason: Some("float top-up".into()),
        now,
    };
    let a = f.store.record_cash_movement(req()).unwrap();
    let b = f.store.record_cash_movement(req()).unwrap();
    assert_eq!(a, b);
    let count: i64 = f
        .store
        .connection()
        .query_row(
            "SELECT COUNT(*) FROM cash_movements WHERE operation_id=?1",
            params![op.to_string()],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 1);
    let err = f
        .store
        .record_cash_movement(CashMovementRequest {
            tenant_id: f.tenant,
            branch_id: f.branch,
            device_id: f.device,
            user_id: f.user,
            cash_session_id: f.cash_session,
            operation_id: OperationId::new(),
            kind: CashMovementKind::NoSale,
            amount: Money(1),
            reason: None,
            now,
        })
        .unwrap_err();
    assert!(matches!(err, StoreError::Validation(_)));
}

#[test]
fn print_failure_retry_never_duplicates_the_sale() {
    let mut f = fixture();
    let now = t("2026-09-29T01:00:00Z");
    let cart = cart_with_one(&f, now);
    let sale = f
        .store
        .checkout(CheckoutRequest {
            tenant_id: f.tenant,
            branch_id: f.branch,
            device_id: f.device,
            register_id: f.register,
            user_id: f.user,
            cart_id: cart,
            operation_id: OperationId::new(),
            cash_session_id: Some(f.cash_session),
            payments: vec![PaymentInput {
                kind: TenderKind::Cash,
                amount: Money(1100),
                tendered: Some(Money(1100)),
                reference: None,
            }],
            now,
        })
        .unwrap();
    let lease = f
        .store
        .claim_next_print_job(f.tenant, f.branch, f.device, now)
        .unwrap()
        .unwrap();
    assert_eq!(lease.sale_id, Some(sale.sale_id));
    assert_eq!(lease.attempt, 1);
    let historical_receipt = f.store.receipt_snapshot(sale.sale_id).unwrap().0;
    assert_eq!(
        lease.receipt_text.as_deref(),
        Some(historical_receipt.as_str())
    );
    f.store
        .finish_print_job(
            f.tenant,
            f.branch,
            f.device,
            lease.print_job_id,
            lease.lease_token,
            false,
            Some("printer offline"),
            now,
        )
        .unwrap();
    f.store
        .requeue_failed_print_job(f.tenant, f.branch, f.device, lease.print_job_id, now)
        .unwrap();
    let retry = f
        .store
        .claim_next_print_job(f.tenant, f.branch, f.device, now)
        .unwrap()
        .unwrap();
    assert_eq!(retry.print_job_id, lease.print_job_id);
    assert_eq!(retry.attempt, 2);
    assert_ne!(retry.lease_token, lease.lease_token);
    f.store
        .finish_print_job(
            f.tenant,
            f.branch,
            f.device,
            retry.print_job_id,
            retry.lease_token,
            true,
            None,
            now,
        )
        .unwrap();
    let state: (String, i64) = f
        .store
        .connection()
        .query_row(
            "SELECT state,attempts FROM print_jobs WHERE id=?1",
            params![retry.print_job_id.to_string()],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(state, ("PRINTED".into(), 2));
    let sales: i64 = f
        .store
        .connection()
        .query_row(
            "SELECT COUNT(*) FROM sales WHERE id=?1",
            params![sale.sale_id.to_string()],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(sales, 1);
}

#[test]
fn interrupted_print_lease_is_recovered_and_stale_worker_cannot_complete_it() {
    let mut f = fixture();
    let now = t("2026-09-29T01:00:00Z");
    let cart = cart_with_one(&f, now);
    f.store
        .checkout(CheckoutRequest {
            tenant_id: f.tenant,
            branch_id: f.branch,
            device_id: f.device,
            register_id: f.register,
            user_id: f.user,
            cart_id: cart,
            operation_id: OperationId::new(),
            cash_session_id: Some(f.cash_session),
            payments: vec![PaymentInput {
                kind: TenderKind::Cash,
                amount: Money(1100),
                tendered: Some(Money(1100)),
                reference: None,
            }],
            now,
        })
        .unwrap();
    let abandoned = f
        .store
        .claim_next_print_job(f.tenant, f.branch, f.device, now)
        .unwrap()
        .unwrap();
    let restarted = now + chrono::Duration::minutes(6);
    assert_eq!(
        f.store
            .recover_stale_print_jobs(
                f.tenant,
                f.branch,
                f.device,
                now + chrono::Duration::minutes(5),
                restarted
            )
            .unwrap(),
        1
    );
    let retry = f
        .store
        .claim_next_print_job(f.tenant, f.branch, f.device, restarted)
        .unwrap()
        .unwrap();
    assert_eq!(retry.print_job_id, abandoned.print_job_id);
    assert_ne!(retry.lease_token, abandoned.lease_token);
    let stale = f
        .store
        .finish_print_job(
            f.tenant,
            f.branch,
            f.device,
            abandoned.print_job_id,
            abandoned.lease_token,
            true,
            None,
            restarted,
        )
        .unwrap_err();
    assert!(matches!(stale, StoreError::Conflict(_)));
    f.store
        .finish_print_job(
            f.tenant,
            f.branch,
            f.device,
            retry.print_job_id,
            retry.lease_token,
            true,
            None,
            restarted,
        )
        .unwrap();
}

#[test]
fn lost_sync_response_replays_same_hub_result_after_worker_restart() {
    let mut f = fixture();
    let now = t("2026-09-29T01:00:00Z");
    let cart = cart_with_one(&f, now);
    f.store
        .checkout(CheckoutRequest {
            tenant_id: f.tenant,
            branch_id: f.branch,
            device_id: f.device,
            register_id: f.register,
            user_id: f.user,
            cart_id: cart,
            operation_id: OperationId::new(),
            cash_session_id: Some(f.cash_session),
            payments: vec![PaymentInput {
                kind: TenderKind::Cash,
                amount: Money(1100),
                tendered: Some(Money(1100)),
                reference: None,
            }],
            now,
        })
        .unwrap();
    let context = bhaipos_store::LocalTerminalContext {
        tenant_id: f.tenant,
        branch_id: f.branch,
        device_id: f.device,
        register_id: f.register,
    };
    let first = f
        .store
        .claim_sync_batch(context, "device-secret-0123456789", 10, now)
        .unwrap()
        .remove(0);
    let accepted = f
        .store
        .accept_hub_mutation(&first, "device-secret-0123456789", now)
        .unwrap();
    assert_eq!(accepted.state, "COMMITTED");
    assert!(!accepted.replayed);
    let restarted = now + chrono::Duration::minutes(6);
    let replay = f
        .store
        .claim_sync_batch(context, "device-secret-0123456789", 10, restarted)
        .unwrap()
        .remove(0);
    assert_eq!(replay.material.mutation_id, first.material.mutation_id);
    assert_ne!(replay.lease_token, first.lease_token);
    let accepted_again = f
        .store
        .accept_hub_mutation(&replay, "device-secret-0123456789", restarted)
        .unwrap();
    assert_eq!(accepted_again.hub_sequence, accepted.hub_sequence);
    assert!(accepted_again.replayed);
    assert!(matches!(
        f.store.complete_sync_delivery(
            context,
            first.material.mutation_id,
            first.lease_token,
            SyncDeliveryOutcome::Hub(accepted.clone()),
            restarted
        ),
        Err(StoreError::Conflict(_))
    ));
    f.store
        .complete_sync_delivery(
            context,
            replay.material.mutation_id,
            replay.lease_token,
            SyncDeliveryOutcome::Hub(accepted_again),
            restarted,
        )
        .unwrap();
    let counts:(i64,String)=f.store.connection().query_row("SELECT (SELECT COUNT(*) FROM hub_mutations),(SELECT state FROM sync_queue WHERE id=?1)",params![replay.material.mutation_id.to_string()],|row|Ok((row.get(0)?,row.get(1)?))).unwrap();
    assert_eq!(counts, (1, "COMMITTED".into()));
}

#[test]
fn transient_sync_failure_uses_durable_backoff_before_retry() {
    let mut f = fixture();
    let now = t("2026-09-29T01:00:00Z");
    let cart = cart_with_one(&f, now);
    f.store
        .checkout(CheckoutRequest {
            tenant_id: f.tenant,
            branch_id: f.branch,
            device_id: f.device,
            register_id: f.register,
            user_id: f.user,
            cart_id: cart,
            operation_id: OperationId::new(),
            cash_session_id: Some(f.cash_session),
            payments: vec![PaymentInput {
                kind: TenderKind::Cash,
                amount: Money(1100),
                tendered: Some(Money(1100)),
                reference: None,
            }],
            now,
        })
        .unwrap();
    let context = bhaipos_store::LocalTerminalContext {
        tenant_id: f.tenant,
        branch_id: f.branch,
        device_id: f.device,
        register_id: f.register,
    };
    let first = f
        .store
        .claim_sync_batch(context, "device-secret-0123456789", 10, now)
        .unwrap()
        .remove(0);
    f.store
        .complete_sync_delivery(
            context,
            first.material.mutation_id,
            first.lease_token,
            SyncDeliveryOutcome::Retry("network unavailable".into()),
            now,
        )
        .unwrap();
    assert!(f
        .store
        .claim_sync_batch(
            context,
            "device-secret-0123456789",
            10,
            now + chrono::Duration::seconds(1)
        )
        .unwrap()
        .is_empty());
    let retry = f
        .store
        .claim_sync_batch(
            context,
            "device-secret-0123456789",
            10,
            now + chrono::Duration::seconds(3),
        )
        .unwrap();
    assert_eq!(retry.len(), 1);
    assert_eq!(retry[0].material.mutation_id, first.material.mutation_id);
}

#[test]
fn rotated_or_revoked_device_cannot_submit_sync_mutations() {
    let mut f = fixture();
    let now = t("2026-09-29T01:00:00Z");
    f.store
        .record_cash_movement(CashMovementRequest {
            tenant_id: f.tenant,
            branch_id: f.branch,
            device_id: f.device,
            user_id: f.user,
            cash_session_id: f.cash_session,
            operation_id: OperationId::new(),
            kind: CashMovementKind::PaidIn,
            amount: Money(500),
            reason: Some("top up".into()),
            now,
        })
        .unwrap();
    let context = bhaipos_store::LocalTerminalContext {
        tenant_id: f.tenant,
        branch_id: f.branch,
        device_id: f.device,
        register_id: f.register,
    };
    assert_eq!(
        f.store
            .rotate_local_device_credential(context, f.user, "device-secret-v2-012345", now)
            .unwrap(),
        2
    );
    assert!(matches!(
        f.store
            .claim_sync_batch(context, "device-secret-0123456789", 10, now),
        Err(StoreError::Authorization(_))
    ));
    let envelope = f
        .store
        .claim_sync_batch(context, "device-secret-v2-012345", 10, now)
        .unwrap()
        .remove(0);
    f.store.set_device_status(f.device, "REVOKED", now).unwrap();
    assert!(matches!(
        f.store
            .accept_hub_mutation(&envelope, "device-secret-v2-012345", now),
        Err(StoreError::Authorization(_))
    ));
}

#[test]
fn unsafe_sync_semantics_require_explicit_append_only_resolution() {
    let mut f = fixture();
    let now = t("2026-09-29T01:00:00Z");
    let mutation = Uuid::new_v4();
    let operation = OperationId::new();
    f.store.connection().execute("INSERT INTO sync_queue(id,tenant_id,branch_id,device_id,operation_id,entity_type,entity_id,mutation_type,payload_json,state,created_at,updated_at) VALUES(?1,?2,?3,?4,?5,'catalog_patch','product-1','UPDATE','{}','PENDING',?6,?6)",params![mutation.to_string(),f.tenant.to_string(),f.branch.to_string(),f.device.to_string(),operation.to_string(),now.to_rfc3339()]).unwrap();
    let context = bhaipos_store::LocalTerminalContext {
        tenant_id: f.tenant,
        branch_id: f.branch,
        device_id: f.device,
        register_id: f.register,
    };
    let envelope = f
        .store
        .claim_sync_batch(context, "device-secret-0123456789", 10, now)
        .unwrap()
        .remove(0);
    let result = f
        .store
        .accept_hub_mutation(&envelope, "device-secret-0123456789", now)
        .unwrap();
    assert_eq!(result.state, "REQUIRES_REVIEW");
    f.store
        .complete_sync_delivery(
            context,
            mutation,
            envelope.lease_token,
            SyncDeliveryOutcome::Hub(result.clone()),
            now,
        )
        .unwrap();
    let resolution = f
        .store
        .resolve_sync_conflict(
            context,
            f.user,
            result.hub_sequence,
            "REJECT",
            "stale catalogue version",
            now,
        )
        .unwrap();
    assert!(!resolution.is_nil());
    assert!(matches!(
        f.store.resolve_sync_conflict(
            context,
            f.user,
            result.hub_sequence,
            "APPLY",
            "second decision",
            now
        ),
        Err(StoreError::Db(_))
    ));
}

#[test]
fn sale_void_is_compensating_idempotent_and_preserves_original_receipt() {
    let mut f = fixture();
    let now = t("2026-09-29T01:00:00Z");
    let cart = cart_with_one(&f, now);
    let sale = f
        .store
        .checkout(CheckoutRequest {
            tenant_id: f.tenant,
            branch_id: f.branch,
            device_id: f.device,
            register_id: f.register,
            user_id: f.user,
            cart_id: cart,
            operation_id: OperationId::new(),
            cash_session_id: Some(f.cash_session),
            payments: vec![PaymentInput {
                kind: TenderKind::Cash,
                amount: Money(1100),
                tendered: Some(Money(1100)),
                reference: None,
            }],
            now,
        })
        .unwrap();
    let receipt_before = f.store.receipt_snapshot(sale.sale_id).unwrap();
    assert_eq!(
        f.store
            .cash_session_report(f.tenant, f.branch, f.cash_session)
            .unwrap()
            .expected_cash,
        Money(21_100)
    );

    let manager = UserId::new();
    f.store
        .create_user(
            manager,
            f.tenant,
            "Manager",
            &hash_pin("7777").unwrap(),
            now,
        )
        .unwrap();
    let role = Uuid::new_v4();
    f.store.create_role(role, f.tenant, "void-manager").unwrap();
    f.store
        .define_permission("manager.approve", "Approve sensitive actions")
        .unwrap();
    f.store.grant_permission(role, "manager.approve").unwrap();
    f.store.assign_role(manager, role, Some(f.branch)).unwrap();

    let op = OperationId::new();
    let reason = "accidental duplicate scan";
    let secret = b"void-approval-secret";
    let binding = ApprovalBinding {
        tenant_id: f.tenant,
        approver_user_id: manager,
        device_id: f.device,
        operation_id: op,
        action: "sale.void".into(),
        entity_id: sale.sale_id.to_string(),
        payload_sha256: Store::void_sale_payload_sha256(sale.sale_id, Some(f.cash_session), reason),
        nonce: Uuid::new_v4().to_string(),
        expires_at: now + chrono::Duration::minutes(2),
    };
    let sig = sign_approval(secret, &binding);
    let approval = f
        .store
        .consume_manager_approval(secret, &binding, &sig, "manager.approve", now)
        .unwrap();
    let req = || bhaipos_store::VoidSaleRequest {
        tenant_id: f.tenant,
        branch_id: f.branch,
        device_id: f.device,
        user_id: f.user,
        operation_id: op,
        sale_id: sale.sale_id,
        cash_session_id: Some(f.cash_session),
        approval_ref: approval,
        reason: reason.into(),
        now,
    };
    let first = f.store.void_sale(req()).unwrap();
    let second = f.store.void_sale(req()).unwrap();
    assert_eq!(first, second);
    assert_eq!(first.reversed_total, Money(1100));
    assert_eq!(
        f.store
            .stock_quantity(f.tenant, f.branch, f.product)
            .unwrap(),
        10_000
    );
    let status: String = f
        .store
        .connection()
        .query_row(
            "SELECT status FROM sales WHERE id=?1",
            params![sale.sale_id.to_string()],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(status, "VOIDED");
    let report = f
        .store
        .cash_session_report(f.tenant, f.branch, f.cash_session)
        .unwrap();
    assert_eq!(report.cash_voids, Money(1100));
    assert_eq!(report.expected_cash, Money(20_000));
    assert_eq!(
        f.store.receipt_snapshot(sale.sale_id).unwrap(),
        receipt_before
    );
    let void_count: i64 = f
        .store
        .connection()
        .query_row(
            "SELECT COUNT(*) FROM sale_voids WHERE sale_id=?1",
            params![sale.sale_id.to_string()],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(void_count, 1);
}

#[test]
fn inventory_cache_is_reconstructable_from_append_only_movements() {
    let mut f = fixture();
    let now = t("2026-09-29T01:00:00Z");
    let context = bhaipos_store::LocalTerminalContext {
        tenant_id: f.tenant,
        branch_id: f.branch,
        device_id: f.device,
        register_id: f.register,
    };
    f.store.connection().execute("UPDATE stock_levels SET quantity_milli=123 WHERE tenant_id=?1 AND branch_id=?2 AND centre_id=?3 AND product_id=?4",params![f.tenant.to_string(),f.branch.to_string(),f.centre.to_string(),f.product.to_string()]).unwrap();
    let inspect = f
        .store
        .rebuild_stock_cache(context, f.user, false, now)
        .unwrap();
    assert_eq!(inspect.mismatches, 1);
    assert_eq!(
        f.store
            .stock_quantity(f.tenant, f.branch, f.product)
            .unwrap(),
        123
    );
    let repaired = f
        .store
        .rebuild_stock_cache(context, f.user, true, now)
        .unwrap();
    assert_eq!(repaired.mismatches, 1);
    assert_eq!(
        f.store
            .stock_quantity(f.tenant, f.branch, f.product)
            .unwrap(),
        10_000
    );
}

#[test]
fn receiving_and_sale_preserve_integer_weighted_average_valuation() {
    let mut f = fixture();
    let now = t("2026-09-29T01:00:00Z");
    let context = bhaipos_store::LocalTerminalContext {
        tenant_id: f.tenant,
        branch_id: f.branch,
        device_id: f.device,
        register_id: f.register,
    };
    let supplier = Uuid::new_v4();
    f.store.connection().execute("INSERT INTO suppliers(id,tenant_id,name,active,created_at) VALUES(?1,?2,'Supplier B',1,?3)",params![supplier.to_string(),f.tenant.to_string(),now.to_rfc3339()]).unwrap();
    let request = || bhaipos_store::InventoryReceiptRequest {
        context,
        user_id: f.user,
        operation_id: OperationId(Uuid::nil()),
        supplier_id: supplier,
        centre_id: f.centre,
        supplier_document_no: Some("INV-2".into()),
        lines: vec![bhaipos_store::InventoryReceiptLineInput {
            product_id: f.product,
            received_quantity: QuantityMilli(10_000),
            rejected_quantity: QuantityMilli(0),
            damaged_quantity: QuantityMilli(0),
            unit_cost: Money(900),
            lot_number: Some("LOT-2".into()),
            expires_on: Some("2027-01-01".into()),
        }],
        now,
    };
    let received = f.store.receive_inventory(request()).unwrap();
    let replay = f.store.receive_inventory(request()).unwrap();
    assert_eq!(received, replay);
    let expiring = f
        .store
        .expiring_lots(context, f.user, "2027-01-31")
        .unwrap();
    assert_eq!(expiring.len(), 1);
    assert_eq!(expiring[0].lot_number.as_deref(), Some("LOT-2"));
    let before = f
        .store
        .inventory_valuation(context, f.user, f.centre, f.product)
        .unwrap();
    assert_eq!(before.quantity, QuantityMilli(20_000));
    assert_eq!(before.total_value, Money(16_000));
    assert_eq!(before.weighted_average_cost, Money(800));
    let cart = cart_with_one(&f, now + chrono::Duration::minutes(1));
    f.store
        .checkout(CheckoutRequest {
            tenant_id: f.tenant,
            branch_id: f.branch,
            device_id: f.device,
            register_id: f.register,
            user_id: f.user,
            cart_id: cart,
            operation_id: OperationId::new(),
            cash_session_id: Some(f.cash_session),
            payments: vec![PaymentInput {
                kind: TenderKind::Cash,
                amount: Money(1100),
                tendered: Some(Money(1100)),
                reference: None,
            }],
            now: now + chrono::Duration::minutes(1),
        })
        .unwrap();
    let after = f
        .store
        .inventory_valuation(context, f.user, f.centre, f.product)
        .unwrap();
    assert_eq!(after.quantity, QuantityMilli(19_000));
    assert_eq!(after.total_value, Money(15_200));
}

#[test]
fn transfer_dispatch_and_partial_receipt_preserve_evidence_without_teleporting() {
    let mut f = fixture();
    let now = t("2026-09-29T01:00:00Z");
    let context = bhaipos_store::LocalTerminalContext {
        tenant_id: f.tenant,
        branch_id: f.branch,
        device_id: f.device,
        register_id: f.register,
    };
    let destination = Uuid::new_v4();
    f.store
        .create_inventory_centre(destination, f.tenant, f.branch, "BACK", "Back Store")
        .unwrap();
    let created = f
        .store
        .create_inventory_transfer(
            context,
            f.user,
            OperationId::new(),
            f.centre,
            f.branch,
            destination,
            &[bhaipos_store::InventoryTransferLineInput {
                product_id: f.product,
                quantity: QuantityMilli(2_000),
                lot_id: None,
            }],
            now,
        )
        .unwrap();
    f.store
        .dispatch_transfer(
            context,
            f.user,
            created.transfer_id,
            OperationId::new(),
            now,
        )
        .unwrap();
    assert_eq!(
        f.store
            .stock_quantity(f.tenant, f.branch, f.product)
            .unwrap(),
        8_000
    );
    let line: String = f
        .store
        .connection()
        .query_row(
            "SELECT id FROM inventory_transfer_lines WHERE transfer_id=?1",
            params![created.transfer_id.to_string()],
            |row| row.get(0),
        )
        .unwrap();
    let line_id = Uuid::parse_str(&line).unwrap();
    let partial = f
        .store
        .receive_transfer(
            context,
            f.user,
            created.transfer_id,
            OperationId::new(),
            &[bhaipos_store::TransferReceiptLineInput {
                transfer_line_id: line_id,
                received_quantity: QuantityMilli(1_000),
                damaged_quantity: QuantityMilli(0),
                note: None,
            }],
            now,
        )
        .unwrap();
    assert_eq!(partial.status, "PARTIALLY_RECEIVED");
    let complete = f
        .store
        .receive_transfer(
            context,
            f.user,
            created.transfer_id,
            OperationId::new(),
            &[bhaipos_store::TransferReceiptLineInput {
                transfer_line_id: line_id,
                received_quantity: QuantityMilli(500),
                damaged_quantity: QuantityMilli(500),
                note: Some("damaged in transit".into()),
            }],
            now,
        )
        .unwrap();
    assert_eq!(complete.status, "RECEIVED");
    let balances:(i64,i64,i64)=f.store.connection().query_row("SELECT (SELECT quantity_milli FROM stock_levels WHERE centre_id=?1 AND product_id=?3),(SELECT quantity_milli FROM stock_levels WHERE centre_id=?2 AND product_id=?3),(SELECT damaged_qty_milli FROM inventory_transfer_lines WHERE id=?4)",params![f.centre.to_string(),destination.to_string(),f.product.to_string(),line],|row|Ok((row.get(0)?,row.get(1)?,row.get(2)?))).unwrap();
    assert_eq!(balances, (8_000, 1_500, 500));
}

#[test]
fn stocktake_reconciles_movements_after_snapshot_instead_of_erasing_them() {
    let mut f = fixture();
    let snapshot = t("2026-09-29T01:00:00Z");
    let context = bhaipos_store::LocalTerminalContext {
        tenant_id: f.tenant,
        branch_id: f.branch,
        device_id: f.device,
        register_id: f.register,
    };
    let stocktake = f
        .store
        .create_stocktake(context, f.user, f.centre, &[f.product], snapshot)
        .unwrap();
    let sale_time = snapshot + chrono::Duration::minutes(1);
    let cart = cart_with_one(&f, sale_time);
    f.store
        .checkout(CheckoutRequest {
            tenant_id: f.tenant,
            branch_id: f.branch,
            device_id: f.device,
            register_id: f.register,
            user_id: f.user,
            cart_id: cart,
            operation_id: OperationId::new(),
            cash_session_id: Some(f.cash_session),
            payments: vec![PaymentInput {
                kind: TenderKind::Cash,
                amount: Money(1100),
                tendered: Some(Money(1100)),
                reference: None,
            }],
            now: sale_time,
        })
        .unwrap();
    f.store
        .count_stocktake_line(
            context,
            f.user,
            stocktake,
            f.product,
            QuantityMilli(8_000),
            sale_time + chrono::Duration::minutes(1),
        )
        .unwrap();
    let approved = f
        .store
        .approve_stocktake(
            context,
            f.user,
            stocktake,
            OperationId::new(),
            sale_time + chrono::Duration::minutes(2),
        )
        .unwrap();
    assert_eq!(approved.adjustments, 1);
    assert_eq!(
        f.store
            .stock_quantity(f.tenant, f.branch, f.product)
            .unwrap(),
        8_000
    );
    let evidence:(i64,i64,i64)=f.store.connection().query_row("SELECT movements_after_snapshot_milli,reconciled_expected_qty_milli,variance_qty_milli FROM stocktake_lines WHERE stocktake_id=?1",params![stocktake.to_string()],|row|Ok((row.get(0)?,row.get(1)?,row.get(2)?))).unwrap();
    assert_eq!(evidence, (-1_000, 9_000, -1_000));
}

#[test]
fn waste_is_idempotent_costed_and_lot_expiry_is_queryable() {
    let mut f = fixture();
    let now = t("2026-09-29T01:00:00Z");
    let context = bhaipos_store::LocalTerminalContext {
        tenant_id: f.tenant,
        branch_id: f.branch,
        device_id: f.device,
        register_id: f.register,
    };
    let op = OperationId::new();
    let first = f
        .store
        .record_waste(
            context,
            f.user,
            op,
            f.centre,
            f.product,
            None,
            QuantityMilli(500),
            "SPOILED",
            Some("cold-chain break"),
            now,
        )
        .unwrap();
    let second = f
        .store
        .record_waste(
            context,
            f.user,
            op,
            f.centre,
            f.product,
            None,
            QuantityMilli(500),
            "SPOILED",
            Some("cold-chain break"),
            now,
        )
        .unwrap();
    assert_eq!(first, second);
    assert_eq!(first.cost_value, Money(350));
    assert_eq!(
        f.store
            .stock_quantity(f.tenant, f.branch, f.product)
            .unwrap(),
        9_500
    );
    let events: i64 = f
        .store
        .connection()
        .query_row(
            "SELECT COUNT(*) FROM waste_events WHERE operation_id=?1",
            params![op.to_string()],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(events, 1);
}

#[test]
fn purchase_order_receiving_is_partial_idempotent_and_records_cost_variance() {
    let mut f = fixture();
    let now = t("2026-09-29T02:00:00Z");
    let context = bhaipos_store::LocalTerminalContext {
        tenant_id: f.tenant,
        branch_id: f.branch,
        device_id: f.device,
        register_id: f.register,
    };
    let supplier = Uuid::new_v4();
    f.store
        .create_supplier(
            context,
            f.user,
            supplier,
            "Supplier A",
            Some("+97317000000"),
            Some(30),
            now,
        )
        .unwrap();
    let created = f
        .store
        .create_purchase_order(
            context,
            f.user,
            OperationId::new(),
            supplier,
            &[bhaipos_store::PurchaseOrderLineInput {
                product_id: f.product,
                quantity: QuantityMilli(2_000),
                unit_cost: Money(700),
                tax_rate_bps: 0,
            }],
            now,
        )
        .unwrap();
    f.store
        .progress_purchase_order(
            context,
            f.user,
            created.purchase_order_id,
            OperationId::new(),
            "APPROVED",
            now,
        )
        .unwrap();
    let raw: String = f
        .store
        .connection()
        .query_row(
            "SELECT id FROM purchase_order_lines WHERE po_id=?1",
            params![created.purchase_order_id.to_string()],
            |row| row.get(0),
        )
        .unwrap();
    let line = Uuid::parse_str(&raw).unwrap();
    let op = OperationId::new();
    let receive = || {
        vec![bhaipos_store::PurchaseOrderReceiptLineInput {
            purchase_order_line_id: line,
            received_quantity: QuantityMilli(1_000),
            rejected_quantity: QuantityMilli(0),
            damaged_quantity: QuantityMilli(0),
            unit_cost: Money(750),
            lot_number: None,
            expires_on: None,
        }]
    };
    let first = f
        .store
        .receive_purchase_order(
            context,
            f.user,
            created.purchase_order_id,
            op,
            f.centre,
            &receive(),
            Some("DN-1"),
            now,
        )
        .unwrap();
    let replay = f
        .store
        .receive_purchase_order(
            context,
            f.user,
            created.purchase_order_id,
            op,
            f.centre,
            &receive(),
            Some("DN-1"),
            now,
        )
        .unwrap();
    assert_eq!(first, replay);
    assert_eq!(first.status, "PARTIALLY_RECEIVED");
    assert_eq!(first.cost_variance, Money(50));
    let received: i64 = f
        .store
        .connection()
        .query_row(
            "SELECT received_qty_milli FROM purchase_order_lines WHERE id=?1",
            params![line.to_string()],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(received, 1_000);
}

#[test]
fn supplier_invoice_payment_and_statement_are_exact_and_replay_safe() {
    let mut f = fixture();
    let now = t("2026-09-29T02:00:00Z");
    let context = bhaipos_store::LocalTerminalContext {
        tenant_id: f.tenant,
        branch_id: f.branch,
        device_id: f.device,
        register_id: f.register,
    };
    let supplier = Uuid::new_v4();
    f.store
        .create_supplier(
            context,
            f.user,
            supplier,
            "Supplier Finance",
            None,
            Some(15),
            now,
        )
        .unwrap();
    let invoice = f
        .store
        .post_supplier_invoice(
            context,
            f.user,
            OperationId::new(),
            supplier,
            "INV-100",
            "2026-09-29",
            Some("2026-10-14"),
            None,
            None,
            &[bhaipos_store::SupplierInvoiceLineInput {
                product_id: Some(f.product),
                description: "Milk".into(),
                quantity: QuantityMilli(2_000),
                unit_cost: Money(700),
                tax: Money(140),
            }],
            now,
        )
        .unwrap();
    assert_eq!(invoice.total, Money(1540));
    let payment_op = OperationId::new();
    let allocations = vec![bhaipos_store::SupplierPaymentAllocationInput {
        invoice_id: invoice.invoice_id,
        amount: Money(1000),
    }];
    let first = f
        .store
        .pay_supplier(
            context,
            f.user,
            payment_op,
            supplier,
            "BANK_TRANSFER",
            Money(1000),
            Some("BANK-1"),
            &allocations,
            now,
        )
        .unwrap();
    let replay = f
        .store
        .pay_supplier(
            context,
            f.user,
            payment_op,
            supplier,
            "BANK_TRANSFER",
            Money(1000),
            Some("BANK-1"),
            &allocations,
            now,
        )
        .unwrap();
    assert_eq!(first, replay);
    let statement = f
        .store
        .supplier_statement(
            context,
            f.user,
            supplier,
            "2026-01-01",
            "2026-12-31T23:59:59Z",
        )
        .unwrap();
    assert_eq!(statement.debits, Money(1540));
    assert_eq!(statement.credits, Money(1000));
    assert_eq!(statement.closing_balance, Money(540));
    let over = f
        .store
        .pay_supplier(
            context,
            f.user,
            OperationId::new(),
            supplier,
            "CASH",
            Money(600),
            None,
            &[bhaipos_store::SupplierPaymentAllocationInput {
                invoice_id: invoice.invoice_id,
                amount: Money(600),
            }],
            now,
        )
        .unwrap_err();
    assert!(matches!(over, StoreError::Conflict(_)));
}

#[test]
fn supplier_return_uses_stock_and_credit_ledgers_without_rewriting_evidence() {
    let mut f = fixture();
    let now = t("2026-09-29T03:00:00Z");
    let context = bhaipos_store::LocalTerminalContext {
        tenant_id: f.tenant,
        branch_id: f.branch,
        device_id: f.device,
        register_id: f.register,
    };
    let supplier = Uuid::new_v4();
    f.store
        .create_supplier(
            context,
            f.user,
            supplier,
            "Returns Supplier",
            None,
            None,
            now,
        )
        .unwrap();
    let dispatch_op = OperationId::new();
    let lines = vec![bhaipos_store::SupplierReturnLineInput {
        product_id: f.product,
        lot_id: None,
        quantity: QuantityMilli(500),
        unit_cost: Money(700),
    }];
    let dispatched = f
        .store
        .dispatch_supplier_return(
            context,
            f.user,
            dispatch_op,
            supplier,
            f.centre,
            "DAMAGED",
            &lines,
            now,
        )
        .unwrap();
    let replay = f
        .store
        .dispatch_supplier_return(
            context,
            f.user,
            dispatch_op,
            supplier,
            f.centre,
            "DAMAGED",
            &lines,
            now,
        )
        .unwrap();
    assert_eq!(dispatched, replay);
    assert_eq!(
        f.store
            .stock_quantity(f.tenant, f.branch, f.product)
            .unwrap(),
        9_500
    );
    let settled = f
        .store
        .settle_supplier_return(
            context,
            f.user,
            dispatched.supplier_return_id,
            OperationId::new(),
            "CREDITED",
            now,
        )
        .unwrap();
    assert_eq!(settled.value, Money(350));
    let statement = f
        .store
        .supplier_statement(
            context,
            f.user,
            supplier,
            "2026-01-01",
            "2026-12-31T23:59:59Z",
        )
        .unwrap();
    assert_eq!(statement.closing_balance, Money(-350));
}

#[test]
fn customer_credit_checkout_and_payment_are_atomic_idempotent_ledger_events() {
    let mut f = fixture();
    let now = t("2026-09-29T04:00:00Z");
    let context = bhaipos_store::LocalTerminalContext {
        tenant_id: f.tenant,
        branch_id: f.branch,
        device_id: f.device,
        register_id: f.register,
    };
    let customer = Uuid::new_v4();
    f.store
        .create_customer(
            context,
            f.user,
            OperationId::new(),
            customer,
            "Credit Customer",
            Some("39000000"),
            None,
            now,
        )
        .unwrap();
    f.store
        .set_customer_credit_account(
            context,
            f.user,
            customer,
            Money(2_000),
            Some(30),
            "ACTIVE",
            now,
        )
        .unwrap();
    let cart = cart_with_one(&f, now);
    f.store
        .associate_cart_customer(context, f.user, cart, customer, now)
        .unwrap();
    let checkout = f
        .store
        .checkout(CheckoutRequest {
            tenant_id: f.tenant,
            branch_id: f.branch,
            device_id: f.device,
            register_id: f.register,
            user_id: f.user,
            cart_id: cart,
            operation_id: OperationId::new(),
            cash_session_id: None,
            payments: vec![PaymentInput {
                kind: TenderKind::CustomerCredit,
                amount: Money(1100),
                tendered: None,
                reference: None,
            }],
            now,
        })
        .unwrap();
    assert_eq!(checkout.total, Money(1100));
    let balance = f
        .store
        .customer_credit_balance(context, f.user, customer, "2026-09-30")
        .unwrap();
    assert_eq!(balance.outstanding, Money(1100));
    assert_eq!(balance.available, Money(900));
    let op = OperationId::new();
    let first = f
        .store
        .receive_customer_credit_payment(
            context,
            f.user,
            op,
            customer,
            "BENEFIT_PAY",
            Money(500),
            Some("BP-1"),
            now,
        )
        .unwrap();
    let replay = f
        .store
        .receive_customer_credit_payment(
            context,
            f.user,
            op,
            customer,
            "BENEFIT_PAY",
            Money(500),
            Some("BP-1"),
            now,
        )
        .unwrap();
    assert_eq!(first, replay);
    assert_eq!(first.outstanding_balance, Money(600));
}

#[test]
fn loyalty_ledger_rejects_over_redemption_and_replays_exact_event() {
    let mut f = fixture();
    let now = t("2026-09-29T04:00:00Z");
    let context = bhaipos_store::LocalTerminalContext {
        tenant_id: f.tenant,
        branch_id: f.branch,
        device_id: f.device,
        register_id: f.register,
    };
    let customer = Uuid::new_v4();
    f.store
        .create_customer(
            context,
            f.user,
            OperationId::new(),
            customer,
            "Loyal Customer",
            None,
            None,
            now,
        )
        .unwrap();
    let op = OperationId::new();
    let first = f
        .store
        .record_loyalty_event(
            context, f.user, op, customer, "EARN", 100, "MANUAL", None, None, now,
        )
        .unwrap();
    let replay = f
        .store
        .record_loyalty_event(
            context, f.user, op, customer, "EARN", 100, "MANUAL", None, None, now,
        )
        .unwrap();
    assert_eq!(first, replay);
    let error = f
        .store
        .record_loyalty_event(
            context,
            f.user,
            OperationId::new(),
            customer,
            "REDEEM",
            -101,
            "SALE",
            None,
            None,
            now,
        )
        .unwrap_err();
    assert!(matches!(error, StoreError::Conflict(_)));
}

#[test]
fn production_completion_is_idempotent_and_consumes_components_once() {
    let mut f = fixture();
    let now = t("2026-09-29T05:00:00Z");
    let context = bhaipos_store::LocalTerminalContext {
        tenant_id: f.tenant,
        branch_id: f.branch,
        device_id: f.device,
        register_id: f.register,
    };
    let output = ProductId::new();
    f.store
        .create_product(
            output,
            NewProduct {
                tenant_id: f.tenant,
                sku: "SKU-OUT",
                name: "Prepared Milk",
                barcode: "6290000000091",
                price: Money(2000),
                cost: Money(0),
                tax_rate_bps: 1000,
                tax_inclusive: true,
                track_inventory: true,
                allow_decimal_qty: false,
            },
            now,
        )
        .unwrap();
    let recipe = f
        .store
        .create_recipe(
            context,
            f.user,
            output,
            QuantityMilli(1_000),
            &[bhaipos_store::RecipeComponentInput {
                product_id: f.product,
                quantity: QuantityMilli(2_000),
            }],
            now,
        )
        .unwrap();
    let order = f
        .store
        .create_production_order(
            context,
            f.user,
            OperationId::new(),
            recipe,
            f.centre,
            QuantityMilli(1_000),
            now,
        )
        .unwrap();
    let op = OperationId::new();
    let usage = vec![bhaipos_store::ProductionConsumptionInput {
        product_id: f.product,
        quantity: QuantityMilli(2_000),
        lot_id: None,
    }];
    let first = f
        .store
        .complete_production(
            context,
            f.user,
            order.production_order_id,
            op,
            QuantityMilli(1_000),
            &usage,
            now,
        )
        .unwrap();
    let replay = f
        .store
        .complete_production(
            context,
            f.user,
            order.production_order_id,
            op,
            QuantityMilli(1_000),
            &usage,
            now,
        )
        .unwrap();
    assert_eq!(first, replay);
    assert_eq!(first.output_cost, Money(1400));
    assert_eq!(
        f.store
            .stock_quantity(f.tenant, f.branch, f.product)
            .unwrap(),
        8_000
    );
    assert_eq!(
        f.store.stock_quantity(f.tenant, f.branch, output).unwrap(),
        1_000
    );
}

#[test]
fn expense_workflow_is_idempotent_append_evidenced_and_reports_exact_profit() {
    let mut f = fixture();
    let now = t("2026-09-29T06:00:00Z");
    let context = bhaipos_store::LocalTerminalContext {
        tenant_id: f.tenant,
        branch_id: f.branch,
        device_id: f.device,
        register_id: f.register,
    };
    let category = Uuid::new_v4();
    f.store
        .create_expense_category(
            context,
            f.user,
            OperationId::new(),
            category,
            "Utilities",
            now,
        )
        .unwrap();
    let expense_id = Uuid::new_v4();
    let create_operation = OperationId::new();
    let created = f
        .store
        .create_expense(
            context,
            f.user,
            create_operation,
            expense_id,
            category,
            "Electricity bill",
            Money(15_000),
            Money(1_364),
            "2026-09-29",
            now,
        )
        .unwrap();
    let replay = f
        .store
        .create_expense(
            context,
            f.user,
            create_operation,
            expense_id,
            category,
            "Electricity bill",
            Money(15_000),
            Money(1_364),
            "2026-09-29",
            now,
        )
        .unwrap();
    assert_eq!(created, replay);
    assert_eq!(created.status, "DRAFT");
    f.store
        .submit_expense(context, f.user, OperationId::new(), expense_id, now)
        .unwrap();
    f.store
        .decide_expense(
            context,
            f.user,
            OperationId::new(),
            expense_id,
            true,
            Some("Within monthly budget"),
            now,
        )
        .unwrap();
    let payment_operation = OperationId::new();
    let paid = f
        .store
        .pay_expense(
            context,
            f.user,
            payment_operation,
            expense_id,
            "BANK_TRANSFER",
            Some("BANK-EXP-1"),
            now,
        )
        .unwrap();
    let paid_replay = f
        .store
        .pay_expense(
            context,
            f.user,
            payment_operation,
            expense_id,
            "BANK_TRANSFER",
            Some("BANK-EXP-1"),
            now,
        )
        .unwrap();
    assert_eq!(paid, paid_replay);
    assert_eq!(paid.amount, Money(15_000));
    assert_eq!(paid.status, "PAID");
    let sale_cart = cart_with_one(&f, now);
    f.store
        .checkout(CheckoutRequest {
            tenant_id: f.tenant,
            branch_id: f.branch,
            device_id: f.device,
            register_id: f.register,
            user_id: f.user,
            cart_id: sale_cart,
            operation_id: OperationId::new(),
            cash_session_id: Some(f.cash_session),
            payments: vec![PaymentInput {
                kind: TenderKind::Cash,
                amount: Money(1100),
                tendered: Some(Money(1100)),
                reference: None,
            }],
            now,
        })
        .unwrap();
    assert!(f
        .store
        .connection()
        .execute(
            "UPDATE expenses SET amount_fils=1 WHERE id=?1",
            params![expense_id.to_string()],
        )
        .is_err());
    let report = f
        .store
        .operating_profit_report(
            context,
            f.user,
            "2026-09-29T00:00:00Z",
            "2026-09-30T00:00:00Z",
        )
        .unwrap();
    assert_eq!(report.net_sales, Money(1_000));
    assert_eq!(report.cogs, Money(700));
    assert_eq!(report.gross_profit, Money(300));
    assert_eq!(report.operating_expenses, Money(15_000));
    assert_eq!(report.operating_profit, Money(-14_700));
    assert_eq!(
        f.store
            .connection()
            .query_row(
                "SELECT COUNT(*) FROM expense_events WHERE expense_id=?1",
                params![expense_id.to_string()],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
        4
    );
}

#[test]
fn delivery_collection_and_courier_cash_settlement_are_real_idempotent_financial_events() {
    let mut f = fixture();
    let now = t("2026-09-29T07:00:00Z");
    let context = bhaipos_store::LocalTerminalContext {
        tenant_id: f.tenant,
        branch_id: f.branch,
        device_id: f.device,
        register_id: f.register,
    };
    let worker_id = Uuid::new_v4();
    f.store
        .create_delivery_worker(
            context,
            f.user,
            OperationId::new(),
            worker_id,
            "Rider One",
            Some("39000001"),
            now,
        )
        .unwrap();
    let delivery_id = Uuid::new_v4();
    let create_operation = OperationId::new();
    let created = f
        .store
        .create_delivery_order(
            context,
            f.user,
            create_operation,
            delivery_id,
            None,
            None,
            Some("39000002"),
            Money(5_000),
            Some("Cash on delivery"),
            now,
        )
        .unwrap();
    let replay = f
        .store
        .create_delivery_order(
            context,
            f.user,
            create_operation,
            delivery_id,
            None,
            None,
            Some("39000002"),
            Money(5_000),
            Some("Cash on delivery"),
            now,
        )
        .unwrap();
    assert_eq!(created, replay);
    assert_eq!(created.status, "PENDING");
    assert!(matches!(
        f.store.create_delivery_order(
            context,
            f.user,
            create_operation,
            delivery_id,
            None,
            None,
            Some("39000002"),
            Money(5_001),
            Some("Cash on delivery"),
            now,
        ),
        Err(StoreError::Conflict(_))
    ));
    assert!(matches!(
        f.store.transition_delivery(
            context,
            f.user,
            OperationId::new(),
            delivery_id,
            "DELIVERED",
            None,
            now,
        ),
        Err(StoreError::Conflict(_))
    ));
    for status in ["PREPARING", "READY"] {
        f.store
            .transition_delivery(
                context,
                f.user,
                OperationId::new(),
                delivery_id,
                status,
                None,
                now,
            )
            .unwrap();
    }
    f.store
        .transition_delivery(
            context,
            f.user,
            OperationId::new(),
            delivery_id,
            "DISPATCHED",
            Some(worker_id),
            now,
        )
        .unwrap();
    let collection_operation = OperationId::new();
    let collection = f
        .store
        .collect_delivery_payment(
            context,
            f.user,
            collection_operation,
            delivery_id,
            "CASH",
            Money(5_000),
            None,
            now,
        )
        .unwrap();
    let collection_replay = f
        .store
        .collect_delivery_payment(
            context,
            f.user,
            collection_operation,
            delivery_id,
            "CASH",
            Money(5_000),
            None,
            now,
        )
        .unwrap();
    assert_eq!(collection, collection_replay);
    assert_eq!(collection.payment_state, "PAID");
    assert!(matches!(
        f.store.collect_delivery_payment(
            context,
            f.user,
            OperationId::new(),
            delivery_id,
            "CASH",
            Money(5_000),
            None,
            now,
        ),
        Err(StoreError::Conflict(_))
    ));
    f.store
        .transition_delivery(
            context,
            f.user,
            OperationId::new(),
            delivery_id,
            "DELIVERED",
            None,
            now,
        )
        .unwrap();
    let settlement_operation = OperationId::new();
    let settlement = f
        .store
        .settle_courier_cash(
            context,
            f.user,
            settlement_operation,
            worker_id,
            Money(4_900),
            Some("100 fils shortage acknowledged"),
            now,
        )
        .unwrap();
    let settlement_replay = f
        .store
        .settle_courier_cash(
            context,
            f.user,
            settlement_operation,
            worker_id,
            Money(4_900),
            Some("100 fils shortage acknowledged"),
            now,
        )
        .unwrap();
    assert_eq!(settlement, settlement_replay);
    assert_eq!(settlement.expected_cash, Money(5_000));
    assert_eq!(settlement.returned_cash, Money(4_900));
    assert_eq!(settlement.variance, Money(-100));
    assert_eq!(settlement.status, "DISCREPANCY");
    assert!(matches!(
        f.store.settle_courier_cash(
            context,
            f.user,
            settlement_operation,
            worker_id,
            Money(5_000),
            None,
            now,
        ),
        Err(StoreError::Conflict(_))
    ));
    assert_eq!(
        f.store
            .open_courier_cash(context, f.user, worker_id)
            .unwrap(),
        Money::ZERO
    );
    assert_eq!(
        f.store
            .connection()
            .query_row(
                "SELECT COUNT(*) FROM delivery_collections WHERE delivery_id=?1",
                params![delivery_id.to_string()],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
        1
    );
}

#[test]
fn attendance_events_are_idempotent_state_bound_and_calculate_exact_worked_time() {
    let mut f = fixture();
    let context = bhaipos_store::LocalTerminalContext {
        tenant_id: f.tenant,
        branch_id: f.branch,
        device_id: f.device,
        register_id: f.register,
    };
    let employee_id = Uuid::new_v4();
    let create_operation = OperationId::new();
    let employee = f
        .store
        .create_employee(
            context,
            f.user,
            create_operation,
            employee_id,
            "EMP-100",
            "Store Assistant",
            Some("Shelf Staff"),
            Some("39000003"),
            Some("2026-09-01"),
            t("2026-09-29T05:59:00Z"),
        )
        .unwrap();
    assert_eq!(
        employee,
        f.store
            .create_employee(
                context,
                f.user,
                create_operation,
                employee_id,
                "EMP-100",
                "Store Assistant",
                Some("Shelf Staff"),
                Some("39000003"),
                Some("2026-09-01"),
                t("2026-09-29T05:59:00Z"),
            )
            .unwrap()
    );
    let clock_in_operation = OperationId::new();
    let clocked_in = f
        .store
        .record_attendance_event(
            context,
            f.user,
            clock_in_operation,
            employee_id,
            "CLOCK_IN",
            t("2026-09-29T06:00:00Z"),
            None,
        )
        .unwrap();
    assert_eq!(clocked_in.state, "CLOCKED_IN");
    assert_eq!(
        clocked_in,
        f.store
            .record_attendance_event(
                context,
                f.user,
                clock_in_operation,
                employee_id,
                "CLOCK_IN",
                t("2026-09-29T06:00:00Z"),
                None,
            )
            .unwrap()
    );
    assert!(matches!(
        f.store.record_attendance_event(
            context,
            f.user,
            OperationId::new(),
            employee_id,
            "CLOCK_OUT",
            t("2026-09-29T05:59:59Z"),
            None,
        ),
        Err(StoreError::Conflict(_)) | Err(StoreError::Validation(_))
    ));
    for (event_type, occurred_at) in [
        ("BREAK_START", "2026-09-29T08:00:00Z"),
        ("BREAK_END", "2026-09-29T08:30:00Z"),
    ] {
        f.store
            .record_attendance_event(
                context,
                f.user,
                OperationId::new(),
                employee_id,
                event_type,
                t(occurred_at),
                None,
            )
            .unwrap();
    }
    let closed = f
        .store
        .record_attendance_event(
            context,
            f.user,
            OperationId::new(),
            employee_id,
            "CLOCK_OUT",
            t("2026-09-29T11:00:00Z"),
            None,
        )
        .unwrap();
    assert_eq!(closed.state, "CLOCKED_OUT");
    assert_eq!(closed.break_seconds, 1_800);
    assert_eq!(closed.worked_seconds, Some(16_200));
    let report = f
        .store
        .attendance_report(
            context,
            f.user,
            employee_id,
            "2026-09-29T00:00:00Z",
            "2026-09-30T00:00:00Z",
        )
        .unwrap();
    assert_eq!(report.completed_sessions, 1);
    assert_eq!(report.worked_seconds, 16_200);
    assert_eq!(report.break_seconds, 1_800);
    assert_eq!(
        f.store
            .connection()
            .query_row(
                "SELECT COUNT(*) FROM attendance_session_events WHERE session_id=?1",
                params![closed.session_id.to_string()],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
        4
    );
}

#[test]
fn operational_alerts_are_idempotent_state_bound_and_append_evidenced() {
    let mut f = fixture();
    let context = bhaipos_store::LocalTerminalContext {
        tenant_id: f.tenant,
        branch_id: f.branch,
        device_id: f.device,
        register_id: f.register,
    };
    assert!(f
        .store
        .user_has_permission(context, f.user, "alert.view")
        .unwrap());
    assert!(!f
        .store
        .user_has_permission(context, f.user, "unknown.permission")
        .unwrap());
    let alert_id = Uuid::new_v4();
    let create_operation = OperationId::new();
    let created = f
        .store
        .create_operational_alert(
            context,
            f.user,
            create_operation,
            alert_id,
            "CRITICAL",
            "SECURITY_EVENT",
            "Repeated terminal authentication failures",
            Some("device"),
            Some(&f.device.to_string()),
            r#"{"failures":5}"#,
            t("2026-09-29T12:00:00Z"),
        )
        .unwrap();
    assert_eq!(created.status, "NEW");
    assert_eq!(
        created,
        f.store
            .create_operational_alert(
                context,
                f.user,
                create_operation,
                alert_id,
                "CRITICAL",
                "SECURITY_EVENT",
                "Repeated terminal authentication failures",
                Some("device"),
                Some(&f.device.to_string()),
                r#"{"failures":5}"#,
                t("2026-09-29T12:00:00Z"),
            )
            .unwrap()
    );
    assert!(matches!(
        f.store.create_operational_alert(
            context,
            f.user,
            create_operation,
            alert_id,
            "HIGH",
            "SECURITY_EVENT",
            "Changed replay",
            None,
            None,
            "{}",
            t("2026-09-29T12:00:00Z"),
        ),
        Err(StoreError::Conflict(_))
    ));
    assert!(matches!(
        f.store.transition_operational_alert(
            context,
            f.user,
            OperationId::new(),
            alert_id,
            "RESOLVED",
            None,
            Some("Cannot skip investigation"),
            t("2026-09-29T12:01:00Z"),
        ),
        Err(StoreError::Conflict(_))
    ));
    for (status, assignee, note, at) in [
        ("ACKNOWLEDGED", Some(f.user), None, "2026-09-29T12:02:00Z"),
        (
            "IN_PROGRESS",
            Some(f.user),
            Some("Investigating terminal logs"),
            "2026-09-29T12:03:00Z",
        ),
        (
            "RESOLVED",
            Some(f.user),
            Some("Credential rotated and terminal verified"),
            "2026-09-29T12:04:00Z",
        ),
    ] {
        f.store
            .transition_operational_alert(
                context,
                f.user,
                OperationId::new(),
                alert_id,
                status,
                assignee,
                note,
                t(at),
            )
            .unwrap();
    }
    assert!(matches!(
        f.store.transition_operational_alert(
            context,
            f.user,
            OperationId::new(),
            alert_id,
            "DISMISSED",
            None,
            Some("too late"),
            t("2026-09-29T12:05:00Z"),
        ),
        Err(StoreError::Conflict(_))
    ));
    assert!(f
        .store
        .active_operational_alerts(context, f.user, 50)
        .unwrap()
        .is_empty());
    assert_eq!(
        f.store
            .connection()
            .query_row(
                "SELECT COUNT(*) FROM operational_alert_events WHERE alert_id=?1",
                params![alert_id.to_string()],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
        4
    );
}

#[test]
fn operational_alert_evaluation_is_idempotent_scoped_and_evidence_driven() {
    let mut f = fixture();
    let context = bhaipos_store::LocalTerminalContext {
        tenant_id: f.tenant,
        branch_id: f.branch,
        device_id: f.device,
        register_id: f.register,
    };
    // At 22:00 UTC the Bahrain calendar date is already 2026-10-05.
    let now = t("2026-10-04T22:00:00Z");
    let second_device = DeviceId::new();
    f.store
        .create_device(
            second_device,
            f.tenant,
            f.branch,
            "POS-02",
            "second-device-secret-0123456789",
            t("2026-09-01T00:00:00Z"),
        )
        .unwrap();
    f.store
        .connection()
        .execute(
            "UPDATE devices SET last_heartbeat_at=?1 WHERE id=?2",
            params!["2026-10-04T15:00:00Z", second_device.to_string()],
        )
        .unwrap();
    f.store
        .connection()
        .execute(
            "INSERT INTO reorder_policies(tenant_id,branch_id,product_id,reorder_point_milli,target_stock_milli,active) VALUES(?1,?2,?3,12000,20000,1)",
            params![f.tenant.to_string(), f.branch.to_string(), f.product.to_string()],
        )
        .unwrap();
    let lot = Uuid::new_v4();
    f.store.connection().execute(
        "INSERT INTO inventory_lots(id,tenant_id,product_id,lot_number,expires_on,status,unit_cost_fils,received_at) VALUES(?1,?2,?3,'EXP-1','2026-10-06','ACTIVE',700,?4)",
        params![lot.to_string(),f.tenant.to_string(),f.product.to_string(),now.to_rfc3339()],
    ).unwrap();
    f.store.connection().execute(
        "INSERT INTO lot_balances(tenant_id,branch_id,centre_id,lot_id,quantity_milli) VALUES(?1,?2,?3,?4,1000)",
        params![f.tenant.to_string(),f.branch.to_string(),f.centre.to_string(),lot.to_string()],
    ).unwrap();
    f.store.connection().execute(
        "INSERT INTO sync_queue(id,tenant_id,branch_id,device_id,operation_id,entity_type,entity_id,mutation_type,payload_json,state,attempts,created_at,updated_at) VALUES(?1,?2,?3,?4,?5,'product',?6,'UPSERT','{}','RETRYING',2,'2026-10-04T14:00:00Z','2026-10-04T14:00:00Z')",
        params![Uuid::new_v4().to_string(),f.tenant.to_string(),f.branch.to_string(),f.device.to_string(),OperationId::new().to_string(),f.product.to_string()],
    ).unwrap();
    let supplier: String = f
        .store
        .connection()
        .query_row(
            "SELECT id FROM suppliers WHERE tenant_id=?1 LIMIT 1",
            params![f.tenant.to_string()],
            |row| row.get(0),
        )
        .unwrap();
    let invoice = Uuid::new_v4();
    f.store.connection().execute(
        "INSERT INTO supplier_invoices(id,tenant_id,branch_id,supplier_id,invoice_number,invoice_date,due_date,subtotal_fils,tax_fils,total_fils,amount_paid_fils,status,created_at) VALUES(?1,?2,?3,?4,'OVERDUE-1','2026-09-01','2026-10-04',10000,1000,11000,0,'OPEN',?5)",
        params![invoice.to_string(),f.tenant.to_string(),f.branch.to_string(),supplier,now.to_rfc3339()],
    ).unwrap();
    let backup = Uuid::new_v4();
    f.store.connection().execute(
        "INSERT INTO backup_records(id,tenant_id,branch_id,backup_type,storage_path,sha256,schema_version,app_version,state,integrity_state,created_at) VALUES(?1,?2,?3,'SCHEDULED','backup.db',?4,'0018','0.1.0','FAILED','FAILED',?5)",
        params![backup.to_string(),f.tenant.to_string(),f.branch.to_string(),"0".repeat(64),now.to_rfc3339()],
    ).unwrap();
    f.store
        .connection()
        .execute(
            "UPDATE users SET failed_attempts=4 WHERE id=?1",
            params![f.user.to_string()],
        )
        .unwrap();

    let policy = bhaipos_store::OperationalAlertEvaluationPolicy {
        sync_delay_minutes: 60,
        terminal_offline_minutes: 60,
        expiry_warning_days: 7,
        authentication_failure_threshold: 3,
    };
    let operation_id = OperationId::new();
    let first = f
        .store
        .evaluate_operational_alerts(context, f.user, operation_id, policy, now)
        .unwrap();
    assert_eq!(first.created_alerts, 7);
    assert_eq!(first.existing_alerts, 0);
    assert_eq!(
        first,
        f.store
            .evaluate_operational_alerts(
                context,
                f.user,
                operation_id,
                policy,
                now + chrono::Duration::minutes(5),
            )
            .unwrap()
    );
    let second = f
        .store
        .evaluate_operational_alerts(
            context,
            f.user,
            OperationId::new(),
            policy,
            now + chrono::Duration::minutes(5),
        )
        .unwrap();
    assert_eq!(second.created_alerts, 0);
    assert_eq!(second.existing_alerts, 7);
    assert_eq!(
        f.store.connection().query_row(
            "SELECT COUNT(*) FROM operational_alerts WHERE tenant_id=?1 AND branch_id=?2 AND alert_type IN ('LOW_STOCK','EXPIRY','SYNC_DELAY','TERMINAL_OFFLINE','OVERDUE_SUPPLIER_INVOICE','BACKUP_FAILURE','SECURITY_EVENT')",
            params![f.tenant.to_string(),f.branch.to_string()],
            |row| row.get::<_,i64>(0),
        ).unwrap(),
        7
    );
}

#[test]
fn background_jobs_are_payload_bound_leased_cancellable_and_recoverable() {
    let mut f = fixture();
    let context = bhaipos_store::LocalTerminalContext {
        tenant_id: f.tenant,
        branch_id: f.branch,
        device_id: f.device,
        register_id: f.register,
    };
    let now = t("2026-10-04T12:00:00Z");
    let enqueue_operation = OperationId::new();
    let request = BackgroundJobEnqueueRequest {
        context,
        user_id: f.user,
        operation_id: enqueue_operation,
        job_type: "CATALOGUE_IMPORT".into(),
        payload_json: serde_json::json!({"source":"catalogue.csv"}).to_string(),
        progress_total: Some(10),
        cancellable: true,
        max_attempts: 2,
        not_before: None,
        now,
    };
    let queued = f.store.enqueue_background_job(request.clone()).unwrap();
    assert_eq!(queued.state, "QUEUED");
    assert_eq!(
        queued,
        f.store.enqueue_background_job(request.clone()).unwrap()
    );
    let mut changed = request;
    changed.payload_json = serde_json::json!({"source":"different.csv"}).to_string();
    assert!(matches!(
        f.store.enqueue_background_job(changed),
        Err(StoreError::Conflict(_))
    ));

    let claim_operation = OperationId::new();
    let lease = f
        .store
        .claim_next_background_job(context, f.user, claim_operation, 120, now)
        .unwrap()
        .unwrap();
    assert_eq!(lease.job_id, queued.job_id);
    assert_eq!(lease.attempt, 1);
    assert_eq!(
        Some(lease.clone()),
        f.store
            .claim_next_background_job(context, f.user, claim_operation, 120, now)
            .unwrap()
    );
    let heartbeat_operation = OperationId::new();
    let progress = f
        .store
        .heartbeat_background_job(
            context,
            f.user,
            heartbeat_operation,
            lease.job_id,
            lease.lease_token,
            4,
            Some(10),
            120,
            now + chrono::Duration::seconds(30),
        )
        .unwrap();
    assert_eq!(progress.progress_current, 4);
    assert!(!progress.cancel_requested);
    assert_eq!(
        progress,
        f.store
            .heartbeat_background_job(
                context,
                f.user,
                heartbeat_operation,
                lease.job_id,
                lease.lease_token,
                4,
                Some(10),
                120,
                now + chrono::Duration::seconds(31),
            )
            .unwrap()
    );
    let cancel_operation = OperationId::new();
    let cancel = f
        .store
        .request_background_job_cancellation(
            context,
            f.user,
            cancel_operation,
            lease.job_id,
            "operator stopped the import",
            now + chrono::Duration::seconds(35),
        )
        .unwrap();
    assert_eq!(cancel.state, "RUNNING");
    assert!(cancel.cancel_requested);
    assert_eq!(
        cancel,
        f.store
            .request_background_job_cancellation(
                context,
                f.user,
                cancel_operation,
                lease.job_id,
                "operator stopped the import",
                now + chrono::Duration::seconds(35),
            )
            .unwrap()
    );
    let cancelled_finish_operation = OperationId::new();
    let cancelled_outcome = BackgroundJobFinishOutcome::Cancelled {
        result_json: Some(serde_json::json!({"rows_applied":4}).to_string()),
    };
    let acknowledged = f
        .store
        .finish_background_job(
            context,
            f.user,
            cancelled_finish_operation,
            lease.job_id,
            lease.lease_token,
            cancelled_outcome.clone(),
            now + chrono::Duration::seconds(40),
        )
        .unwrap();
    assert_eq!(acknowledged.state, "CANCELLED");
    assert_eq!(
        acknowledged,
        f.store
            .finish_background_job(
                context,
                f.user,
                cancelled_finish_operation,
                lease.job_id,
                lease.lease_token,
                cancelled_outcome,
                now + chrono::Duration::seconds(50),
            )
            .unwrap()
    );
    assert!(matches!(
        f.store.finish_background_job(
            context,
            f.user,
            OperationId::new(),
            lease.job_id,
            lease.lease_token,
            BackgroundJobFinishOutcome::Succeeded {
                result_json: serde_json::json!({"incorrect":true}).to_string(),
            },
            now + chrono::Duration::seconds(41),
        ),
        Err(StoreError::Conflict(_))
    ));

    let retry_job = f
        .store
        .enqueue_background_job(BackgroundJobEnqueueRequest {
            context,
            user_id: f.user,
            operation_id: OperationId::new(),
            job_type: "BACKUP".into(),
            payload_json: "{}".into(),
            progress_total: None,
            cancellable: false,
            max_attempts: 2,
            not_before: None,
            now: now + chrono::Duration::minutes(1),
        })
        .unwrap();
    let retry_lease = f
        .store
        .claim_next_background_job(
            context,
            f.user,
            OperationId::new(),
            60,
            now + chrono::Duration::minutes(1),
        )
        .unwrap()
        .unwrap();
    assert_eq!(retry_lease.job_id, retry_job.job_id);
    let retry_finish_operation = OperationId::new();
    let retry_outcome = BackgroundJobFinishOutcome::Failed {
        error: "temporary I/O failure".into(),
        retryable: true,
    };
    let retry = f
        .store
        .finish_background_job(
            context,
            f.user,
            retry_finish_operation,
            retry_lease.job_id,
            retry_lease.lease_token,
            retry_outcome.clone(),
            now + chrono::Duration::seconds(61),
        )
        .unwrap();
    assert_eq!(retry.state, "QUEUED");
    assert_eq!(
        retry,
        f.store
            .finish_background_job(
                context,
                f.user,
                retry_finish_operation,
                retry_lease.job_id,
                retry_lease.lease_token,
                retry_outcome,
                now + chrono::Duration::seconds(62),
            )
            .unwrap()
    );
    let empty_claim_operation = OperationId::new();
    assert!(f
        .store
        .claim_next_background_job(
            context,
            f.user,
            empty_claim_operation,
            60,
            now + chrono::Duration::seconds(62),
        )
        .unwrap()
        .is_none());
    assert!(f
        .store
        .claim_next_background_job(
            context,
            f.user,
            empty_claim_operation,
            60,
            now + chrono::Duration::seconds(64),
        )
        .unwrap()
        .is_none());
    let final_lease = f
        .store
        .claim_next_background_job(
            context,
            f.user,
            OperationId::new(),
            60,
            now + chrono::Duration::seconds(64),
        )
        .unwrap()
        .unwrap();
    assert_eq!(final_lease.attempt, 2);
    let recovery_operation = OperationId::new();
    let recovery = f
        .store
        .recover_expired_background_jobs(
            context,
            f.user,
            recovery_operation,
            now + chrono::Duration::seconds(125),
        )
        .unwrap();
    assert_eq!(recovery.requeued, 0);
    assert_eq!(recovery.failed, 1);
    assert_eq!(recovery.cancelled, 0);
    assert_eq!(
        recovery,
        f.store
            .recover_expired_background_jobs(
                context,
                f.user,
                recovery_operation,
                now + chrono::Duration::minutes(10),
            )
            .unwrap()
    );
    let jobs = f.store.background_jobs(context, f.user, 20).unwrap();
    assert_eq!(
        jobs.iter()
            .find(|job| job.job_id == retry_job.job_id)
            .unwrap()
            .state,
        "FAILED"
    );
    assert_eq!(
        f.store
            .connection()
            .query_row(
                "SELECT COUNT(*) FROM background_job_events WHERE tenant_id=?1",
                params![f.tenant.to_string()],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
        10
    );
}
