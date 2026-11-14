# ChimpPOS Stage 13B — Trading parties and account foundations

> **Superseded apply guidance:** the financial-integrity remediation is still
> under review and its migrations are unapplied. Do not run `sqlx migrate run`
> against `chimp_pos_dev`. Historical currency/session approvals and a
> disposable-database PostgreSQL test run are required first. See
> `STAGE13B_SCHEMA_HARDENING.md` and `STAGE13B_MIGRATION_REVIEW.md`.

## Included in this delivery

1. Migration `20261112100000_stage_13b_accounts_payables_receivables.sql`:
   - customer and supplier account payment tables and company-scoped sequences;
   - customer/supplier payment allocations;
   - supplier invoices and invoice lines (posting creates the payable; PO/receipt alone does not);
   - customer and supplier credit notes;
   - derived customer and supplier balance views;
   - cash-session references for cash account receipts/payments;
   - customer/supplier permissions.
2. Customer and supplier create/list/update APIs with company-scoped numbering and optimistic concurrency.
3. Sales draft requests can optionally carry `customer_id`.
4. Checkout now permits partial or zero payment only for an active customer with credit enabled, an authorized operator (`customers.credit`), and sufficient remaining credit limit. Anonymous/walk-in sales still require full payment.
5. Cash reconciliation/refund affordability formulas include customer account cash receipts and supplier account cash payments. These are counted from their source payment tables, not duplicated as manual cash movements.

## API routes

- `GET /api/v1/customers`
- `POST /api/v1/customers`
- `PUT /api/v1/customers/{id}`
- `PATCH /api/v1/customers/{id}/credit-policy` (requires `customers.credit`)
- `GET /api/v1/suppliers`
- `POST /api/v1/suppliers`
- `PUT /api/v1/suppliers/{id}`

Create customer example:

```json
{
  "name": "Jane Doe",
  "phone": "+263771234567"
}
```

Customer create/profile update does not accept credit enablement, credit limits,
or payment terms. Set these through the separate credit-policy endpoint with
`customers.credit`; cashiers cannot change credit policy through
`customers.profile.manage`.

Create supplier example:

```json
{
  "name": "Wholesale Supplier",
  "phone": "+263772222222",
  "payment_terms_days": 14
}
```

Update requests require `expected_version`, copied from the returned record. This prevents silently overwriting another user's edits.

## Sales changes

Create/update sale payloads accept optional `customer_id`. For example:

```json
{
  "location_id": 1,
  "customer_id": 12,
  "notes": "Customer account sale",
  "items": []
}
```

At checkout, `payments` may be less than the sale total, including an empty list, only if the customer is active, credit is enabled, the operator has `customers.credit`, and `current balance + new unpaid amount <= credit_limit`. A sale with no customer or a customer without credit must be paid in full. A payment total greater than the sale total is always rejected.

## Apply and verify

Run from the backend directory:

```bash
cargo fmt
cargo check
cargo test
sqlx migrate run
```

Apply migrations before using the new customer-linked sale flow. Verify the migration ledger records the Stage 13B migration. Never manually rerun a migration already recorded as applied.

## Acceptance checks

1. Create a customer and supplier; numbers should be sequential within each company.
2. Update each record using its current `version`; retry with the old version and confirm HTTP 409 `VERSION_CONFLICT`.
3. Confirm a customer or supplier ID from another company cannot be attached or updated.
4. Create an anonymous sale and attempt a partial checkout; it must be rejected.
5. Create a customer with credit disabled and attempt partial checkout; it must be rejected.
6. Enable credit and set a limit; an authorized operator can complete a partial/zero-paid sale within the limit.
7. Attempt a sale that would exceed the limit; it must be rejected.
8. Confirm a cashier without `customers.credit` cannot complete a credit sale.
9. Confirm normal fully paid sales and cash-session reconciliation retain their prior behaviour.
10. Confirm existing sales remain anonymous and all historical suppliers remain present.

## Important scope boundary

This is the first Stage 13B implementation slice, not the final accounts workflow. The database foundation is in place for account receipts, payment allocations, supplier invoice posting/payment workflows and credit-note posting, but dedicated HTTP handlers/services for those financial transactions are not included in this delivery. Do not insert directly into these financial tables from the mobile client. Those operations must be added as transactional backend services with allocation-total checks, idempotency replay validation, cash-session locking, and immutable posting rules before production use.

The archive was inspected and edited, but this environment does not have `cargo` or a PostgreSQL server/client available. Compilation and migration execution therefore remain unverified here; run the verification commands above on the development machine before applying to production.
