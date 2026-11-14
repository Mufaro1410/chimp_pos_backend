# Stage 13B PostgreSQL integration test fixtures

`stage13b_financial_integrity_pg.rs` connects only when both `STAGE13B_TEST_DATABASE_URL` and `STAGE13B_TEST_FIXTURES_JSON` are set. It checks `current_database()` and refuses to run unless its name starts with `chimp_pos_stage13b_test`. It does not run migrations, and it does not read the project's `.env` file.

The database must already contain both Stage 13B migrations and the explicit historical approvals needed for them to succeed. The test fixture JSON has this shape:

```json
{
  "company_id": "<company UUID>",
  "actor_id": "<active owner/admin user UUID>",
  "customer_id": 1,
  "customer_credit_attempt": "0.01",
  "customer_allocation_payment_id": 1,
  "customer_allocation_sale_a_id": 1,
  "customer_allocation_sale_b_id": 2,
  "supplier_allocation_payment_a_id": 1,
  "supplier_allocation_payment_b_id": 2,
  "supplier_allocation_invoice_id": 1,
  "customer_reversal_payment_id": 2,
  "customer_reversal_allocation_id": 1,
  "customer_rollback_allocation_id": 2,
  "supplier_reversal_payment_id": 3,
  "supplier_reversal_allocation_id": 3,
  "customer_credit_note_id": 1,
  "customer_cash_payment_id": 4,
  "cash_session_id": 1
}
```

Fixture requirements:

- The customer allocation payment has exactly `0.01` available, and both completed sale targets have at least `0.01` open in the same company/customer/currency.
- Supplier payment fixtures A and B each have at least `0.01` available; the shared posted invoice has exactly `0.01` open in their currency.
- The customer starts with credit enabled and a balance/limit such that one `customer_credit_attempt` is allowed but two concurrent attempts exceed the limit. A company sale sequence and active inventory location must exist.
- Customer and supplier reversal payments each have an active allocation of at least `0.01`. The rollback allocation is a separate unreversed allocation on the customer reversal payment. The customer credit note is at least `0.01`.
- The cash payment is unallocated, in company base currency, and can be reversed by `0.01`; the referenced session is open and has sufficient cash. The actor has `customers.payment`, `suppliers.payment`, `customers.credit`, and `cash_register.close`.

The suite writes test events and one credit sale and closes its configured cash session. Use a freshly provisioned disposable database and discard that database through the normal test-environment lifecycle after the run. Never point these variables at `chimp_pos_dev` or a database containing valuable data.

Run with:

```bash
STAGE13B_TEST_DATABASE_URL='postgres://…/chimp_pos_stage13b_test_run' \
STAGE13B_TEST_FIXTURES_JSON='{"company_id":"…"}' \
cargo test --test stage13b_financial_integrity_pg -- --nocapture
```

The abbreviated JSON above is illustrative only; provide every field and fixture described above. PostgreSQL race behavior remains unverified until this suite completes against a real disposable database.
