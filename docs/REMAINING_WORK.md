# BHAIPOS Remaining Work Master Checklist

Assessment date: 2026-10-09  
Baseline: public `main` commit `10fd27aca0df9ee9e9982f909616713f6c1f8a6c`  
Source of truth: `docs/REQUIREMENT_COVERAGE.md`, `docs/IMPLEMENTATION_STATUS.md`, and the repository at the baseline above.

This is the dependency-ordered backlog for taking BHAIPOS from its current state to a production release. A checked source-code task is not automatically a completed capability. Each capability must also satisfy the common completion gate below.

## Common completion gate for every capability

- [ ] Domain rules and exact financial/inventory semantics are documented.
- [ ] Persistence is migration-driven, additive, tenant-scoped, and safe for existing data.
- [ ] Trusted commands derive tenant, branch, device/register, user, role, and permissions from authenticated runtime context.
- [ ] Retryable mutations have payload-bound durable idempotency.
- [ ] Financial, stock, security, and configuration evidence is immutable or corrected by compensation.
- [ ] Audit evidence is appended and its hash chain remains verifiable.
- [ ] Offline behavior, stale-state risks, and synchronization conflict semantics are explicit.
- [ ] Unit, integration, tenant-isolation, authorization, replay, and recovery tests pass.
- [ ] The real desktop workflow is exercised; a schema, command, or screen alone is insufficient.
- [ ] Coverage/status/evidence documents are updated with exact commits and CI/runtime results.

## Step 1 — Close the diagnostics and recovery gate

- [x] Add `diagnostics.export` permission, fail-closed role grants, and trusted command enforcement.
- [x] Add a durable diagnostic-snapshot operation with operation ID and canonical request digest.
- [x] Make identical replay return the original snapshot; reject changed-payload replay.
- [x] Store immutable, tenant/branch/device-scoped diagnostic snapshot history.
- [x] Produce a redacted export that excludes secrets, PIN material, device credentials, session cookies, provider keys, and sensitive filesystem paths.
- [x] Write exports atomically into an application-controlled directory; never accept a renderer-provided output path.
- [x] Add snapshot-history list/view UI and a trusted export action.
- [x] Add redaction, scope, immutable-history, idempotency, and interrupted-write tests.
- [ ] Add live printer probing without exposing device secrets.
- [ ] Exercise backup cancellation, process interruption, corrupt input, low disk, locked database, and restart recovery on Windows.
- [ ] Exercise restore preview, pre-restore safety backup, staged replacement, crash recovery, integrity verification, and replay on Windows.
- [ ] Keep production restore disabled until the Windows recovery evidence passes.

**Exit gate:** a redacted diagnostic bundle and snapshot history are usable through the desktop, and backup/restore recovery passes on a clean Windows test machine.

## Step 2 — Finish foundational identity, authorization, and offline policy

- [ ] Define versioned offline policy records and bounded offline authorization windows.
- [ ] Enforce credential/policy expiry for disconnected login and protected actions.
- [ ] Define offline allow/deny rules for refund, void, price override, credit, stock adjustment, supplier payment, and other high-risk actions.
- [ ] Add reconciliation outcomes for actions accepted under stale policy.
- [ ] Finish administrative PIN reset, account unlock, suspension, and credential-upgrade workflows.
- [ ] Finish configurable custom-role and permission administration with change audit.
- [ ] Integrate action-bound manager approval into every sensitive action, not only voids.
- [ ] Store approval signing keys in production-grade OS-backed custody and define rotation/recovery.
- [ ] Add approval replay/expiry/action/payload/target/branch/device mismatch tests for every integrated action.
- [ ] Add session timeout, forced lock, lockout policy administration, and revocation-on-reconnect behavior.
- [ ] Add audit-chain verification UI, scheduled verification, alert production, and repair/escalation procedure.
- [ ] Expand tenant/branch/device guard coverage before exposing each remaining schema-ready write API.
- [ ] Add application heartbeat collection and remote terminal enrollment/activation UX.
- [ ] Exercise Windows Credential Manager storage, rotation, revocation, reinstall, and recovery.

**Exit gate:** all protected commands are fail-closed, offline authority is bounded and versioned, and real Windows tests prove credential/approval behavior.

## Step 3 — Complete the cashier, cart, and checkout workflow

- [ ] Add exact SKU, normalized name, and PLU search with bounded paginated results.
- [ ] Finish quantity changes, decimal-weight validation, line deletion, notes, and customer association.
- [ ] Add authorized price override with action-bound manager approval and reason evidence.
- [ ] Add line/cart discount entry through the deterministic pricing service.
- [ ] Preserve stable line identities through hold/restore and retries.
- [ ] Revalidate restored carts for product sellability, branch availability, price, promotion, tax, and authorization.
- [ ] Show a clear before/after diff for material restored-cart changes; never silently overwrite state.
- [ ] Finish function-key paths, scanner-focus recovery, modal behavior, and keyboard-only checkout.
- [ ] Add complete tender-policy configuration and enforcement for cash, card, BenefitPay, bank transfer, and customer credit.
- [ ] Finish denomination shortcuts, amount-tendered/change UX, drawer behavior, and split-payment editing.
- [ ] Store complete transaction-time business, customer, product, tax, cost, tender, branch/register/device, and receipt presentation snapshots.
- [ ] Add crash-at-every-checkout-stage and lost-response desktop tests proving one atomic sale.
- [ ] Exercise offline sale, restart, receipt queueing, refund, void, and reconciliation end to end.

**Exit gate:** a cashier can perform the complete supported retail workflow without admin access, with authoritative totals calculated only in the trusted domain layer.

## Step 4 — Complete pricing, promotions, tax, and Bahrain localization

- [ ] Specify and implement deterministic precedence for base, branch, channel, effective-date, member, promotion, and manual-override prices.
- [ ] Implement exact rounding increments and configured preferred endings.
- [ ] Implement immutable price and cost history commands with source, scope, reason, approver, and effective dates.
- [ ] Implement margin floors, warnings, approval thresholds, and actionable alerts.
- [ ] Implement fixed/percentage discounts, promotional prices, quantity tiers, Buy-X-Get-Y, category, threshold, loyalty, and segment offers.
- [ ] Implement coupons with validity, limits, branch/customer/product/category restrictions, and minimum spend.
- [ ] Implement date/weekday/time/branch/channel/segment scheduling.
- [ ] Implement exclusive, stackable, best-price, and priority conflict modes with deterministic ordering tests.
- [ ] Implement pricing-policy recommendation preview and explicit approval; never auto-publish recommendations.
- [ ] Verify current Bahrain VAT, receipt, and record-retention requirements against authoritative NBR/legal sources and record effective dates.
- [ ] Add effective-dated Standard Rated, Zero Rated, Exempt, Out of Scope, and custom tax administration.
- [ ] Complete Bahrain CR/tax identity, +973/international phone, structured address, business-date, and local tender configuration.
- [ ] Complete Arabic/RTL application layouts and an explicit Arabic receipt shaping/rasterization strategy.
- [ ] Test mixed Arabic/English receipts on supported 58 mm and 80 mm devices.

**Exit gate:** price/tax results are deterministic and historically reproducible, and the supported Bahrain configuration has dated authoritative compliance evidence.

## Step 5 — Finish receipts, cash control, refunds, and void operations

- [ ] Add ESC/POS barcode/QR rendering and printer/profile configuration UI.
- [ ] Add printable X, Z, EOD, variance, and cashup reports using historical session evidence.
- [ ] Complete drawer hardware configuration and no-sale authorization/evidence.
- [ ] Implement petty cash as a separately reconciled fund with transfer evidence to/from drawers.
- [ ] Complete interrupted cash-session takeover/recovery and conflict UX.
- [ ] Add variance acknowledgement, investigation, escalation, notes, and resolution workflows.
- [ ] Finish refund tender-routing policy and provider-specific evidence.
- [ ] Add completed-sale void policy, deadlines, approvals, and multi-terminal conflict tests.
- [ ] Prove partial-refund quantity limits and duplicate replay under concurrent terminals.
- [ ] Validate RAW spooler, COM output, cutting, drawer pulse, retry, and original receipt reprint on Windows hardware.

**Exit gate:** opening through closing cash is fully reconcilable, and print/refund/void failures preserve the sale and complete through safe retry or compensation.

## Step 6 — Complete catalogue, assortment, bundles, and barcode administration

- [ ] Build full product/category/alternate-barcode CRUD through trusted scoped commands.
- [ ] Add configurable and testable weighted/scale barcode layouts and checksum policies.
- [ ] Add ghost-barcode review actions: assign, create product, dismiss, and audit.
- [ ] Add duplicate-product detection using barcode, SKU, normalized name, package/size, and corroborating attributes.
- [ ] Implement dependency-aware product merge preview and explicit execution without rewriting historical snapshots.
- [ ] Build branch assortment state and availability-schedule administration.
- [ ] Enforce all product/category date, weekday, and time-window schedules at final checkout.
- [ ] Implement sell-time bundles/kits/hampers, composition versions, packaging cost, pricing, and stock consumption.
- [ ] Prevent double component consumption between sale-time bundles and production-completion recipes.
- [ ] Add product images and controlled attachment storage/validation.

**Exit gate:** catalogue operations are safe at scale, barcode collisions/merges preserve evidence, and bundle stock behavior is unambiguous.

## Step 7 — Complete inventory operations and intelligence

- [ ] Add transfer cancellation rules and compensating evidence for each safe state.
- [ ] Complete discrepancy and damage-in-transit resolution workflows.
- [ ] Add FEFO recommendations and optional lot allocation without rewriting movement history.
- [ ] Add manager-controlled near-expiry temporary pricing.
- [ ] Add scanner-first stocktake UI with autosave, expected snapshot, review, and approval.
- [ ] Add controlled batch operations with stable operation IDs and partial-failure recovery.
- [ ] Implement valuation-at-date by product, category, branch, centre, and supplier.
- [ ] Define and implement sales velocity, days of cover, slow stock, dead stock, stockout risk, margin erosion, expiry, and shrinkage metrics.
- [ ] Implement reorder suggestions using available stock, velocity, lead time, open POs, rules, and branch demand.
- [ ] Keep suggestions non-authoritative until explicit requisition/PO approval.
- [ ] Add negative-stock/discrepancy reconciliation views and compensation actions.
- [ ] Exercise receiving, transfer, stocktake, waste, production, and rebuild conservation under concurrency and crash injection.

**Exit gate:** the append-only ledger reconstructs every balance/cost projection, and all inventory workflows conserve evidence under retry and interruption.

## Step 8 — Complete suppliers, purchasing, supplier finance, and expenses

- [ ] Build supplier, supplier-product, requisition, PO, receiving, return, invoice, payment, and statement administration UI.
- [ ] Add configurable approval thresholds and action-bound approvals for purchasing and supplier payments.
- [ ] Complete supplier reliability metrics using only evidenced data.
- [ ] Add secure invoice/payment/return attachments with type, size, malware, and authorization controls.
- [ ] Complete supplier aging, credit-note, replacement, and allocation reconciliation.
- [ ] Add cost-drift and PO/invoice/received-cost variance reports and alerts.
- [ ] Add expense attachment handling, thresholds, branch allocation, and approval administration.
- [ ] Add explicit cash-basis/accrual report options and definitions without implying a general ledger.
- [ ] Reconcile paid cash expenses with drawer or petty-cash fund events.
- [ ] Exercise partial receipt, over-delivery, rejection, substitution, cost mismatch, payment replay, and return compensation end to end.

**Exit gate:** purchasing through supplier settlement is exact, append-oriented, authorized, and traceable to stock and cash evidence.

## Step 9 — Complete customers, loyalty, credit, delivery, production, attendance, and alerts

- [ ] Build customer identity, canonical phone, Bahrain/international address, notes, history, and consent administration.
- [ ] Implement loyalty earn, redeem, refund, reversal, adjustment, expiry, tiers, member pricing, and promotional points.
- [ ] Add credit allocation, statements, aging buckets, overdue/available balance, account status, and terms UI.
- [ ] Enforce offline credit-risk policy and stale-balance reconciliation.
- [ ] Build cashier/admin/courier delivery workspaces with least-necessary rider data.
- [ ] Add delivery item/address snapshots, cancellation, return, refund compensation, and collection reconciliation.
- [ ] Finish courier cash custody handoff, shortage/overage investigation, and settlement UI.
- [ ] Build production planning/in-progress/completion UI and yield/component/waste variance reports.
- [ ] Build employee, branch assignment, schedules, attendance, missing clock-out, late/early, and report UI.
- [ ] Add overdue-customer-credit and settlement-discrepancy alert producers.
- [ ] Complete Alert Centre assignment, filtering, notes, escalation, history, and resolution reporting.

**Exit gate:** each ledger-backed operational domain has complete administration, compensation, reports, offline rules, and alerts.

## Step 10 — Implement sales channels and marketplace reconciliation

- [ ] Make channel mandatory and authoritative for every order/sale.
- [ ] Implement per-channel price/tax/tender/delivery/accounting policy.
- [ ] Build WhatsApp, Phone, In-House Delivery, Marketplace, and future Web Store order intake boundaries.
- [ ] Persist gross order value, commission, platform fee, merchant/platform discounts, delivery fee, tax, and expected settlement separately.
- [ ] Import or enter actual settlement evidence and reconcile it to expected amounts.
- [ ] Create explicit discrepancies; never silently net or overwrite components.
- [ ] Add marketplace statements, aging, reports, exports, and alert integration.
- [ ] Add idempotency and lost-response tests for order ingestion and settlement reconciliation.

**Exit gate:** channel economics reproduce exactly and every settlement difference has reviewable evidence.

## Step 11 — Finish synchronization, hub deployment, and fleet operations

- [ ] Implement a deployable authenticated hub/network adapter around the existing signed mutation protocol.
- [ ] Add TLS endpoint validation and production device/hub authentication.
- [ ] Implement reliable connectivity detection, batching, backoff, checkpoints, and bounded queue processing.
- [ ] Build the Sync Reconciliation Centre for apply, compensate, reject, resolve, and notes.
- [ ] Implement catalogue/config version and effective-date conflict policy.
- [ ] Route dangerous stale mutations to `REQUIRES_REVIEW` and stop infinite semantic retries.
- [ ] Preserve concurrent legitimate offline sales and surface oversell/negative-stock reconciliation.
- [ ] Build multi-terminal fleet health with version, heartbeat, active user/session, sync lag, printer, DB, and credential state.
- [ ] Add credential rotation/revocation tests across real processes and reconnect boundaries.
- [ ] Exercise lost hub response, duplicate delivery, reordered packets, restart, partition, and conflict resolution with multiple terminals.

**Exit gate:** real terminals synchronize through a deployed hub with replay-safe commits and human-reviewable unsafe conflicts.

## Step 12 — Build isolated optional integrations

- [ ] Implement the narrow authenticated WhatsApp sidecar IPC boundary with no database, shell, filesystem, or financial authority.
- [ ] Add QR pairing, encrypted/policy-compliant session persistence, reconnect, health, controlled disconnect, inbox/media, and outbound messaging.
- [ ] Make sidecar loss independent of sale commit and add outage/restart tests.
- [ ] Implement OCR ingestion for image/PDF evidence in an isolated provider boundary.
- [ ] Store source evidence, untrusted extraction, candidate matches, reviewer edits, approval, and final outcome.
- [ ] Implement payment-screenshot candidate/mismatch/review classification without claiming bank settlement.
- [ ] Implement deterministic AI tool registry, schemas, authorization, risk classification, preview/diff, confirmation, execution, audit, and compensation evidence.
- [ ] Add OpenAI/Anthropic-compatible provider adapters without exposing unrestricted SQL or commands.
- [ ] Build versioned document storage, archive/replacement, indexing state, and tenant/permission-scoped retrieval.
- [ ] Add provider timeout, malformed output, prompt-injection, data-minimization, permission, and disabled-module tests.

**Exit gate:** optional integrations are isolated, permission-scoped, failure-tolerant, and incapable of bypassing deterministic business rules.

## Step 13 — Complete imports, exports, background jobs, and feature flags

- [ ] Build CSV, XLSX, and supported legacy import mapping, validation, preview, row-error, apply, and audit stages.
- [ ] Implement conservative matching: barcode, unique exact SKU, corroborated identity, then manual-only fuzzy candidates.
- [ ] Preserve leading-zero barcodes/SKUs and exact BHD values.
- [ ] Sanitize spreadsheet formula prefixes while preserving business meaning.
- [ ] Build paginated/filterable exports and durable export jobs.
- [ ] Implement durable handlers for restore, import, export, OCR, AI, sync, and analytics.
- [ ] Add safe cancellation, resume/retry, interruption recovery, progress, and failure evidence to each handler.
- [ ] Implement feature-flag evaluation in trusted services and navigation.
- [ ] Prove every optional module can be disabled with no broken navigation, jobs, dependencies, or financial side effects.

**Exit gate:** large imports/exports and optional modules operate through recoverable jobs without freezing or destabilizing checkout.

## Step 14 — Complete reporting and store operations

- [ ] Define every metric, timestamp basis, business-date rule, refund/void treatment, and tax-inclusive/exclusive treatment.
- [ ] Build paginated/filterable reports for sales, tenders, tax, cashier, branch, product, category, and margin.
- [ ] Build inventory, movement, valuation, purchasing, supplier, expense, credit, loyalty, delivery, waste, shrinkage, production, and settlement reports.
- [ ] Build audit/security, attendance, terminal, sync, backup, and alert reports.
- [ ] Implement fast/slow/dead stock, margin erosion, stockout risk, supplier drift, refund/discount anomalies, affinity, segments, profitability, and time-trend analytics.
- [ ] Add opening checklist configuration and completion evidence.
- [ ] Build EOD orchestration for sessions, variances, courier collections, sync exceptions, backups, and alerts without requiring nightly closure.
- [ ] Add 24-hour-store and after-midnight prior-business-date tests.

**Exit gate:** every report is reproducible from immutable evidence and uses consistent Bahrain-local/business-date semantics.

## Step 15 — Complete security, backup, update, and release engineering

- [ ] Add encrypted backup format and OS-backed key custody, rotation, recovery, and restore tests.
- [ ] Define and implement safe external-backup import/quarantine policy.
- [ ] Add attachment/media validation, malware-scanning boundary, quotas, and retention.
- [ ] Add API/sidecar rate limits, request-size limits, timeouts, and abuse logging.
- [ ] Review CSP, navigation, protocol handlers, filesystem allow-lists, sidecar IPC, import paths, templates, OCR, AI, and media as untrusted inputs.
- [ ] Implement signed update discovery, download, signature verification, controlled install, release notes, restart, and rollback-safe failure behavior.
- [ ] Produce reproducible version/build SHA metadata and a SHA-256 artifact manifest.
- [ ] Generate CycloneDX or SPDX SBOM plus dependency/license/vulnerability reports.
- [ ] Configure Authenticode certificate custody and trusted timestamping.
- [ ] Verify installer and executable signatures; never label unsigned artifacts as signed.
- [ ] Add exact Git commit/build provenance attestations and release retention.

**Exit gate:** backups and updates have production key custody, and release artifacts are signed, attributable, auditable, and rollback-safe.

## Step 16 — Performance, failure injection, and complete invariant testing

- [ ] Generate representative datasets for 100,000+ products, 500,000+ sales, and millions of lines/movements.
- [ ] Benchmark barcode lookup, product search, cart mutation, checkout commit, reports, sync, and UI memory.
- [ ] Meet or explicitly remediate the target budgets: barcode <50 ms, search <150 ms, cart <100 ms, checkout <500 ms on supported hardware.
- [ ] Add query-plan/index regression checks, bounded pagination, UI virtualization, and incremental loading.
- [ ] Complete financial invariant coverage for tax, discount ordering, promotion conflict, split tenders, change, receipt uniqueness, refunds, voids, COGS, and ledgers.
- [ ] Complete inventory conservation, transfer, stale stocktake, valuation, production, and waste invariants.
- [ ] Complete authorization binding, tenant isolation, permission denial, device revocation, and approval replay tests across all commands.
- [ ] Inject database busy/locked, app crash, lost response, printer/sidecar/network failure, duplicate request, partial receiving, stale stocktake, sync restart, credential rotation, and background-job interruption.
- [ ] Prove recovery behavior and final evidence, not just error detection.
- [ ] Run migration upgrade/rollback-safe-failure tests from every supported prior schema version with realistic data and WAL state.

**Exit gate:** published benchmark and failure-recovery evidence passes on supported retail hardware and the CI invariant suite covers every financial/stock mutation.

## Step 17 — Clean Windows acceptance and production release

- [ ] Build the production Windows x64 installer in a controlled release environment.
- [ ] Install on a clean supported Windows machine with no development tools.
- [ ] Initialize a business, branch, register, terminal, users, roles, permissions, and hardware profiles.
- [ ] Import a large catalogue and validate leading zeros, duplicates, and errors.
- [ ] Scan/search products; complete offline cash/card/BenefitPay/split/credit sales as supported.
- [ ] Print, fail, retry, reprint, open drawer, refund, void, and verify historical receipts.
- [ ] Open/close/recover sessions; reconcile cash, petty cash, variances, X/Z/EOD, and courier cash.
- [ ] Receive/transfer/count/waste/produce stock and prove ledger/cache reconstruction.
- [ ] Exercise supplier, expense, customer credit, loyalty, delivery, marketplace, alert, and reporting workflows.
- [ ] Back up, simulate damage/interruption, restore, restart, and verify every authoritative ledger.
- [ ] Upgrade from the previous supported release and verify migrations/data preservation.
- [ ] Synchronize multiple terminals; exercise partition, lost response, duplicate replay, oversell, revocation, and reconciliation.
- [ ] Verify tenant isolation through real UI/command/hub paths.
- [ ] Uninstall/reinstall and verify intended business-data preservation.
- [ ] Verify signed updater success and rollback-safe invalid/tampered update failure.
- [ ] Verify Authenticode signature, timestamp, checksums, SBOM, provenance, version, and release notes.
- [ ] Record hardware/OS versions, exact artifact hashes, test evidence, defects, waivers, and approvals.

**Exit gate:** every acceptance item passes or is recorded as an explicit release-blocking failure. Only then may BHAIPOS be called production-ready.

## External inputs and currently unavoidable blockers

- [ ] A supported clean Windows x64 machine or CI runner for native Tauri, printer, Credential Manager, installer, update, crash, and recovery testing.
- [ ] Representative 58 mm/80 mm ESC/POS USB/spooler and serial/COM printers plus a cash drawer and barcode/scale devices.
- [ ] A production hub environment, TLS identity/certificates, and at least two enrolled terminals.
- [ ] Authoritative Bahrain NBR/legal review with an applicable effective date before any compliance claim.
- [ ] Approved provider accounts/test environments for payment verification, WhatsApp, OCR, and AI capabilities that will ship.
- [ ] Authenticode signing certificate, protected signing process, and trusted timestamp service.
- [ ] Product-owner decisions for configurable policies: offline risk, refund/void deadlines, approval thresholds, credit, promotions, rounding, retention, and supported hardware/Windows versions.

## Recommended execution order from this baseline

1. Step 1: diagnostics export/history and Windows backup/restore recovery.
2. Steps 2–5: close foundational authority and the complete sell/pay/cash/receipt/refund path.
3. Steps 6–10: complete catalogue, inventory, suppliers, customers/operations, and channel accounting.
4. Step 11: deploy and verify real multi-terminal synchronization.
5. Steps 12–14: isolated integrations, jobs/imports, and reproducible reporting.
6. Steps 15–16: security/release engineering, benchmarks, and full failure injection.
7. Step 17: clean Windows acceptance and signed production release.

The order may overlap where dependencies allow, but no later capability may bypass an unresolved money, inventory, authorization, idempotency, audit, offline, or tenant-isolation invariant.
