# Stage 13B Financial Reversal Lifecycle

Status: schema and Rust transactional reversal service implemented locally; no account posting/reversal HTTP routes exist and no migration has been applied.

## Event model

- A customer receipt or supplier payment is a positive, immutable source posting.
- A payment reversal is a positive, immutable event that reduces the net payment. It references the original payment and copies company, party, and currency. The actual reversal method/session describe where the returned/received cash moved.
- An allocation reversal is a positive, immutable event against one original allocation. It restores the target sale/invoice open amount and the payment's unallocated amount. It does not move cash.
- A credit-note reversal is a positive, immutable event against one original note. It restores the account balance and does not move cash.
- Net allocation = allocations minus allocation reversals. Net payment = payments minus payment reversals. Net credit = credit notes minus credit-note reversals. No balance is stored or manually maintained.

## Required transaction sequence

Every service operation must use a single database transaction, persist an idempotency fingerprint, and roll back all child events if any operation fails.

1. Check an existing idempotency key. Same key and same canonical request fingerprint returns the original event; the same key with a different fingerprint returns a conflict.
2. Lock the party row (`customers` or `suppliers`).
3. Lock the source payment row, then affected target documents sorted by ID, then allocation rows sorted by ID.
4. For a payment reversal, insert all required allocation reversals before inserting the payment reversal. The payment trigger permits reversing only the original unallocated amount plus allocation amounts already reversed. A caller cannot return cash while leaving the corresponding allocation active.
5. For cash, lock the actual open cash session after party/payment/document locks. The session trigger verifies company and base currency. The reversal event itself records the session used for the physical cash movement.
6. Insert the reversal event(s), then commit. Any cap, permission, idempotency, FK, cash-session, or database error aborts the entire transaction.

Credit-note reversals lock party, source sale/invoice, and source note in that order. They have no cash-session field because they do not represent a cash movement.

## Global lock order

`party -> payment -> target document(s), sorted by ID -> allocation row(s), sorted by ID -> cash session`

For credit-note reversal: `party -> source document -> credit note -> cash session (not used)`. Checkout locks its customer before the sale, and reads the derived customer balance after acquiring that lock in a separate SQL statement. Services that cannot honor this order must not run concurrently with account posting until they are changed.

Database triggers enforce event caps and session state as a final guard. Rust services acquire parent locks in separate statements before computing remaining amounts so a post-wait READ COMMITTED query sees committed work. The opt-in PostgreSQL integration suite is intended to prove these races; it has not been run in this environment, so concurrency safety remains unverified.

## Balance and cash signs

| Event | Customer/supplier account balance | Cash session |
|---|---|---|
| Customer receipt | Reduces receivable | Cash inflow `+` |
| Customer receipt reversal | Restores receivable `+` | Cash outflow `-` if actual method is cash |
| Supplier payment | Reduces payable | Cash outflow `-` |
| Supplier payment reversal | Restores payable `+` | Cash inflow `+` if actual method is cash |
| Allocation reversal | No party balance change by itself | No cash movement |
| Credit-note reversal | Restores receivable/payable `+` | No cash movement |

Cash-register expected totals include each cash reversal exactly once from its reversal table. Do not add an equivalent `cash_movements` row. Noncash reversals have no cash-session attribution. A cash reversal outside a register requires a separately approved off-register process and cannot be assigned to a closed or unrelated session.

## Credit notes and refunds

- Customer credit notes adjust receivables; their reversals restore the balance. A sale return paid as cash uses the existing immutable `sale_refunds` event. Do not also issue a credit note for the same returned value.
- Supplier credit notes adjust the payable; their reversals restore it. Corrections to a posted supplier invoice use credit notes, not invoice edits.
- A payment reversal does not alter the original payment or credit note. The linked source rows remain available for audit.

## Idempotency and immutability

Each reversal table has a company-scoped unique idempotency key and a required request fingerprint. The Rust service fingerprints amount, method, session, reason, and sorted allocation reversal inputs. The trigger protects original allocation rows and all reversal rows from update/delete. Original payment and credit-note rows remain protected by the migration's immutability trigger.

## Unimplemented / not yet enabled

- No customer receipt, supplier payment, allocation, credit-note, or reversal route is exposed yet.
- Allocation creation services are not implemented; future writers must follow the global lock order and same idempotency contract.
- The reversal service must receive successful PostgreSQL integration testing on a dedicated disposable database after historical approvals are recorded and the revised migrations are applied there.
- The cash close and refund-affordability queries include reversal signs in source, but those paths are not PostgreSQL-tested yet.
