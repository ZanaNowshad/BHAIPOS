# BHAIPOS Architecture

## Topology

BHAIPOS uses a local-first authority hierarchy:

```text
Cashier/Admin UI (React)
        │ IPC only
        ▼
Tauri command boundary
        │ deterministic typed commands
        ▼
Authoritative Rust application/domain services
        │ single transactional writer
        ▼
SQLite WAL database on each authorized node
        │ durable mutation queue
        ▼
Authenticated branch/store hub (planned runtime)
        │ operation-ID replay + checkpoint/watermark
        ▼
Other trusted terminals / optional remote services
```

A terminal may continue locally while disconnected only for operations explicitly allowed by local policy. Connectivity never grants authority by itself. Device identity, user identity, permission, tenant, branch, operation identity and business invariants are independently validated.

## Bounded contexts

**Identity & Trust** — tenants, branches, users, roles, permissions, devices, credential versions, approvals, feature flags and secrets references.

**Commerce** — products, barcodes, branch assortment, pricing history, promotions, coupons, bundles, persistent carts, sales, lines, tenders, refunds, voids and immutable receipt snapshots.

**Cash** — branch → register → cash session → cashier, cash movements, petty cash, cash variance and EOD evidence.

**Inventory** — centres, stock cache, append-only movements, lots, expiry, transfers, stocktake, reconciliation, waste and valuation inputs.

**Procurement/AP** — supplier catalogue, requisitions, POs, receiving, discrepancies, invoices, payments, allocations, returns and supplier ledger.

**Customer** — directory, Bahrain addresses, loyalty ledger, tiers, credit accounts/ledger, payments and aging inputs.

**Fulfilment & Channels** — deliveries, courier custody, digital orders, sales channels and marketplace settlements.

**Production** — BOM/recipes, production orders, expected/actual usage, yield and waste.

**Operations** — employees, attendance, expenses, alerts, jobs, documents, backups, diagnostics and updates.

**External/AI** — WhatsApp sidecar metadata, OCR review records, payment evidence boundary, deterministic AI action runs, confirmations and undo metadata.

## Money and quantity

Authoritative BHD values use signed 64-bit integer fils. `1 BHD = 1000 fils`. Tax rates use basis points. Arithmetic expands intermediate products to `i128` and rounds deterministically.

Sell quantities use thousandths of one sell unit. `1000 = 1.000`. That supports `0.750 kg` as `750` without floating-point inventory or pricing arithmetic.

## Tenant isolation

Application commands receive a tenant identity and never infer it from user-provided entity IDs. Core command paths compare tenant/branch/device/user/cart/sale scope before commit. The database adds cross-tenant guard triggers on high-value relationships. IDs are durable UUIDs; tenant identity is still required even when IDs are globally collision-resistant.

The next expansion step is to apply the same guard pattern to every remaining mutable back-office table before enabling its write API.

## Authorization

Permissions are allow-listed strings such as `sale.checkout`, `sale.refund` and `cash.session.open`. Absence of a permission denies the command. There is no default permission fallback.

Sensitive manager approval is separate from login permission. An approval signature binds tenant, approver, device, operation ID, action, entity, payload SHA-256, nonce and expiry. Consuming the nonce persists it so a valid token cannot be replayed.

## Checkout transaction

Checkout opens a SQLite `BEGIN IMMEDIATE` transaction, then:

1. authenticates active device scope;
2. authenticates active user scope and explicit permission;
3. verifies cart tenant/branch/device/user/status;
4. reloads authoritative line snapshots from the DB;
5. computes line totals and tax using integer arithmetic;
6. validates tender allocations exactly equal total;
7. validates open register cash session if cash is present;
8. allocates an atomic branch/business-date receipt sequence;
9. inserts sale and immutable sale lines;
10. applies stock movements/cache changes;
11. records payment evidence semantics;
12. stores immutable receipt text and SHA-256;
13. queues the print job;
14. completes the cart;
15. appends hash-chained audit evidence;
16. queues the sync mutation;
17. stores idempotent result;
18. commits once.

A replay with the same operation ID and canonical request digest returns the committed result instead of creating a second sale. Reuse with a changed payload or a different mutation action fails closed as an idempotency conflict.

## Sync model

The schema contains durable queue states: `PENDING → SENDING → RETRYING → COMMITTED`, with `FAILED → REQUIRES_REVIEW → RESOLVED` for unsafe cases.

The planned hub protocol must distinguish mergeable metadata from immutable operational records. Financial, stock, cash, supplier/customer ledger and approval events are append/compensate semantics—not last-write-wins. Lost responses are recovered by replaying the same operation ID.

## External sidecars

WhatsApp, OCR and AI are explicitly outside the core financial commit. Their crashes, disconnects or provider outages cannot roll back or define a sale. Payment screenshots are evidence only and must never set authoritative settlement without an approved provider integration.


## Cash/EOD additive upgrade

`0004_cash_eod.sql` adds refund-session attribution, sale-void tender effects, cash-session indexes, and immutable tenant guards. Additive refund columns are applied only when absent so reopening an existing database is safe.

`0005_idempotency_payload_binding.sql` adds one immutable operation identity per tenant, binding action and canonical SHA-256 request digest. A result cannot be inserted without its matching binding. Historical unbound results are preserved, but their replay requires manual review because payload equality cannot be proven retrospectively.

`0008_receipt_reprint_permission.sql` introduces an explicit `receipt.reprint` capability and idempotently grants it to existing owner/administrator roles. Failed-job inspection and requeue remain tenant/branch/device scoped and use historical receipt snapshots.

`0009_print_leases_and_profiles.sql` separates ephemeral worker leases from receipt evidence. Every claim receives a new opaque token; completion requires the current token, and interrupted leases become pending after a bounded timeout. A crash after the operating system accepts bytes but before BHAIPOS records success can cause a duplicate physical receipt on recovery—printing is intentionally at-least-once, while the sale remains exactly-once. Operators can identify the original receipt number and attempt count.

Receipt bytes are built only from the immutable snapshot after SHA-256 verification. Printer profiles explicitly select 58/80mm width, RAW Windows spooler or serial/COM transport, encoding, cut mode and cash-sale drawer pulse. ASCII profiles reject Arabic/non-ASCII text rather than emitting corrupted evidence; UTF-8 must be enabled only for a printer whose Arabic shaping/code-page behavior has been validated.

## Trusted terminal synchronization

`0010_sync_protocol.sql` records single-use enrollment grants/consumptions, opaque delivery leases, durable retry schedules/errors, immutable hub mutation evidence, per-device watermarks and append-only conflict resolutions. Terminal secrets are never exposed to the renderer: the supported Windows desktop writes and reads them through Credential Manager.

Every envelope binds tenant, branch, device, credential version, mutation/operation identity, entity identity, payload and SHA-256 digest under HMAC. The hub authenticates current device state and credential version before accepting it. Replaying the same mutation and material returns its original sequence/state; changing material under the same mutation ID fails. Immutable financial/stock event classes append idempotently. Unknown or unsafe mutation classes become `REQUIRES_REVIEW`; a manager records one `APPLY`, `COMPENSATE` or `REJECT` resolution with notes and audit evidence.
