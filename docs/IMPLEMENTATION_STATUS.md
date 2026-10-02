# BHAIPOS Implementation Status

Status vocabulary:

- **LOCAL VERIFIED** — executed successfully in the generation environment.
- **IMPLEMENTED / BUILD PENDING** — source and tests exist, but the Rust toolchain/dependencies were unavailable locally; CI must compile and execute them.
- **SCHEMA READY** — normalized persistent model exists, but command/service/UI behavior is not complete.
- **NOT IMPLEMENTED** — runtime behavior still needs engineering.

## Evidence currently available

**LOCAL VERIFIED**

- All seventeen SQLite migrations load into a clean in-memory database and can be reapplied safely.
- 158 application tables created.
- 206 integrity/security triggers created.
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

## Schema-ready modules requiring service implementation

Product/category scheduling; branch/channel pricing APIs; price/cost history commands; pricing policy and repricing review; margin protection; promotions/coupons/conflict resolution; bundles/hampers; FEFO allocation; near-expiry markdown workflow; transfer cancellation; batch inventory operations; valuation-at-date/slow/dead stock/replenishment queries; procurement administration UI and approval thresholds; invoice attachments/credit-note depth; expense attachments/approval thresholds and petty-cash reconciliation; customers/addresses; loyalty earn/redeem/expiry; credit allocation/statements/aging; delivery/courier workspace and return/refund compensation; digital order hub; marketplace settlement reconciliation; production planning/yield; attendance scheduling/late/early UI; WhatsApp metadata; OCR review records; AI action/undo metadata; document library; alerts; background jobs; feature flags; backups/restores; diagnostics/update records.

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
- Backup/restore implementation and compatibility verifier.
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
