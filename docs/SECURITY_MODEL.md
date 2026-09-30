# BHAIPOS Security Model

## Trust assertions

A protected command is authorized only when all required assertions succeed: tenant scope, branch scope, active user, explicit role permission, active trusted device, entity ownership, command invariants and idempotency identity. Sensitive actions add a single-use manager approval bound to the exact payload.

Unknown actions are denied. No role inherits a catch-all permission by accident.

## Device identity

Device UUID identity is durable. A single immutable local binding selects the installation's tenant, branch, device and register, so synced device rows cannot be mistaken for local authority. Credentials are Argon2id hashes and have a monotonically increasing credential version; plaintext local credentials are held through Windows Credential Manager and are not returned to the renderer. Rotation changes the credential but not the device identity or historical transaction references. Revocation blocks protected command and hub mutation submission and cannot be reversed by the generic status setter; re-enrollment uses an expiring, single-consumption grant.

Production still needs Windows secure-storage integration for the plaintext client-side device secret. It must not be stored in application JSON or SQLite.

## Tauri boundary

The desktop shell has a minimal capability file, strict CSP, no shell plugin, no arbitrary process execution, and no broad filesystem permission. Its explicit command allow-list uses typed renderer DTOs that contain business payload and stable operation/entity IDs but no tenant, branch, device or user authority. Authority comes from the local binding and an offline-authenticated session, both revalidated for protected calls. Future sidecars must have separately scoped local authentication and cannot inherit financial command authority.

## Financial integrity

Money uses integer fils. Intermediate multiplication uses `i128`; aggregation, change, expected cash and variance use checked `i64` operations. SQLite triggers reject REAL/TEXT financial values and invalid tax domains even when writes bypass Rust. Checkout recalculates totals from DB-authoritative line snapshots. Applied tender amounts must exactly equal the sale total. Sale finalization is one DB transaction. Operation IDs are persisted with their action and canonical SHA-256 request digest; only an identical retry receives the committed result. Changed-payload and cross-action reuse fail closed.

Immutable DB triggers protect sale lines, payments, receipt snapshots, refunds, stock movements, cash movements, audit events, supplier/customer/loyalty ledgers and approval consumption records from normal update/delete correction. Corrections use explicit compensating records.

## Audit integrity

Audit records form a SHA-256 chain per tenant/device. Each hash covers tenant, device, actor, event type, entity type/id, payload, timestamp and previous hash. Verification follows those links rather than inferring insertion order from timestamps or UUIDs, and detects forks, cycles, dangling segments and hash changes. Database immutability prevents ordinary mutation; protected event append fails closed if the existing chain is inconsistent.

## Payment evidence

BenefitPay references/screenshots are recorded as evidence. OCR matching cannot set bank-settled state. Only an approved payment-provider/bank integration may produce authoritative settlement confirmation.

## Remaining security work

Remaining hardening includes deployed TLS hub transport/mutual endpoint authentication, secure backup encryption/key custody, update signature verification, sidecar authentication, attachment malware handling, OCR/AI data minimization, rate limits, session timeouts, admin approval thresholds, encrypted sensitive PII fields where required, signed release provenance, dependency/SBOM scanning and Windows hardening. Credential Manager and signed mutation material are implemented boundaries but still require Windows and multi-process penetration testing.
