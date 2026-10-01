# BHAIPOS Dependency-Ordered Implementation Plan

Spec authority: `docs/REQUIREMENT_COVERAGE.md`, the repository architecture/security documents, and the product build directive supplied by the owner.

Global constraints: preserve integer-fils money, authenticated tenant/branch/device/user context, payload-bound idempotency, append-only evidence, atomic domain transactions, offline authority, and fail-closed behavior. A task is not `VERIFIED` until its executable runtime tests pass.

## Task 1 — Typed desktop command boundary

Status: **IMPLEMENTED — UNVERIFIED**. Static authority verification passes; Rust compilation/runtime awaits a Cargo-capable runner.

Produce typed Tauri request/response contracts for health, local bootstrap, login, barcode/cart operations, checkout, print recovery, refunds, cash movements, cash reports and session close. Resolve authority from a locally held authenticated session rather than accepting tenant/device/user identities from renderer payloads. Keep the capability set narrow.

Test-first gate: add bridge contract assertions to `scripts/verify_foundation.py`, observe failure, implement the commands, then make the verifier pass. Run Cargo formatting, Clippy and tests when the toolchain is available; otherwise record the exact blocker and retain `IMPLEMENTED — UNVERIFIED` status.

Expected: Python foundation verification passes; no command accepts renderer-provided tenant, branch, device or user authority; database schema reports the latest applied migration.

## Task 2 — Cashier runtime wiring

Status: **IMPLEMENTED — UNVERIFIED**. Python command-contract verification, strict TypeScript checking and the production Vite build pass locally; native Tauri/Rust and Windows retail-device exercise remain outstanding.

Replace seeded/frontend-authoritative cashier state with typed invoke calls. Implement durable cart identity, barcode focus, held-cart restoration, tender entry, checkout result rendering, print retry and lock/logout behavior.

Test-first gate: UI command-contract tests fail before implementation and pass after it; full TypeScript/React build passes when dependencies are available.

Expected: the renderer never calculates authoritative sale totals and has no raw SQL/filesystem/shell access.

## Task 3 — Offline print and restart recovery

Status: **IMPLEMENTED — UNVERIFIED**. Schema/trust-boundary verification and frontend regression build pass locally. Rust compilation, Windows RAW spooler/COM execution and physical printer failure injection require a capable Windows/Cargo runner.

Implement the Windows spooler/ESC-POS worker boundary, durable job leasing/retry, stale lease recovery, 80mm/58mm historical receipt rendering and drawer pulse policy.

Test-first gate: printer failure, process interruption, retry and original-snapshot reprint tests.

## Task 4 — Synchronization and trusted terminals

Status: **IMPLEMENTED — UNVERIFIED**. Domain/protocol source and failure-recovery tests cover enrollment, rotation/revocation, signed delivery, backoff, lost-response replay, checkpoints and review resolution. Cargo, Windows Credential Manager and real multi-process/network execution remain outstanding.

Implement enrollment, OS-protected credential storage, rotation/revocation, authenticated mutation transport, watermarks, lost-response replay and reconciliation states without last-write-wins for ledgers.

Test-first gate: duplicate replay, lost response, restart, credential rotation, revoked device and semantic-conflict recovery tests.

## Task 5 — Inventory operations

Status: **IMPLEMENTED — UNVERIFIED**. Migration and trusted domain source cover append-only receiving, cost/stock projection rebuild, weighted-average valuation, dispatch/partial receive transfers, movement-aware stocktake, lots/expiry and waste. Static schema/boundary checks pass; Cargo execution remains outstanding.

Implement ledger rebuild, receiving, centre/inter-branch transfer states, movement-aware stocktake, lots/expiry, waste, weighted-average valuation and reconciliation.

Test-first gate: ledger/cache consistency, conservation, stale stocktake, partial receipt, production/waste and valuation tests.

## Task 6 — Procurement and supplier finance

Status: **IMPLEMENTED — UNVERIFIED**. Migration and trusted domain source cover supplier terms, requisitions, exact purchase orders and transitions, partial/discrepancy receiving, invoice posting, payment allocation, append-only statements and stock-backed supplier returns. Static schema/boundary checks pass; Cargo execution remains outstanding.

Implement suppliers, requisitions/POs, discrepancy receiving, invoices, allocation, append-only supplier ledger, statements and returns.

Test-first gate: idempotent receiving/payment, exact balances, partial receipt, cost variance and reversal tests.

## Task 7 — Customer and store operations

Status: **IN PROGRESS**. Implemented slices cover tenant-scoped customers, Bahrain phone normalization, append-only loyalty, credit accounts/payments/balances, atomic customer-credit checkout, idempotent production completion, exact-fils expense approval/payment with reproducible operating-profit reporting, and delivery/courier collection plus cash-custody settlement. Attendance, marketplace settlement and alerts remain.

Implement loyalty, credit, delivery/courier custody, expenses, channels/settlements, BOM/production, attendance and alerts through the same authority/audit model.

Test-first gate: ledger reversal, limits/aging, actual delivery payment events, settlement discrepancies and idempotent production tests.

## Task 8 — Controlled integrations and administration

Implement isolated WhatsApp, OCR proposal/review, deterministic AI tools, documents, imports/exports, jobs, diagnostics and feature flags. Optional modules must not affect checkout availability.

Test-first gate: sidecar/provider failure, untrusted proposal rejection, permission scope, formula-injection and disabled-module tests.

## Task 9 — Backup, performance and Windows release

Implement WAL-aware backup/restore, signed update verification, benchmarks, failure injection, packaging, SBOM/provenance and the clean-Windows acceptance path. Never claim signatures that were not verified.

Test-first gate: restore/upgrade preservation, 100k/500k performance budgets, clean install/uninstall persistence and full acceptance evidence.
