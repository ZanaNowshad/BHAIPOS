# BHAIPOS Offline Authorization Policy

Status: implemented foundation with partial command integration. This document describes the authoritative behavior at the trusted desktop boundary; it is not a claim that remote hub reconciliation is complete.

## Authority and evidence

- Policy identity is tenant- and branch-scoped. The terminal, branch and user come from authenticated local runtime state, never renderer input.
- Every activation creates a new immutable version and retires the previous active version. Activation is payload-bound and idempotent.
- A device checkpoint binds a policy version, synchronization time and validity deadline to one durable device identity.
- Every action is explicitly `ALLOW`, `REQUIRE_MANAGER_APPROVAL`, or `DENY`. Missing and unknown actions fail closed.
- The effective offline age is the smaller of the policy staleness limit and the action limit. A zero action limit never authorizes the action offline.

## Login and recovery

PIN verification establishes identity but does not by itself grant an operational session. The trusted service evaluates the `LOGIN` rule after revalidating the local user, branch and device.

- An active `ALLOW` rule creates a normal session.
- An explicit `DENY`, `REQUIRE_MANAGER_APPROVAL`, not-yet-effective policy, inactive/revoked device, invalid local session, or failed PIN denies login.
- Missing, expired or stale policy evidence may create a restricted recovery session only when the authenticated user currently has both `offline_policy.view` and `offline_policy.manage`.
- A recovery session can inspect/activate policy and log out. Selling, shifts, cash, refunds, diagnostics, backups and all other operational commands remain locked.
- After activation, the user must lock the terminal and sign in again. Policy activation does not silently elevate the restricted session.

This recovery path prevents an expired standalone terminal from becoming permanently unmanageable without converting policy recovery into transaction authority.

## Current protected actions

The desktop boundary reevaluates policy immediately before these operations:

| Action | Protected workflow |
|---|---|
| `LOGIN` | Local disconnected sign-in |
| `SALE_CHECKOUT` | Final sale commit |
| `CUSTOMER_CREDIT` | Checkout containing a customer-credit tender |
| `REFUND` | Refund commit |
| `CASH_EVENT` | Paid-in, paid-out, safe-drop, no-sale and configured cash movement commit |

`REQUIRE_MANAGER_APPROVAL` currently rejects the protected mutation unless that workflow has integrated a payload-bound single-use manager approval. It never degrades to `ALLOW`. Manager approval integration for refund, cash events and the remaining high-risk actions is still required.

## Disconnected-state limitations

A physically disconnected terminal cannot learn an immediate remote suspension, role change, device revocation, price change, credit balance, refund performed elsewhere or policy replacement. BHAIPOS therefore bounds locally cached authority by version, synchronization time, action age and expiry. Legitimate evidence accepted within those bounds must be preserved and reconciled after reconnect; it must not be deleted because remote state later differs.

Remote policy refresh, hub revocation propagation, stale-action reconciliation outcomes and multi-terminal Windows acceptance remain release gates.
