#!/usr/bin/env python3
from __future__ import annotations
import json, re, sqlite3, uuid
from pathlib import Path

ROOT=Path(__file__).resolve().parents[1]
MIGRATIONS=[ROOT/'migrations/0001_core.sql',ROOT/'migrations/0002_retail_os.sql',ROOT/'migrations/0003_integrity_guards.sql']
UPGRADE_0004=ROOT/'migrations/0004_cash_eod.sql'
UPGRADE_0005=ROOT/'migrations/0005_idempotency_payload_binding.sql'
UPGRADE_0006=ROOT/'migrations/0006_financial_domain_guards.sql'
UPGRADE_0007=ROOT/'migrations/0007_local_terminal_binding.sql'
UPGRADE_0008=ROOT/'migrations/0008_receipt_reprint_permission.sql'
UPGRADE_0009=ROOT/'migrations/0009_print_leases_and_profiles.sql'
UPGRADE_0010=ROOT/'migrations/0010_sync_protocol.sql'
UPGRADE_0011=ROOT/'migrations/0011_inventory_operations.sql'
UPGRADE_0012=ROOT/'migrations/0012_procurement_supplier_finance.sql'
UPGRADE_0013=ROOT/'migrations/0013_customer_store_operations.sql'
UPGRADE_0014=ROOT/'migrations/0014_production_operations.sql'

def uid(): return str(uuid.uuid4())
def must_fail(fn, contains=None):
    try: fn()
    except sqlite3.DatabaseError as e:
        if contains and contains not in str(e): raise AssertionError(f"expected {contains!r}, got {e!r}")
        return
    raise AssertionError('expected database operation to fail')

def apply_schema(con, include_payload_binding=True):
    con.execute('PRAGMA foreign_keys=ON')
    for p in MIGRATIONS: con.executescript(p.read_text())
    cols={r[1] for r in con.execute('PRAGMA table_info(refund_payments)')}
    upgrades={
        'cash_session_id':'ALTER TABLE refund_payments ADD COLUMN cash_session_id TEXT REFERENCES cash_sessions(id)',
        'device_id':'ALTER TABLE refund_payments ADD COLUMN device_id TEXT REFERENCES devices(id)',
        'user_id':'ALTER TABLE refund_payments ADD COLUMN user_id TEXT REFERENCES users(id)',
    }
    for col,ddl in upgrades.items():
        if col not in cols: con.execute(ddl)
    con.executescript(UPGRADE_0004.read_text())
    if include_payload_binding:
        con.executescript(UPGRADE_0005.read_text())
        con.executescript(UPGRADE_0006.read_text())
        con.executescript(UPGRADE_0007.read_text())
        con.executescript(UPGRADE_0008.read_text())
        con.executescript(UPGRADE_0009.read_text())
        con.executescript(UPGRADE_0010.read_text())
        con.executescript(UPGRADE_0011.read_text())
        con.executescript(UPGRADE_0012.read_text())
        con.executescript(UPGRADE_0013.read_text())
        con.executescript(UPGRADE_0014.read_text())

con=sqlite3.connect(':memory:')
apply_schema(con)
# Re-running the migration path must be safe on an existing database.
apply_schema(con)

# Permission data upgrades must unlock existing owner roles, not only newly
# bootstrapped businesses.
upgrade_tenant,upgrade_role=uid(),uid()
con.execute("insert into tenants(id,name,created_at) values(?,?,?)",(upgrade_tenant,'Upgrade Test','2026-09-29T01:00:00+00:00'))
con.execute("insert into roles(id,tenant_id,name) values(?,?,?)",(upgrade_role,upgrade_tenant,'owner'))
con.executescript(UPGRADE_0008.read_text())
assert con.execute("select 1 from role_permissions where role_id=? and permission_code='receipt.reprint'",(upgrade_role,)).fetchone()

# The additive binding migration must preserve historical unbound results.
# Runtime replay of these rows fails closed because their request payload is
# unknowable, but upgrade itself never deletes or fabricates evidence.
legacy=sqlite3.connect(':memory:')
apply_schema(legacy, include_payload_binding=False)
legacy_tenant,legacy_operation=uid(),uid()
legacy.execute("insert into tenants(id,name,created_at) values(?,?,?)",(legacy_tenant,'Legacy','2026-09-29T01:00:00+00:00'))
legacy.execute(
    "insert into idempotency_results(tenant_id,operation_id,action,result_json,committed_at) values(?,?,?,?,?)",
    (legacy_tenant,legacy_operation,'CHECKOUT','{}','2026-09-29T01:00:00+00:00'),
)
legacy.executescript(UPGRADE_0005.read_text())
assert legacy.execute(
    "select count(*) from idempotency_results where tenant_id=? and operation_id=?",
    (legacy_tenant,legacy_operation),
).fetchone()[0]==1
legacy.close()

# Structural checks.
tables={r[0] for r in con.execute("select name from sqlite_master where type='table' and name not like 'sqlite_%'")}
triggers={r[0] for r in con.execute("select name from sqlite_master where type='trigger'")}
critical={
    'tenants','branches','users','roles','permissions','devices','registers','local_terminal_binding','cash_sessions','products','product_barcodes',
    'branch_assortments','price_history','carts','cart_lines','sales','sale_lines','sale_payments','receipt_snapshots',
    'inventory_movements','stock_levels','refunds','refund_lines','idempotency_operations','idempotency_results','sync_queue','audit_events',
    'suppliers','purchase_orders','goods_receipts','supplier_invoices','supplier_payments','supplier_ledger','expenses',
    'refund_payments','cash_variance_cases','customers','loyalty_ledger','customer_credit_ledger','delivery_orders','digital_orders','production_orders','waste_events',
    'whatsapp_messages','payment_evidence','ai_action_runs','operational_alerts','background_jobs','backup_records','print_jobs','print_job_leases','printer_profiles',
    'sync_delivery_leases','hub_mutations','device_sync_checkpoints','sync_conflict_resolutions','sync_delivery_errors','sync_retry_schedule',
    'device_enrollment_grants','device_enrollment_consumptions','inventory_cost_balances','inventory_movement_lots',
    'inventory_operation_results','inventory_reconciliation_runs','inventory_transfer_receipts','inventory_transfer_receipt_lines','stocktake_count_events','stocktake_approvals',
    'procurement_operation_results','purchase_order_events','supplier_invoice_events','supplier_return_events','store_operation_results','customer_credit_payment_allocations','production_events'
}
missing=critical-tables
assert not missing, f'missing critical tables: {sorted(missing)}'
assert len(tables) >= 115, len(tables)
assert len(triggers) >= 50, len(triggers)
assert con.execute('PRAGMA foreign_key_check').fetchall()==[]

# Every financial *_fils column is INTEGER-affinity and there are no real/float financial columns.
financial_cols=[]
float_cols=[]
for table in tables:
    for _,name,ctype,*_ in con.execute(f'PRAGMA table_info("{table}")'):
        ctype=(ctype or '').upper()
        if name.endswith('_fils'):
            financial_cols.append((table,name,ctype))
            assert 'INT' in ctype, (table,name,ctype)
        if any(x in ctype for x in ('REAL','FLOAT','DOUBLE')): float_cols.append((table,name,ctype))
assert financial_cols and not float_cols, float_cols

# Curated tenant-bearing authoritative tables must explicitly carry tenant_id.
for table in ['devices','registers','cash_sessions','products','product_barcodes','branch_assortments','price_history','stock_levels','inventory_movements','carts','sales','refunds','sync_queue','audit_events','supplier_payments','customer_credit_payments','operational_alerts','ai_action_runs','print_jobs']:
    cols={r[1] for r in con.execute(f'PRAGMA table_info("{table}")')}
    assert 'tenant_id' in cols, table

# Tenant guard evidence.
now='2026-09-29T01:00:00+00:00'; ta,tb=uid(),uid(); ba,bb=uid(),uid()
con.execute("insert into tenants(id,name,created_at) values(?,?,?)",(ta,'A',now))
con.execute("insert into tenants(id,name,created_at) values(?,?,?)",(tb,'B',now))
con.execute("insert into branches(id,tenant_id,code,name,created_at) values(?,?,?,?,?)",(ba,ta,'A','A',now))
con.execute("insert into branches(id,tenant_id,code,name,created_at) values(?,?,?,?,?)",(bb,tb,'B','B',now))
must_fail(lambda: con.execute("insert into devices(id,tenant_id,branch_id,label,status,credential_hash,created_at) values(?,?,?,?,?,?,?)",(uid(),ta,bb,'bad','ACTIVE','x',now)), 'TENANT_SCOPE_VIOLATION')

# Create coherent A scope and prove barcode isolation and immutable audit.
user,device,product=uid(),uid(),uid()
con.execute("insert into users(id,tenant_id,display_name,pin_hash,status,created_at) values(?,?,?,?,?,?)",(user,ta,'U','hash','ACTIVE',now))
con.execute("insert into devices(id,tenant_id,branch_id,label,status,credential_hash,created_at) values(?,?,?,?,?,?,?)",(device,ta,ba,'D','ACTIVE','hash',now))
register=uid(); con.execute("insert into registers(id,tenant_id,branch_id,code,name) values(?,?,?,?,?)",(register,ta,ba,'R1','Register 1'))
con.execute("insert into local_terminal_binding(singleton,tenant_id,branch_id,device_id,register_id,installed_at) values(1,?,?,?,?,?)",(ta,ba,device,register,now))
must_fail(lambda: con.execute("update local_terminal_binding set device_id=? where singleton=1",(uid(),)), 'IMMUTABLE')
con.executescript(UPGRADE_0009.read_text())
assert con.execute("select paper_width_mm,characters_per_line,drawer_pulse_policy from printer_profiles where device_id=?",(device,)).fetchone()==(80,48,'CASH_SALE')
must_fail(lambda: con.execute("insert into printer_profiles(id,tenant_id,branch_id,device_id,friendly_name,transport,paper_width_mm,characters_per_line,character_encoding,cut_mode,drawer_pulse_policy,created_at,updated_at) values(?,?,?,?,?,'WINDOWS_SPOOLER',80,48,'ASCII','PARTIAL','NEVER',?,?)",(uid(),tb,bb,device,'bad',now,now)), 'TENANT_SCOPE_VIOLATION')
con.execute("insert into products(id,tenant_id,sku,name,base_price_fils,current_cost_fils,created_at) values(?,?,?,?,?,?,?)",(product,ta,'S1','P',1000,700,now))
con.execute("insert into product_barcodes(tenant_id,barcode,product_id,created_at) values(?,?,?,?)",(ta,'123',product,now))
# Same literal barcode can exist in another tenant without collision.
product_b=uid(); con.execute("insert into products(id,tenant_id,sku,name,base_price_fils,current_cost_fils,created_at) values(?,?,?,?,?,?,?)",(product_b,tb,'S1','PB',1000,700,now)); con.execute("insert into product_barcodes(tenant_id,barcode,product_id,created_at) values(?,?,?,?)",(tb,'123',product_b,now))
assert con.execute("select count(*) from product_barcodes where barcode='123'").fetchone()[0]==2

audit=uid(); con.execute("insert into audit_events(id,tenant_id,device_id,actor_user_id,event_type,entity_type,entity_id,payload_json,previous_hash,event_hash,created_at) values(?,?,?,?,?,?,?,?,?,?,?)",(audit,ta,device,user,'TEST','x','1','{}','','abc',now))
must_fail(lambda: con.execute("update audit_events set payload_json='tampered' where id=?",(audit,)), 'IMMUTABLE')

# SQLite's dynamic typing must not admit floating financial values or invalid
# tax configurations even when writes bypass the application layer.
must_fail(
    lambda: con.execute(
        "insert into products(id,tenant_id,sku,name,base_price_fils,current_cost_fils,created_at) values(?,?,?,?,?,?,?)",
        (uid(),ta,'FLOAT','Float price',1.5,1,now),
    ),
    'INVALID_FINANCIAL_DOMAIN',
)
must_fail(lambda: con.execute("update products set tax_rate_bps=-10000 where id=?",(product,)), 'INVALID_FINANCIAL_DOMAIN')
must_fail(lambda: con.execute("update products set tax_category='EXEMPT',tax_rate_bps=1000 where id=?",(product,)), 'INVALID_FINANCIAL_DOMAIN')

# Idempotency evidence must be payload-bound, globally unique per tenant
# operation ID, and immutable. Results cannot exist without a matching binding.
operation=uid()
must_fail(
    lambda: con.execute(
        "insert into idempotency_results(tenant_id,operation_id,action,result_json,committed_at) values(?,?,?,?,?)",
        (ta,operation,'CHECKOUT','{}',now),
    ),
    'IDEMPOTENCY_BINDING_REQUIRED',
)
con.execute(
    "insert into idempotency_operations(tenant_id,operation_id,action,request_sha256,created_at) values(?,?,?,?,?)",
    (ta,operation,'CHECKOUT','a'*64,now),
)
con.execute(
    "insert into idempotency_results(tenant_id,operation_id,action,result_json,committed_at) values(?,?,?,?,?)",
    (ta,operation,'CHECKOUT','{}',now),
)
must_fail(
    lambda: con.execute(
        "insert into idempotency_operations(tenant_id,operation_id,action,request_sha256,created_at) values(?,?,?,?,?)",
        (ta,operation,'REFUND','b'*64,now),
    ),
    'UNIQUE',
)
must_fail(
    lambda: con.execute(
        "update idempotency_operations set request_sha256=? where tenant_id=? and operation_id=?",
        ('b'*64,ta,operation),
    ),
    'IMMUTABLE',
)

# Source policy scans: authoritative Rust money/store cannot contain floating money types.
rust_authoritative=(ROOT/'crates/bhaipos-core/src/money.rs').read_text()+'\n'+(ROOT/'crates/bhaipos-core/src/tax.rs').read_text()+'\n'+(ROOT/'crates/bhaipos-store/src/store.rs').read_text()
assert not re.search(r'\bf(?:32|64)\b', rust_authoritative), 'floating type found in authoritative financial source'
assert 'transaction_with_behavior(TransactionBehavior::Immediate)' in rust_authoritative
assert 'idempotency_results' in rust_authoritative
assert 'idempotency_operations' in rust_authoritative
assert 'different action or payload' in rust_authoritative
assert 'RECORDED_NOT_SETTLED' in rust_authoritative
assert 'manager_approval_consumptions' in rust_authoritative
assert 'cash_session_report' in rust_authoritative
assert 'CASH_SESSION_CLOSE' in rust_authoritative
assert 'refund_payments' in rust_authoritative
assert re.search(r'subtotal\s*=\s*subtotal\.checked_add', rust_authoritative)
assert 'audit chain topology is inconsistent' in rust_authoritative
assert 'ORDER BY created_at DESC,id DESC' not in rust_authoritative
assert 'bootstrap_local_business' in rust_authoritative and 'local_terminal_context' in rust_authoritative

# UI contract static checks.
css=(ROOT/'apps/desktop/src/styles.css').read_text()
assert 'min-width:1024px' in css and 'min-height:768px' in css
assert 'min-height:48px' in css
prototype=(ROOT/'prototype/index.html').read_text()
assert 'BHAIPOS' in prototype and 'BenefitPay' in prototype and 'Hold Cart' in prototype

# The renderer receives a narrow typed command surface. Authority is resolved
# from trusted desktop state, never from tenant/branch/device/user request fields.
desktop_bridge=(ROOT/'apps/desktop/src-tauri/src/main.rs').read_text()
required_commands={
    'health','bootstrap_local_business','login','logout','rotate_local_device_credential','open_cash_session','create_cart',
    'scan_barcode','checkout',
    'requeue_failed_print_job','refund','record_cash_movement','cash_session_report',
    'close_cash_session',
    'cart_snapshot','hold_cart','list_held_carts','restore_cart',
    'find_refundable_sale','quote_refund','list_failed_print_jobs',
}
registered=re.search(r'tauri::generate_handler!\[([^]]+)\]',desktop_bridge,re.S)
assert registered, 'desktop command allow-list missing'
registered_commands={name.strip() for name in registered.group(1).split(',') if name.strip()}
assert required_commands<=registered_commands, sorted(required_commands-registered_commands)
assert 'struct AuthenticatedSession' in desktop_bridge and 'fn require_session' in desktop_bridge
assert 'validate_local_session' in desktop_bridge
assert re.search(
    r'pub const LATEST_SCHEMA\s*:\s*&str\s*=\s*"0014_production_operations"',
    rust_authoritative,
)
assert 'schema:bhaipos_store::LATEST_SCHEMA' in desktop_bridge.replace(' ','')
for request_name,body in re.findall(r'struct\s+(\w*Request)\s*\{([^}]*)\}',desktop_bridge,re.S):
    forbidden=re.findall(r'\b(?:tenant_id|branch_id|device_id|user_id|actor_user_id)\s*:',body)
    assert not forbidden, (request_name,forbidden)
ci_workflow=(ROOT/'.github/workflows/core.yml').read_text()
assert 'cargo check -p bhaipos-desktop' in ci_workflow
assert 'npm run typecheck' in ci_workflow and 'npm run build' in ci_workflow

cashier_ui=(ROOT/'apps/desktop/src/App.tsx').read_text()
pos_api_path=ROOT/'apps/desktop/src/api/pos.ts'
assert pos_api_path.exists(), 'typed POS IPC client missing'
pos_api=pos_api_path.read_text()
assert "from '@tauri-apps/api/core'" in pos_api
for command in ['create_cart','cart_snapshot','scan_barcode','hold_cart','list_held_carts','restore_cart','checkout','find_refundable_sale','quote_refund','refund','list_failed_print_jobs','requeue_failed_print_job','rotate_local_device_credential','logout']:
    assert f"'{command}'" in pos_api, command
assert 'const seed' not in cashier_ui
assert '.reduce(' not in cashier_ui
assert 'tenantId' not in pos_api and 'branchId' not in pos_api and 'deviceId' not in pos_api and 'userId' not in pos_api
assert 'useBarcodeFocus' in cashier_ui
assert 'onClick={showRefund}' in cashier_ui and 'onClick={showPrintRecovery}' in cashier_ui
assert 'recover_stale_print_jobs' in rust_authoritative
assert 'lease_token' in rust_authoritative and 'receipt_text' in rust_authoritative
assert (ROOT/'apps/desktop/src-tauri/src/printer.rs').exists()
assert "claim_next_print_job" not in registered_commands and "complete_print_job" not in registered_commands
assert 'print_worker_cycle' in desktop_bridge
for symbol in ['claim_sync_batch','complete_sync_delivery','accept_hub_mutation','resolve_sync_conflict','SyncMutationEnvelope','issue_device_enrollment','activate_device_enrollment']:
    assert symbol in rust_authoritative, symbol
for symbol in ['rebuild_stock_cache','receive_inventory','dispatch_transfer','receive_transfer','approve_stocktake','record_waste','inventory_valuation']:
    assert symbol in rust_authoritative, symbol
procurement=(ROOT/'crates/bhaipos-store/src/procurement.rs').read_text()
for symbol in ['create_purchase_requisition','create_purchase_order','progress_purchase_order','receive_purchase_order','post_supplier_invoice','pay_supplier','supplier_statement','dispatch_supplier_return','settle_supplier_return']:
    assert symbol in procurement, symbol
customer_ops=(ROOT/'crates/bhaipos-store/src/customer_ops.rs').read_text()
for symbol in ['create_customer','associate_cart_customer','record_loyalty_event','set_customer_credit_account','receive_customer_credit_payment','customer_credit_balance']:
    assert symbol in customer_ops, symbol
assert 'customer_credit_ledger' in rust_authoritative and 'customer credit limit exceeded' in rust_authoritative
production=(ROOT/'crates/bhaipos-store/src/production.rs').read_text()
for symbol in ['create_recipe','create_production_order','complete_production']:
    assert symbol in production, symbol
credential_store=(ROOT/'apps/desktop/src-tauri/src/credential_store.rs').read_text()
assert 'keyring::Entry' in credential_store and 'write_device_secret' in desktop_bridge
assert 'device_credential_secret' not in pos_api

result={
    'schema_tables': len(tables),
    'integrity_triggers': len(triggers),
    'financial_integer_columns': len(financial_cols),
    'foreign_key_check': 'ok',
    'tenant_guard_test': 'ok',
    'immutability_test': 'ok',
    'idempotency_payload_binding': 'ok',
    'cross_tenant_barcode_isolation': 'ok',
    'authoritative_float_scan': 'ok',
    'ui_1024x768_contract_scan': 'ok',
    'desktop_authority_boundary_scan': 'ok',
    'cashier_ipc_contract_scan': 'ok',
    'migration_reopen_idempotency': 'ok',
    'legacy_idempotency_preservation': 'ok',
    'financial_domain_guards': 'ok',
    'audit_chain_topology_policy': 'ok',
    'local_terminal_binding': 'ok',
}
print(json.dumps(result,indent=2,sort_keys=True))
