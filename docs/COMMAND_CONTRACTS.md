# Command Contract Pattern

Every mutating command inside the trusted domain layer must use a deterministic envelope. The renderer-facing Tauri request deliberately omits the authority fields below; the desktop service resolves them from the immutable local terminal binding and the revalidated authenticated session before constructing the domain request:

```text
CommandEnvelope {
  tenant_id
  branch_id
  device_id
  actor_user_id
  operation_id
  command_name
  payload
  expected_versions?       // optimistic concurrency where applicable
  manager_approval?        // only for policy-defined sensitive commands
  client_created_at        // evidence only; server/local authority timestamps commit
}
```

The backend execution order is fixed:

1. resolve tenant from authenticated context and compare with envelope;
2. authenticate device and credential version;
3. verify user/session state;
4. require the exact permission code;
5. validate branch/entity ownership;
6. check idempotency result before performing work;
7. validate payload schema and invariants;
8. validate/consume manager approval if required;
9. enter the required DB transaction mode;
10. reload authoritative data and recompute values;
11. write domain records and compensating/ledger effects;
12. append audit evidence;
13. enqueue sync/print/background work that must survive restart;
14. persist idempotent result;
15. commit once;
16. return the committed result.

Suggested explicit permissions include:

```text
sale.checkout
sale.hold
sale.discount.line
sale.discount.cart
sale.price_override
sale.refund
sale.void
cash.session.open
cash.session.close
cash.movement.paid_in
cash.movement.paid_out
cash.movement.safe_drop
cash.drawer.no_sale
inventory.receive
inventory.adjust
inventory.transfer.dispatch
inventory.transfer.receive
stocktake.create
stocktake.approve
purchase.create
purchase.approve
supplier.payment.create
expense.create
expense.approve
customer.credit.sale
customer.credit.payment
customer.credit.adjust
delivery.collect_payment
production.complete
device.enroll
device.rotate
device.suspend
device.revoke
backup.create
backup.restore
settings.write
manager.approve
```

Permissions are additive allow-list entries. No unknown permission code is inferred from role names.
