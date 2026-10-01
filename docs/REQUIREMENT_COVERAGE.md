# BHAIPOS Requirement Coverage Matrix

Assessment date: 2026-10-01
Authority baseline: public implementation commit `f2afd9b` plus Task 7 expense-operations working tree
Working branch: `codex/idempotency-payload-binding-20260929`

Status vocabulary is deliberately limited to `NOT STARTED`, `IN PROGRESS`, `IMPLEMENTED — UNVERIFIED`, `VERIFIED`, and `BLOCKED`. A schema or screen alone is not treated as implementation.

| ID | Requirement | Status | Implementation / evidence | Known limitation / next gate |
|---|---|---|---|---|
| 01 | Production retail OS objective | IN PROGRESS | `README.md`, `docs/ARCHITECTURE.md`; integrated foundation for commerce, cash, stock, identity, audit, print and sync intent | Most back-office services and end-to-end desktop workflows remain incomplete |
| 02 | Repository-first operating rules | VERIFIED | Clean two-commit source baseline inspected; `docs/IMPLEMENTATION_STATUS.md` records implemented and missing runtime surfaces | Rust/Windows execution still needs capable runners |
| 03.1 | Integer-fils money | IMPLEMENTED — UNVERIFIED | `bhaipos-core::Money`; checked aggregation/change/variance; `price_times_quantity`; `0006_financial_domain_guards.sql`; Python dynamic-type rejection passes | Rust invariant suite cannot run in this container |
| 03.2 | Tenant isolation | IMPLEMENTED — UNVERIFIED | Authoritative scope checks plus DB guards in `0003_integrity_guards.sql`; Python cross-tenant checks pass | Guard every schema-ready table before exposing its write API |
| 03.3 | Branch/device/user context | IMPLEMENTED — UNVERIFIED | `0007_local_terminal_binding.sql`; authenticated desktop state injects authority; protected commands revalidate binding/device/user | Cargo execution and full UI runtime exercise remain blocked locally |
| 03.4 | Immutable financial history | IMPLEMENTED — UNVERIFIED | Append-only triggers for sales evidence, refunds, cash, inventory, audit and ledgers | Complete compensating commands beyond sale void/refund are missing |
| 03.5 | Historical snapshots | IMPLEMENTED — UNVERIFIED | Sale-line and receipt snapshots; `historical_receipt_snapshot_does_not_change_with_catalog` | Full business/customer/tender presentation snapshot needs expansion |
| 03.6 | Payload-bound idempotency | IMPLEMENTED — UNVERIFIED | `0005_idempotency_payload_binding.sql`; canonical digests for checkout, refund, void, cash movement and close; mismatch/cross-action tests | Cargo execution required; legacy unbound result replay fails closed for review |
| 03.7 | Atomic checkout | IMPLEMENTED — UNVERIFIED | `BEGIN IMMEDIATE` sale, lines, tenders, stock, receipt, print, audit, sync and idempotency transaction | Desktop end-to-end crash/lost-response test not run |
| 03.8 | Append-only inventory | IMPLEMENTED — UNVERIFIED | `0011_inventory_operations.sql`; immutable movement/lot evidence; stock and cost projections; permissioned ledger rebuild with repair evidence | Cargo execution and broader production/BOM movement coverage remain |
| 03.9 | Fail-closed authorization | IMPLEMENTED — UNVERIFIED | Permission allow-list checks and denial test | Full command surface is not yet implemented |
| 03.10 | Tamper-evident audit | IMPLEMENTED — UNVERIFIED | Per-device SHA-256 link topology, immutable triggers, tamper/fork/order regressions and fail-closed append | Rust execution plus operational verification UI/alert are missing |
| 04 | Offline authority and policy | IN PROGRESS | Local SQLite authority, atomic bootstrap, offline Argon2 PIN login and local commands exist | Policy versions, bounded offline windows, broader stale-state controls and reconciliation are missing |
| 05 | Trusted terminal model | IMPLEMENTED — UNVERIFIED | Durable identity/binding, expiring one-use enrollment, Windows Credential Manager secret custody, versioned permissioned rotation, suspend/revoke and attribution | Rust/Windows execution, heartbeat service and remote enrollment UX missing |
| 06 | Action-bound manager approval | IMPLEMENTED — UNVERIFIED | HMAC binding, expiry and nonce consumption; void consumes exact approval | Production key custody and broader sensitive-action integration missing |
| 07 | Identity, RBAC, employees, attendance | IN PROGRESS | Argon2id PINs, lockout, users/roles; employee/attendance schema | Admin workflows, custom-role UI and attendance service missing |
| 08 | Cashier workspace | IN PROGRESS | 1024×768 React shell, 48px touch contract, typed command wiring, scanner-focus restoration and lock/logout | Function-key paths and real scanner/touch Windows E2E remain missing |
| 09 | POS and carts | IN PROGRESS | Persistent durable carts, trusted barcode add/totals, hold/restore and checkout UI | Revalidation diff UX, search/PLU depth, line mutation and override flows incomplete |
| 10 | Pricing, tax and promotions | IN PROGRESS | Base/branch price lookup, validated tax snapshots, immutable price/cost evidence and price/promotion schema | Deterministic promotion engine, history commands and margin approvals missing |
| 11 | Bahrain tax/localization | IN PROGRESS | BHD, Asia/Bahrain default, business date and configurable tax schema | Current NBR compliance has not been authoritatively researched/accepted; Arabic/RTL incomplete |
| 12 | Checkout and payments | IN PROGRESS | Cash/Card/BenefitPay/etc. model, split validation, change and evidence boundary | Provider settlement integration and full tender policy service missing |
| 13 | Receipts and printing | IMPLEMENTED — UNVERIFIED | Immutable receipt text/hash; opaque crash-recoverable leases; stale-token rejection; 58/80mm ESC/POS rendering; Windows RAW spooler/COM worker; cut/cash-drawer policy; permission-scoped recovery UI | Cargo/Windows printer hardware exercise, QR/barcode output, config UI and Arabic-capable device validation remain missing |
| 14 | Refunds and voids | IMPLEMENTED — UNVERIFIED | Historical receipt lookup/quote UI, partial limits, duplicate-line rejection, tender/stock effects, compensating void and Rust tests | Rust/UI runtime and multi-terminal concurrency evidence missing |
| 15 | Register, drawer and cash | IN PROGRESS | Register/session, movements, X/Z/EOD data, close variance and cases | Drawer hardware, printable reports and complete petty-cash fund runtime missing |
| 16 | Catalogue and barcodes | IN PROGRESS | Products, alternate barcode table, classifier, weighted parser and ghost scans | Configurable scale layouts, merge workflow and duplicate review missing |
| 17 | Branch assortment/scheduling | IN PROGRESS | Branch assortment and scheduling schema; sellable filter in barcode lookup | Management service/UI and schedule enforcement incomplete |
| 18 | Bundles, kits, hampers and BOM | IN PROGRESS | Versioned production recipes define explicit completion-time component consumption | Sell-time bundles/kits/hampers and packaging-cost workflows remain |
| 19 | Inventory centres/transfers/lots | IMPLEMENTED — UNVERIFIED | Permissioned idempotent receiving; centre/inter-branch draft, dispatch, partial receipt and damage evidence; lot balances and expiry query | Cargo execution, cancellation and FEFO allocation/markdown workflows remain |
| 20 | Stocktake and valuation | IMPLEMENTED — UNVERIFIED | Movement-aware snapshot/count/approval, compensating adjustment movements, reconstructable cache and integer weighted-average projection | Cargo execution, valuation-at-date report and scanner UI remain |
| 21 | Inventory intelligence | NOT STARTED | Alert/replenishment schema only | Precise queries, definitions and evidence required |
| 22 | Suppliers and purchasing | IMPLEMENTED — UNVERIFIED | `0012_procurement_supplier_finance.sql`; supplier terms; idempotent requisitions/POs; approval/order transitions; partial receipt with quantity, damage and cost discrepancy evidence | Cargo execution, admin UI, approval thresholds and reliability analytics remain |
| 23 | Supplier finance | IMPLEMENTED — UNVERIFIED | Exact invoice posting; payment allocation; append-only supplier ledger/statement; stock-backed return credit/replacement flow | Cargo execution, attachments, aging UI and richer credit-note reconciliation remain |
| 24 | OCR purchase entry | NOT STARTED | OCR proposal/review storage exists | Isolated provider, evidence storage and human approval UI required |
| 25 | Expenses and operating profit | IMPLEMENTED — UNVERIFIED | `0015_expense_operations.sql`; exact-fils draft/submit/approve/reject/pay workflow; immutable event/payment evidence; payload-bound retries; trusted operating-profit report with `net sales - COGS - paid operating expenses` definition | Rust CI execution, attachments, approval thresholds, admin UI and cash-basis/accrual reporting options remain |
| 26 | Customers, loyalty and credit | IN PROGRESS | `0013_customer_store_operations.sql`; canonical Bahrain phone handling; append-only loyalty/credit; limit-enforced atomic credit checkout; idempotent collections and balance/overdue query | Address UI, allocation/aging buckets, loyalty tiers/expiry jobs and offline policy controls remain |
| 27 | Delivery and courier control | NOT STARTED | Delivery/custody schema only | Status service, payment events and settlement workflow required |
| 28 | Sales channels/marketplace | NOT STARTED | Channel and settlement schema only | Pricing/accounting logic and reconciliation runtime required |
| 29 | WhatsApp sidecar | NOT STARTED | Metadata boundary represented in schema/docs | Authenticated isolated sidecar and failure tests required |
| 30 | Payment screenshot review | NOT STARTED | Evidence/review schema and non-settlement policy documented | OCR matcher and conservative review workflow required |
| 31 | Production and waste | IMPLEMENTED — UNVERIFIED | Idempotent completion consumes actual components once, records expected/actual usage and weighted-average output cost; waste remains idempotent/costed | Cargo execution, planning UI and yield/variance reports remain |
| 32 | Reporting and analytics | IN PROGRESS | Cash-session report and schema inputs exist | Most reports, definitions, pagination and exports missing |
| 33 | Business date/store operations | IN PROGRESS | Bahrain close-hour business date and regression test | Checklists and full EOD orchestration missing |
| 34 | Alert centre | NOT STARTED | Durable alert schema only | State workflow, assignment, resolution history and producers required |
| 35 | Synchronization | IMPLEMENTED — UNVERIFIED | Signed device envelopes; durable leases/backoff; payload-bound hub replay; immutable event acceptance; watermarks; `REQUIRES_REVIEW`; append-only manager resolution | Rust/multi-process tests, network adapter, deployed hub and reconciliation UI remain missing |
| 36 | Terminal health | NOT STARTED | Device/diagnostic fields exist | Heartbeat collector and health UI missing |
| 37 | AI back office | NOT STARTED | Safety/action/undo schema and architecture boundary documented | Tool registry, policy engine, previews, confirmations and providers missing |
| 38 | Knowledge base | NOT STARTED | Versioned-document schema only | Storage, indexing, permissions and archive flows required |
| 39 | Imports and exports | NOT STARTED | No executable wizard | CSV/XLSX mapping, conservative match, audit and formula-injection handling required |
| 40 | Backup, restore and DB health | NOT STARTED | Backup/restore record schema only | WAL-aware implementation, compatibility preview and recovery tests required |
| 41 | Background jobs | NOT STARTED | Durable job schema only | Worker, progress, cancellation and recovery semantics required |
| 42 | Diagnostics | NOT STARTED | Diagnostic data model inputs exist | Redacted export and runtime checks required |
| 43 | Update and release security | NOT STARTED | Tauri packaging shell and CI core checks only | Signed updater, Authenticode, manifest, SBOM and provenance required |
| 44 | Tauri trust boundary | IMPLEMENTED — UNVERIFIED | Minimal capabilities/CSP; explicit typed command allow-list; renderer DTOs omit authority; no shell/raw DB/filesystem command | Cargo/runtime verification and future sidecar IPC review required |
| 45 | Feature flags | NOT STARTED | Feature-flag schema only | Runtime gating and disabled-module tests required |
| 46 | Retail UI/UX | IN PROGRESS | Cashier/Admin/Shift shells and visual previews | Production workflows, dark theme, Arabic/RTL and scanner-focus E2E missing |
| 47 | Performance targets | NOT STARTED | Indexes exist for core lookups | 100k/500k/million-row benchmark and budgets not measured |
| 48 | Invariant/failure testing | IN PROGRESS | Rust core invariant suite includes money overflow, zero-value transaction and audit topology cases; Python schema verification passes | Cargo execution and full failure-injection/recovery matrix missing |
| 49 | Clean Windows acceptance | BLOCKED | Acceptance requirements documented | No BHAIPOS Windows artifact or clean Windows run yet |
| 50 | Dependency-ordered implementation | IN PROGRESS | Core-invariant-first architecture and migrations | Proceed through typed command bridge before broad modules |
| 51 | Execution discipline | IN PROGRESS | Focused migrations/tests and evidence records used | CI/runtime evidence must accompany every future capability |
| 52 | Requirement traceability | VERIFIED | This matrix exists and links status to source/evidence | Update on every material change; never upgrade status without evidence |
| 53 | Definition of complete | IN PROGRESS | Release blockers and evidence rules documented | Product is not complete or production-ready |
| 54 | Out-of-scope drift control | VERIFIED | Boundaries documented in architecture/status files | Re-evaluate when enabling accounting/payroll/provider modules |
| 55 | Evidence-based reporting | VERIFIED | `VERIFICATION.md`, this matrix and exact local baseline/branch | GitHub/CI identifiers can be added only after write access and runs exist |

## Current highest-leverage next gate

Compile and execute the expense workflow on CI, then implement delivery/courier cash custody through the same idempotency, audit and append-only financial-event boundaries.
