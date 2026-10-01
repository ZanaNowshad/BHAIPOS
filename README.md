# BHAIPOS

BHAIPOS is a Bahrain-native, offline-first, multi-terminal retail POS and retail operating-system codebase.

This repository is intentionally built around the transaction/security invariants first: tenant isolation, trusted devices, fail-closed RBAC, integer-fils money, deterministic tax, persistent carts, idempotent/atomic checkout, immutable transaction snapshots, append-only inventory/audit evidence, and durable sync intent. Administrative modules are then layered on the same identity and ledger model.

## Current implementation state

This is a **foundation build, not a finished production release**.

Implemented in source:

- Rust domain core for BHD `Money(i64)` and thousandth-unit quantities; no floating-point authoritative money.
- Deterministic inclusive/exclusive tax calculation using validated basis points.
- Argon2id PIN hashing/verification and lockout state.
- Argon2id device-secret hashing, device authentication, credential version rotation, suspension/revocation lifecycle.
- HMAC-SHA256 manager approvals bound to tenant, approver, device, operation, action, entity, payload hash, nonce, and expiry; consumed nonces are single-use.
- Fail-closed RBAC checks in checkout, refund, sale void, cash-session opening/closing, and cash movements.
- EAN/PLU/weighted-barcode primitives and unknown-barcode capture.
- SQLite schema for the complete retail operating model (121 tables).
- Cross-tenant, financial-domain and immutable-ledger guards (194 triggers).
- Persistent carts and held-cart state.
- Server-authoritative checkout recomputation.
- Exact split-tender validation, cash tender/change, BenefitPay `RECORDED_NOT_SETTLED` evidence boundary.
- Payload-bound idempotent checkout, refund, cash movement, cash-session close, and sale-void results; changed payloads and cross-action operation-ID reuse fail closed.
- `BEGIN IMMEDIATE` atomic sale commit across sale, lines, payments, inventory, receipt snapshot, print queue, audit and sync queue.
- Historical receipt snapshots with SHA-256 hashes.
- Append-only inventory movement ledger and stock cache updates.
- Partial refund quantity limits, explicit tender reversal effects, cash-session attribution, and stock compensation.
- Cash movements (paid in/out, safe drop, no-sale), X/Z/EOD cash-session reporting, expected-vs-counted variance, and automatic variance-case creation.
- Compensating sale voids with exact manager-approval binding, inventory restoration, tender reversal evidence, preserved original receipt, and idempotent replay.
- Durable print queue creation plus claim/fail/requeue/retry/printed state transitions with attempt counting; print failure never recreates the sale.
- SHA-256 audit-chain verification by stored hash topology, fail-closed append on chain damage, and DB immutability triggers.
- Business-date handling for Bahrain late-night operation.
- React/Tauri desktop shell source with separate Cashier and Admin modes and a 1024×768 touch-first contract.
- Explicit Tauri command allow-list for bootstrap, offline PIN login, cash session, cart scan, checkout, refund, cash movement/report and print recovery; renderer requests cannot provide tenant/branch/device/user authority.
- Immutable local terminal binding plus per-command user/device revalidation; login recovers the same cashier's open local cash session after restart.
- CI definition for `rustfmt`, Clippy `-D warnings`, and all Rust tests.

Schema/model coverage exists for pricing policies, promotions/coupons, bundles, lots/expiry, transfers, stocktakes, replenishment, suppliers, purchase requisitions/POs/receiving/AP, expenses/petty cash, customers/loyalty/credit, delivery/courier custody, channels/marketplace settlement, recipes/production/waste, employees/attendance, WhatsApp/OCR evidence, AI action safety/undo metadata, documents, alerts, jobs, backups/restores, diagnostics, updates, voids, print jobs, and feature flags.

The implementation status and the unimplemented runtime surfaces are listed precisely in `docs/IMPLEMENTATION_STATUS.md`.

## Repository layout

```text
BHAIPOS/
├── crates/
│   ├── bhaipos-core/       # money, tax, auth, approvals, barcodes, audit primitives
│   └── bhaipos-store/      # SQLite-backed authoritative transaction service
├── migrations/
│   ├── 0001_core.sql
│   ├── 0002_retail_os.sql
│   ├── 0003_integrity_guards.sql
│   ├── 0004_cash_eod.sql
│   ├── 0005_idempotency_payload_binding.sql
│   ├── 0006_financial_domain_guards.sql
│   ├── 0007_local_terminal_binding.sql
│   ├── 0008_receipt_reprint_permission.sql
│   ├── 0009_print_leases_and_profiles.sql
│   └── 0010_sync_protocol.sql
├── apps/desktop/
│   ├── src/                # React cashier/admin shell
│   └── src-tauri/          # restricted Tauri bootstrap
├── prototype/              # dependency-free cashier/admin/shift UI previews
├── scripts/verify_foundation.py
└── .github/workflows/core.yml
```

## Local verification

The environment used to generate this foundation did not contain Cargo and could not resolve external package hosts. Therefore **Rust compilation is not claimed as locally verified**.

The verification that did execute locally is:

```bash
python scripts/verify_foundation.py
```

Expected checks include schema loading, foreign-key validation, tenant-guard rejection, immutable-audit rejection, integer-only financial columns, cross-tenant barcode isolation, authoritative Rust float scan, and UI size contract.

On a machine with Rust installed, the release gate starts with:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-targets
```

For the desktop UI:

```bash
cd apps/desktop
npm install
npm run typecheck
npm run build
```

For the Tauri desktop application, install the normal Tauri 2 Windows build prerequisites and run the Tauri build from `apps/desktop`.

## Non-negotiable transaction rules

1. `tenant_id` is authoritative and checked in commands; core tables also have DB tenant guards.
2. Money is integer fils; aggregation, change and variance use checked arithmetic, and SQLite guards reject dynamic-type bypass. UI totals are advisory.
3. Quantities are integer thousandths; weighed goods never require floating-point authoritative arithmetic.
4. Financial commands require an active trusted device and explicit permission.
5. Checkout/refund/void/cash-movement/session-close operation IDs are replay-safe and bound to canonical request digests; reuse with a different payload or action is rejected.
6. Sale lines, payments, receipt snapshots, refunds, inventory movements, audit events, and ledgers are immutable; corrections use compensating records.
7. BenefitPay screenshots/OCR are evidence only unless a bank/provider API confirms settlement.
8. Printing, WhatsApp, OCR, AI, and network availability cannot define whether a financial commit succeeded.
9. Historical receipts never recalculate from the current catalogue.
10. Unsafe sync conflicts must become review work, not infinite retries or silent overwrites.

## Release policy

Do not label BHAIPOS production-ready until the gates in `docs/IMPLEMENTATION_STATUS.md` are all satisfied, including Windows clean-install acceptance, offline sale/refund/cashup, printer failure injection, multi-terminal lost-response replay, backup/restore, upgrade preservation, security checks, signing provenance, and full runtime module completion.
