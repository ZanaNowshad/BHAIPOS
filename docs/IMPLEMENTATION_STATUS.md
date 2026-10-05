# BHAIPOS Implementation Status

Status vocabulary:

- **LOCAL VERIFIED** — executed successfully in the generation environment.
- **IMPLEMENTED / BUILD PENDING** — source and tests exist, but the Rust toolchain/dependencies were unavailable locally; CI must compile and execute them.
- **SCHEMA READY** — normalized persistent model exists, but command/service/UI behavior is not complete.
- **NOT IMPLEMENTED** — runtime behavior still needs engineering.

## Evidence currently available

**LOCAL VERIFIED**

- All twenty-three SQLite migrations load into a clean in-memory database and can be reapplied safely.
- 171 application tables created.
- 259 integrity/security triggers created.
- 98 `*_fils` financial columns use integer affinity.
- `PRAGMA foreign_key_check` returns no violations after schema creation.
- A deliberately cross-tenant device/branch insert is rejected by the database guard.
- Audit mutation is rejected by immutable trigger.
- The same barcode can exist independently in separate tenants.
- Authoritative Rust money/tax/store source contains no `f32`/`f64` financial types.
- UI contract contains 1024×768 minimum and 48px minimum control sizing.
- Cashier, Admin, and Shift/EOD 1024×768 visual previews render successfully.
- Migration reopen/idempotency path passes after additive cash/EOD schema upgrade.
- Idempotency results require an immutable canonical request binding.
- Operation IDs are unique per tenant across mutation actions; unbound, changed-payload, and cross-action reuse fail closed.
- The additive binding migration preserves historical unbound idempotency results; replay is routed to manual review because the original request digest is unknowable.
- Core financial persistence rejects floating/text values, negative tax rates and non-zero rates on non-taxable categories even when a caller bypasses the Rust domain layer.
- Audit verification follows stored hash links and rejects forks/disconnected topology; event append fails closed if the existing device chain is inconsistent.
- Local terminal identity is persisted independently from synced device rows; cross-scope mutation and ordinary update/delete are rejected.
- The renderer request surface cannot submit tenant, branch, device or user authority fields; protected commands revalidate current device/user state.
- `src/domain/money.ts` passes strict standalone TypeScript type checking.
- The cashier React application passes strict TypeScript checking and a production Vite build with typed local commands for setup/login, cart scan/hold/restore, checkout, historical refund lookup/quote/commit, failed-print recovery and lock/logout.
- `0008_receipt_reprint_permission.sql` safely grants the new receipt-recovery permission to existing owner/administrator roles; reapplication is verified.
- Print lease/profile migration reapplication, default profile upgrade, device/tenant scope guard and renderer exclusion checks pass.
- Sync protocol migration reapplication, immutable hub/reconciliation evidence, tenant guards and narrow renderer authority scans pass.
- Inventory operation migration reapplication, immutable receipt/lot/count/approval evidence and tenant/value guards pass.
- Procurement/supplier-finance migration reapplication, immutable order/invoice/payment/return evidence and tenant/value guards pass.
- Customer/store operation migration reapplication, immutable loyalty/credit/payment evidence and tenant/value guards pass.
- Production migration reapplication, immutable completion/usage evidence and tenant guards pass.
- Expense migration reapplication, exact-fils validation, state-transition evidence, post-submission financial immutability, payment amount binding and tenant/device guards pass.
- Operational-alert migration reapplication, tenant/branch/device scope, legal lifecycle transitions, mandatory closure evidence and append-only history guards pass.
- Background-job migration reapplication, tenant/branch/device/user scope, legal state transitions, append-only event evidence and payload-bound operation-result guards pass.
- Backup/restore migration reapplication, immutable operation/event evidence, tenant/device/user guards and historical record identity guards pass.
- Backup-schedule migration reapplication, payload-bound configuration/ticks, bounded authorization expiry, atomic due-job enqueue and immutable schedule evidence guards pass.
- Backup-retention migration reapplication, append-only schedule/output evidence, legal run/item transitions, tenant/device scope guards and payload-bound result replay pass.
- Diagnostics migration reapplication, explicit view permission, immutable scoped snapshot guards and redacted typed command-surface checks pass.

**CI VERIFIED**

- GitHub Actions run `37380047293` passes Rust formatting, strict Clippy, the full Rust workspace test suite, desktop Rust compilation, desktop TypeScript checking and the production UI build for trusted operational diagnostics and backup administration.
- The permission-scoped diagnostics service performs live SQLite quick/foreign-key checks and returns only tenant/branch/device/register-scoped health, sync/job/print and backup evidence. The owner UI supports manual backup dispatch, schedule configuration, verified restore preview and explicit typed restore confirmation without renderer-provided authority or filesystem paths.
- GitHub Actions run `37377389533` passes Rust formatting, strict Clippy, the full Rust workspace test suite, desktop Rust compilation, desktop TypeScript checking and the production UI build for safe scheduled-backup retention.
- Scheduled backup output is linked durably to its originating schedule/job/device. Retention keeps the configured newest set, preserves every restore-referenced backup, uses a payload-bound replay-safe operation, refuses symlinks/non-files/path escape, resumes safely after delete-before-record interruption and routes deletion failures to review.
- GitHub Actions run `37308423966` passes Rust formatting, strict Clippy, the full Rust workspace test suite, desktop Rust compilation, desktop TypeScript checking and the production UI build for bounded scheduled backups.
- Recurring backup configuration and due evaluation are payload-bound and device-scoped; due enqueue/checkpoint movement is atomic; expired, suspended or permission-revoked authority fails closed to review; logged-out claims revalidate the active schedule and its authorizing user before execution.
- GitHub Actions run `37261516269` passes Rust formatting, strict Clippy, the full Rust workspace test suite, desktop Rust compilation, desktop TypeScript checking and the production UI build for the trusted backup administration bridge and durable backup worker dispatch.
- Manual backup commands enqueue durable jobs without accepting renderer-provided authority or filesystem paths. The trusted worker recovers expired jobs, honors cancellation, reuses the job identity for replay-safe backup creation and routes unsupported/semantic failures to review.
- GitHub Actions run `37260286733` passes Rust formatting, strict Clippy, the full Rust workspace test suite, desktop Rust compilation, desktop TypeScript checking and the production UI build for verified WAL-aware backup and restore.
- Backups use SQLite's online backup API, streaming SHA-256, full integrity and foreign-key checks, exact tenant/terminal compatibility and payload-bound replay. Owner-only restore requires a matching preview hash, creates a verified pre-restore safety backup, and stages the restored database together with immutable restore/idempotency/audit evidence before replacing the live image.
- GitHub Actions runs `37240547597` and `37258354228` pass Rust formatting, strict Clippy, the full Rust workspace test suite, desktop Rust compilation, desktop TypeScript checking and the production UI build for the durable background-job engine and expanded lost-response replay assertions.
- Permissioned job enqueue/claim/heartbeat/progress/cancellation/retry/failure/recovery operations use payload-bound idempotency, opaque leases, bounded monotonic progress, retry backoff, immutable events and material-action audit evidence. Expired workers are recoverable without duplicating terminal effects.
- GitHub Actions run `37238619386` passes Rust formatting, strict Clippy, 10 core tests, all 52 store invariant tests, desktop Rust compilation, desktop TypeScript checking and the production UI build for the evidence-driven operational-alert evaluator.
- A permissioned, payload-bound evaluation operation atomically produces active deduplicated alerts from low-stock, lot-expiry, delayed-sync, offline-terminal, overdue-supplier-invoice, failed-backup and repeated-authentication-failure evidence. Replays return the original result; new evaluations report existing alerts without flooding. Calendar-date conditions use Bahrain local time while duration cutoffs remain UTC.
- GitHub Actions run `37227313354` passes Rust formatting, strict Clippy, the full Rust workspace test suite, desktop Rust compilation, desktop TypeScript checking and the production UI build for the automatic operational-alert producer change.
- Unknown-barcode scans atomically persist scan evidence and one active deduplicated alert; repeated scans update evidence without alert flooding.
- Checkout atomically raises a high-severity negative-stock alert when its inventory consequence makes the affected product/centre balance negative.
- Cash-session close atomically raises a severity-matched cash-variance alert linked to the persisted variance case.
- Producer-generated alerts retain authoritative tenant, branch, device and user attribution, append immutable alert history, and add tamper-evident audit evidence inside the source operation transaction.
- GitHub Actions run `37228681820` passes Rust formatting, strict Clippy, the full Rust workspace test suite, desktop Rust compilation, desktop TypeScript checking and the production UI build for the trusted Alert Centre change.
- The Admin Alert Centre lists only the authenticated terminal branch's active alerts, exposes legal lifecycle actions according to live capabilities, requires closure evidence and binds assignment to the authenticated user inside the trusted desktop service; backend RBAC remains authoritative.

**IMPLEMENTED / BUILD PENDING**

- integer-fils money with checked aggregation/variance/change arithmetic and deterministic validated basis-point VAT;
- integer thousandth quantities;
- Argon2id PIN hash/verification and lockout;
- device secret hash/authentication, credential rotation, suspend/revoke;
- fail-closed RBAC for checkout/refund/cash-session open;
- bound HMAC manager approvals and nonce replay prevention;
- barcode classification/weighted EAN parser;
- persistent cart, hold/restore;
- exact tender validation, cash change, BenefitPay evidence status;
- idempotent atomic checkout;
- canonical SHA-256 request binding for checkout, refund, cash movement, cash-session close and sale void;
- checked payment-total accumulation that reports overflow instead of wrapping/panicking;
- receipt sequence/business date/snapshot/hash;
- inventory movement and stock cache effects;
- permissioned, idempotent receiving with accepted/rejected/damaged evidence and lot/expiry balances;
- draft/dispatch/partial-receipt inventory transfers without destination stock teleportation;
- movement-aware stocktake approval, append-only adjustment evidence and reconstructable stock/cost projections;
- integer weighted-average inventory valuation and idempotent costed waste;
- permissioned supplier terms, idempotent requisitions and exact-fils purchase orders with explicit approval/order transitions;
- partial purchase-order receiving with quantity/damage/cost discrepancy evidence and atomic stock effects;
- supplier invoice posting, exact allocation, append-only ledger statements, and stock-backed credit/replacement returns;
- tenant-scoped customer creation with Bahrain phone normalization, append-only loyalty, credit limits, atomic customer-credit checkout and idempotent collection;
- versioned recipes and idempotent production completion with once-only component consumption, weighted-average output cost and append-only movements;
- idempotent expense category/expense creation, submit/approve/reject/pay transitions, immutable event/payment evidence and exact operating-profit arithmetic;
- idempotent delivery creation and legal state transitions, assigned-rider dispatch, exact payment collection, and append-only courier cash settlement/allocation evidence;
- separate employee records plus retry-safe clock/break events, immutable attendance evidence, missing-clock-out resolution and exact integer-second work reports;
- payload-bound operational-alert creation and legal acknowledgement/investigation/closure transitions, assignment, mandatory closure notes, active-alert retrieval, immutable device-attributed history and audit evidence;
- atomic, active-alert-deduplicated producers for unknown barcodes, checkout-created negative stock and cash-session close variance;
- partial refund limit/idempotency/stock compensation plus explicit tender reversal effects, duplicate-line rejection and historical receipt-scoped quoting;
- paid-in/paid-out/safe-drop/no-sale cash movements and session-level expected cash;
- cash-session close with counted cash, variance, X/Z/EOD report data, and variance-case creation;
- compensating full-sale void with exact consumed manager approval, inventory/tender reversal evidence, and preserved original receipt;
- print queue creation, claiming, failure, requeue, retry and successful completion state transitions;
- opaque print leases, five-minute interrupted-worker recovery, stale-worker rejection, historical snapshot integrity validation, ESC/POS 58/80mm profiles, cut and cash-sale drawer-pulse policy;
- trusted desktop worker adapters for Windows RAW spooler and serial/COM receipt output; the renderer cannot claim or complete print jobs;
- sync queue creation;
- one-time expiring terminal enrollment grants, stable device identity, Windows Credential Manager custody, permissioned credential rotation and terminal revocation enforcement;
- HMAC-bound mutation envelopes, durable send leases/backoff, payload-bound hub replay, checkpoints, stale-worker rejection and append-only conflict resolution evidence;
- hash-chained audit creation/verification independent of timestamp or UUID ordering;
- Rust invariant test suite and GitHub CI workflow;
- Tauri bootstrap and Cashier/Admin React shell source.
- typed Tauri command allow-list for local bootstrap/login, cash-session recovery, cart scan, checkout, print recovery, refund, cash movement/report and close;
- durable background-job engine with payload-bound enqueue, replay-safe claims, opaque renewable leases, bounded progress, cancellation, retry backoff, terminal failure/review states and expired-worker recovery;
- verified WAL-aware manual backup, integrity/hash validation, owner-only compatibility preview and restore, pre-restore safety backup, staged atomic replacement, replay-safe results and append-only recovery evidence;
- bounded owner-authorized backup schedules with exact-once due enqueue, fail-closed permission/expiry review, trusted logged-out worker claims and a typed authority-free desktop configuration contract;

## Schema-ready modules requiring service implementation

Product/category scheduling; branch/channel pricing APIs; price/cost history commands; pricing policy and repricing review; margin protection; promotions/coupons/conflict resolution; bundles/hampers; FEFO allocation; near-expiry markdown workflow; transfer cancellation; batch inventory operations; valuation-at-date/slow/dead stock/replenishment queries; procurement administration UI and approval thresholds; invoice attachments/credit-note depth; expense attachments/approval thresholds and petty-cash reconciliation; customers/addresses; loyalty earn/redeem/expiry; credit allocation/statements/aging; delivery/courier workspace and return/refund compensation; digital order hub; marketplace settlement reconciliation; production planning/yield; attendance scheduling/late/early UI; WhatsApp metadata; OCR review records; AI action/undo metadata; document library; overdue-customer-credit and settlement-discrepancy alert producers after their authoritative domains exist; concrete job handlers for imports, OCR, AI and sync; feature flags; diagnostic export/history and update records.

## Runtime components still not implemented

- Cashier line mutation, customer association, promotions, price override and manager-approval UX remain incomplete; committed totals are already trusted-service authoritative.
- Full Admin CRUD and report/analytics UI.
- Physical Windows printer validation, printer configuration UI, QR/barcode commands and printer-health diagnostics remain incomplete. The RAW spooler/COM worker, cut, policy-bound drawer pulse and durable retry boundary are implemented but not natively built or hardware-tested here.
- Petty-cash fund runtime and reconciliation beyond drawer-linked petty cash remains incomplete.
- Printable X/Z report document rendering and ESC/POS delivery remain incomplete; the underlying session accounting is implemented.
- Deployable branch hub process/network adapter and Reconciliation Centre UI remain incomplete; the authenticated envelope acceptance, enrollment, durable lease/backoff, lost-response replay, watermark and append-only resolution domain services are implemented but await Rust/multi-process execution.
- Real multi-terminal conflict/failure testing.
- Supplier purchasing and receiving command services.
- Channel/alert command services, attendance scheduling UI, delivery workspace/compensation, and remaining loyalty/credit administration.
- WhatsApp Node.js sidecar and QR pairing.
- OCR engine/provider integration and review UI.
- AI provider adapters, tool registry, risk classifier, confirmation UX, compensating/undo executor and knowledge retrieval.
- Windows Credential Manager integration for secrets.
- Backup encryption/key custody, external-backup import policy and Windows crash-interruption exercise. Trusted owner administration, durable manual dispatch, bounded unattended scheduling, safe replayable retention and restore preview/confirmation are implemented.
- Import/migration wizard and spreadsheet injection sanitizer.
- Reports/analytics query layer and export generators.
- Signed updater and rollback behavior.
- Windows MSI/NSIS build verification, Authenticode signing, SBOM and provenance manifest.
- Arabic/RTL receipt shaping validation.
- 100k-product/500k-sale performance benchmark.
- Full failure-injection matrix.
- Clean Windows acceptance on a fresh machine.

## Release blockers

BHAIPOS must not be called production-ready until, at minimum, the following evidence exists:

1. Rust core compiles with no warnings under CI; all invariant tests pass.
2. Desktop React/Tauri build passes on Windows x64.
3. Cashier UI is wired only to typed backend commands; no frontend-authoritative totals or direct DB access.
4. Offline sale, split payment, print failure, refund, shift close and restart recovery pass end-to-end.
5. Device revocation and permission denial are exercised through the real desktop command path.
6. Multi-terminal lost-response replay proves one committed sale/operation.
7. Sync conflicts become `REQUIRES_REVIEW`, not silent LWW for financial/inventory ledgers.
8. Backup → corruption simulation → restore → integrity validation passes.
9. Upgrade preserves business records and WAL-safe state.
10. Clean Windows install/uninstall acceptance passes; uninstall preserves business data.
11. Production installer/executable are signed and SHA-256/provenance/SBOM artifacts are generated.
