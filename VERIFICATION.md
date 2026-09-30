# Verification Record — 2026-09-29

Executed successfully in the generation environment:

- `python scripts/verify_foundation.py`
  - schema tables: 121
  - integrity triggers: 93
  - integer financial columns: 90
  - foreign-key check: OK
  - cross-tenant guard test: OK
  - immutable audit test: OK
  - cross-tenant barcode isolation: OK
  - authoritative Rust float scan: OK
  - 1024×768 UI contract scan: OK
  - migration reopen/idempotency path: OK
  - idempotency result requires durable request binding: OK
  - operation ID cannot be reused across actions: OK
  - idempotency request binding is immutable: OK
  - pre-migration unbound idempotency evidence is preserved: OK
  - SQLite REAL/TEXT values are rejected for protected financial fields: OK
  - negative and non-taxable-category tax-rate violations are rejected: OK
  - audit-chain verification is hash-link/topology based rather than timestamp/UUID ordered: OK
  - immutable, tenant/branch-consistent local terminal binding: OK
  - renderer request authority-field exclusion and explicit Tauri command allow-list: OK
  - CI definitions cover core Rust, desktop Rust and frontend typecheck/build: OK
- `tsc --strict --noEmit --target ES2022 apps/desktop/src/domain/money.ts`: PASS
- SQLite migrations execute cleanly in Python SQLite and can be reapplied to the same database without duplicate-column failure.
- 1024×768 Cashier, Admin, and Shift/EOD visual previews rendered to PNG successfully.

Not executed successfully in this environment:

- `cargo fmt/clippy/test`: BLOCKED because Cargo/Rust is not installed in the container.
- Full React typecheck/build: BLOCKED because React/Tauri npm dependencies are not installed and external DNS/package resolution is unavailable.
- Windows/Tauri build and clean-install acceptance: NOT RUN because this is not a Windows build host.

Implemented after the original `3fb1dd2` verification snapshot, but still awaiting Rust compilation:

- canonical SHA-256 request binding for checkout, refund, cash movement, cash-session close and sale void;
- fail-closed rejection of changed-payload replay and cross-action operation-ID reuse;
- checked payment summation for checkout/refund overflow safety;
- Rust regression tests `checkout_replay_rejects_a_changed_payload` and `operation_id_is_payload_bound_and_cannot_cross_actions`.
- migration `0006_financial_domain_guards.sql`, protecting core money, quantity and tax fields from SQLite dynamic-type bypass;
- checked checkout, refund, cash-change, cash-session and variance arithmetic with the unchecked `Money` operator traits removed;
- zero-value sale/refund behavior without fabricated zero-value tender rows;
- topology-based audit verification and fail-closed append when an existing chain is forked, cyclic, dangling or otherwise inconsistent;
- Rust regressions `checkout_money_overflow_fails_before_any_sale_is_committed`, `zero_value_item_can_be_sold_and_returned_without_fabricated_payment`, `audit_chain_uses_hash_links_instead_of_timestamp_or_uuid_order`, and `audit_append_fails_closed_when_the_existing_chain_is_forked`.
- migration `0007_local_terminal_binding.sql`, atomic first-run business/branch/register/device/owner bootstrap, offline employee-number/PIN login, per-command session revalidation and active cash-session recovery;
- typed renderer-safe Tauri requests for the current POS financial core, with terminal/user authority injected only from trusted desktop state;
- Rust regressions `local_bootstrap_is_atomic_persistent_and_offline_login_capable` and `cash_session_open_replay_returns_existing_only_for_the_same_payload`.

A GitHub Actions workflow is included to run Rust formatting, Clippy and tests when this source is placed in a repository with network-enabled runners.
