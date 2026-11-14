# Stage 13B Database Foundation Validation

Date: 2026-11-12  
Scope: static review of all migrations in timestamp order and comparison with current service code, plus an attempted clean-chain run on the disposable database. No database was modified. Credentials were not printed.

## Clean database migration result

**Not run: PostgreSQL was still unreachable at the time of this validation attempt.** `.env` was loaded into a subprocess without printing its values. Its `DATABASE_URL` targets `localhost:5432/chimp_pos_dev`. Although the user reported that PostgreSQL 16 `main` was running, the observed `pg_lsclusters` output reports that cluster on port 5432 as `down`; `pg_isready -h localhost -p 5432` reports no response. The read-only connection query to `chimp_pos_dev` did not run successfully (`pg_isready` exit status 2). The validation database existence could not be checked, so it was not created and no migration command was run. The earlier attempt to start the cluster failed because `pg_ctlcluster` requires the cluster owner or root, and `sudo -n` reported that a password is required. `chimp_pos_dev` was not modified. The clean-chain execution result remains unverified; this is not a successful migration test.

## Migration-by-migration findings

| Migration | Finding |
|---|---|
| `20261103080748_initial_schema.sql` | Creates company and user roots. User identity indexes are scoped by company. Adequate foundation; no database-level auth policy. |
| `20261104063217_add_rbac.sql` | Adds global permissions and company roles/user-role links. Composite user/role company constraints are sound. `role_permissions` references role ID alone, which is safe only because role-permission rows inherit the role's company and permissions are global. |
| `20261104082230_add_default_role_support.sql` | Adds optional `role_key` and a company-scoped unique partial index. It does not reserve built-in role keys for system roles; this matters to the Stage 13B grants. |
| `20261104083337_seed_default_roles.sql` | Seeds system roles with `is_system_role=TRUE` and grants permission sets. Chronologically after role support, as required. |
| `20261105130108_create_products.sql` | Adds company-owned products, nonnegative `NUMERIC(19,4)` price, and company-scoped name uniqueness. |
| `20261105150953_inventory_foundation.sql` | Adds units, categories, variants, locations, stock balances/movements, suppliers and supplier-products. Many company-bound FKs and decimal checks are present. The movement idempotency index is partial and company-scoped. |
| `20261106055432_create_purchasing.sql` | Adds PO and goods-receipt schema with company-scoped relationships, numeric quantity/cost checks and receipt idempotency. Receipt detail must also match order supplier/variant semantics in services. |
| `20261106062106_add_purchasing_permissions.sql` | Adds permissions and grants restricted to system roles. |
| `20261106064404_add_goods_receipt_sequences.sql` | Adds the receipt sequence after purchasing tables; ordering is consistent. |
| `20261106091750_create_sales.sql` | Adds sales/items/payments, decimal totals, checkout idempotency and role grants. |
| `20261107155928_sales_returns_refunds_voids.sql` | Adds returns, return lines, voids and refunds with tenant-scoped parents and idempotency uniqueness. The return-line FK to `sale_item_id` is not composite/company-scoped; currently service code validates sale items under a locked sale, but the DB alone cannot prevent a line referencing another tenant's sale item. |
| `20261108094955_cash_registers_and_sessions.sql` | Adds registers/sessions/movements and session links. Partial unique index enforces one open session per register. Cash-session checks prohibit sessions on noncash rows but do not require a session on cash rows; current services supply this invariant. |
| `20261110100000_reconcile_cash_register_permissions.sql` | Repairs grants using stable system role keys and active-state checks; ordering follows cash permission creation. |
| `20261111110000_stage_13a_trading_parties.sql` | Creates customer number sequence/table, numbers existing suppliers, creates supplier sequence and adds optional company-scoped `sales.customer_id`. Existing sales remain anonymous. Schema uses `NUMERIC(19,4)` for credit limits and positive/version/payment-term checks. |
| `20261112100000_stage_13b_accounts_payables_receivables.sql` | Creates customer/supplier payments, allocations, supplier invoices/lines, credit notes, balance views and permission grants. Object ordering is broadly valid because all referenced tables exist in earlier migrations. Important issues are detailed below. |

## Stage 13A findings

- Customer/supplier IDs and numbers are scoped to company; `sales(customer_id, company_id)` references `customers(id, company_id)`, correctly preventing another company's customer from being assigned to a sale.
- Sequence initialization is safe for existing rows. Stage 13B repeats customer/supplier sequence initialization idempotently; supplier sequence uses `GREATEST` and does not move an existing sequence backwards.
- Credit-limit fields have nonnegative checks, but DB does not relate `credit_enabled` to a positive limit; zero credit with enabled flag is legal, which is acceptable if service rejects any unpaid amount.
- No database uniqueness policy exists for customer email/phone; the migration adds lookup indexes only. Whether duplicate contacts are valid is a product decision.
- Sales association is optional by design; current checkout service checks customer active/credit settings and serializes customer-level credit use with a row lock. Database constraints alone do not enforce this business rule.

## Stage 13B findings

### Account-balance views

- `customer_account_balances` totals completed customer-linked sales, subtracts their sale payments, then subtracts every `customer_payments.amount` and customer credit note. `supplier_account_balances` totals posted supplier invoices, subtracts every supplier payment and supplier credit note.
- The views are company-correlated and avoid multiplying totals through joins. Their arithmetic has a clear sign convention: positive means owed to the business for customers and owed by the business for suppliers.
- Allocation tables are not used in either view. Thus payment allocations are currently informational and do not determine per-sale/per-invoice open balances or aging; the views only yield a party-wide net balance. This needs to be explicit in API semantics.
- Payments are deducted in full even when unallocated, while allocations may be partial. That can be appropriate for party-level prepayments, but no unapplied-credit balance is separately represented.
- No refund/return events enter the customer view directly. The migration comment expects a credit note for account adjustments. Cash refund against a cash sale must not reduce a receivable; for a credit sale return, the workflow must issue the proper account credit. Existing returns/refunds do not automatically create these notes.
- Supplier invoice amounts are aggregated only when `status='posted'`; this is the correct liability boundary. But aggregate consistency between posted invoice header and lines is not enforced.

### Payment allocations

- Payment and target FKs include `company_id`, so an allocation cannot point to a payment in another company or a sale/invoice in another company.
- Missing relationship constraints: a customer payment can be allocated to a sale belonging to a different customer in the same company; a supplier payment can be allocated to an invoice belonging to a different supplier in the same company.
- No constraints prevent allocations to draft/cancelled sales or draft/void supplier invoices.
- No constraint or trigger ensures sum of allocations is at most the payment amount, or at most the remaining sale/invoice balance. Unique `(payment_id, target_id)` prevents duplicate pairs only.
- The APIs/services must lock the payment and target records, validate same party and eligible state, and check aggregate allocations under concurrency in one transaction. If database-side enforcement is desired, use carefully designed triggers or allocation ledger semantics; row CHECK constraints cannot safely enforce cross-row sums.

### Supplier invoices and invoice lines

- Header FKs keep invoice supplier and purchase order in the same company, but do not ensure that the selected PO belongs to that same supplier.
- `supplier_invoice_items.purchase_order_item_id` and `goods_receipt_item_id` each reference only globally unique item IDs. The line's `company_id` is not part of those foreign keys, so a line for one company's invoice can point to another company's PO item or receipt item. This is a concrete tenant-integrity gap.
- The line references are independently nullable with no check requiring both null (unmatched invoice line) or valid/consistent references. A receipt item can belong to a different PO than `purchase_order_id`, or line PO item and receipt item can disagree.
- No check ensures PO item/receipt item variant and supplier match invoice, receipt quantity is not over-invoiced, or invoice line quantities/costs reconcile to received quantities. Some may be legitimate matching exceptions, but the schema should capture intended rules.
- `line_total` is only constrained nonnegative; there is no formula tying it to quantity, unit cost, tax and discount, or any exact rounding rule.
- Header `total_amount = subtotal + tax_total - discount_total + additional_charges` is enforced and amounts are nonnegative, but discount is not constrained to be at most subtotal. A sufficiently large additional charge can make the total nonnegative despite an excessive discount.
- Nothing ensures header subtotal/tax/discount/charges equal sums of lines. A posted invoice can therefore disagree with its detail rows if the service is incorrect or data is inserted outside it.
- Invoice number is unique per company and supplier, but there is no nonempty trimmed invoice number check. `currency_code CHAR(3)` has no ISO format/currency reference and company currency compatibility is not constrained. Payments have no currency field; party balances can combine invoices of different currencies.
- Status checks permit `draft`, `posted`, `void` and the posted metadata shape is checked. The database does not enforce allowed state transitions, immutability after posting, or that voiding is represented by an auditable reversal rather than editing the row.

### Idempotency and audit fields

- Customer and supplier payment rows have company-scoped unique idempotency keys and nonempty request fingerprints; supplier invoices do too. This is a good uniqueness base.
- Credit note tables have company-scoped unique idempotency keys but no request fingerprint, so a retry using a reused key with different amount/reason/party cannot be distinguished at the database or by a uniform stored replay contract.
- Allocation rows do not have an idempotency key. This may be safe only when allocation creation is strictly part of the parent payment transaction and whole-request retries replay the original payment/allocation response; separate allocation endpoints need their own replay semantics.
- A uniqueness constraint prevents duplicate keys but cannot itself compare request payloads. Service code must lock/check the key and compare canonical fingerprints before returning an existing result.
- Actor and timestamps exist on payment, invoice and credit-note records, but no general audit-event history exists. Posted immutability is only a migration comment/application policy, not DB-enforced.

### Stage 13B permission grants

The final inserts grant financial permissions by `roles.role_key` only. Earlier schema permits a non-system custom role to set a built-in key (`role_key` is optional for custom roles and the system-role check is one-way). A custom role with key `manager` or `cashier` can therefore receive these grants. Earlier permission migrations generally filter `is_system_role=TRUE` and often `is_active=TRUE`. Stage 13B should use both predicates. The permission keys themselves collide intentionally with existing customer/supplier view/manage keys; `ON CONFLICT(permission_key) DO NOTHING` handles these same-key rows.

## Tenant-isolation findings

- Strong pattern in most prior tables: company-scoped composite uniqueness and foreign keys pair entity ID with company ID. Stage 13A customer-to-sale and Stage 13B payment-to-parent/account relations mostly follow this.
- Concrete Stage 13B gap: invoice-line FKs to purchase-order and goods-receipt items are single-column. Add `(id, company_id)` unique keys to both referenced item tables if absent, then composite FKs from invoice lines. The referenced item schema must be inspected in the live migration definitions before writing the corrective migration.
- Business-tenant mismatches remain possible within one company: payment/allocation party mismatch, invoice/PO supplier mismatch, invoice line/receipt/PO mismatch, credit note party/source mismatch. Company FKs alone do not catch these.
- No PostgreSQL row-level security is used. Tenant isolation depends on service predicates plus constraints. Any new account reads must always include company ID even if IDs are globally generated.

## Missing or weak database constraints

1. Composite company foreign keys on invoice-line references to PO and goods-receipt items.
2. Same-customer/sale and same-supplier/invoice invariants for allocations.
3. Same supplier between invoice and PO; consistent invoice-line PO/receipt references and party/source match.
4. Cross-row allocation ceilings and eligible target statuses, enforced transactionally (and optionally with triggers where appropriate).
5. Invoice header-to-lines arithmetic and line formula/rounding checks, or a single authoritative server-calculated posting snapshot protected after posting.
6. Discount not exceeding subtotal, trimmed nonempty invoice number, currency validation and explicit multi-currency balance semantics.
7. Database-enforced financial document immutability/state transition policy or strictly controlled permissions and append-only reversal records.
8. Fingerprints for customer/supplier credit notes; explicit allocation idempotency if allocations are separately mutable.
9. Cash method/session invariant requiring an open session for cash payments at the moment of posting. Existing service handles this, but the schema only enforces the inverse (noncash must have no session).
10. Guard Stage 13B built-in role grants with `is_system_role` and `is_active`.

## Financial integrity risks

- Wrong-party allocations can misstate individual account aging while the aggregate views conceal the mismatch.
- Over-allocation or overpayment is permitted by the schema; aggregate balance views may go negative and cannot distinguish credit balance from unpaid invoices.
- Header/line disagreement or mutation after posting can create payable balances that do not reconcile to invoice detail or source receipt records.
- Currency is not modeled consistently across payments and account views; totals from different currencies could be netted as if equivalent.
- The view is a net balance, not an aged open-item ledger. Payment allocation semantics must be finalized before exposing “amount due per invoice/sale”.
- Customer credit-limit correctness depends on every operation affecting the view coordinating on the same customer lock. Payment/credit-note services must observe that lock discipline, and checkout must be tested under concurrent transactions.
- Cash session formulas in `cash_register_service.rs` and `sales_returns_service.rs` count source cash customer receipts/supplier payments by session; new write services must attach cash-session IDs only to cash movements and serialize with session close.

## Required corrective migrations

No migrations were created or modified in this audit. Before account writes are exposed, prepare one or more additive migrations after confirming the migration chain on disposable PostgreSQL:

1. Add company-composite unique keys to PO and goods-receipt item tables as needed; replace Stage 13B's invoice-line item references with composite `(item_id, company_id)` FKs.
2. Add party-consistency constraints for allocation/payment targets and invoice/PO supplier relationships. For line/receipt/PO consistency that spans multiple tables, validate and lock in service transactions or use carefully scoped triggers.
3. Add invoice-line formula and header reconciliation strategy; constrain discount and nonempty invoice number; define and constrain currency semantics.
4. Add durable correction/immutability semantics for posted invoices/payments/credit notes, ideally append-only adjustments rather than updates/deletes.
5. Add fingerprints to credit notes and define replay semantics for allocations.
6. Correct financial permission seeding for active system roles. Review existing role grants before revoking anything; role-permission rows do not record grant provenance.
7. Consider a cash-session constraint/workflow safeguard requiring `cash_session_id` for cash account payments, while preserving the ability to record noncash methods without a session.

The exact migration names should be assigned when implementation begins. Do not edit or rerun the already timestamped migration if it has been applied anywhere; corrective work must be a new forward migration.

## Recommended API transaction design

1. Begin one PostgreSQL transaction per financial command. Resolve authenticated user/company and require the domain permission inside the transaction.
2. Acquire a transaction-scoped advisory lock for `(company_id, idempotency_key)`, then query the existing record. If present, compare the stored canonical request fingerprint and return the original result only when it matches; otherwise return idempotency conflict.
3. Lock party and source documents in a consistent order: customer/supplier first, then sale/invoice/payment, then target lines/allocations, then cash session. This prevents credit-limit, balance and allocation races and reduces deadlocks.
4. Validate all entity ownership using company predicates and validate same-party relationships, document state, remaining amount, currency, cash-session openness and permitted allocation targets.
5. Compute all money with Rust `Decimal` and a documented `NUMERIC(19,4)` rounding policy; calculate totals server-side. Reject excess precision instead of silently relying on PostgreSQL coercion.
6. Allocate payment/document numbers by atomic sequence-row update (`UPDATE ... SET next_number=next_number+1 ... RETURNING`) within the same transaction.
7. Insert immutable financial event/header plus lines/allocations and audit event atomically. For cash, link the open session and rely on source-payment records for reconciliation; do not create duplicate manual movements.
8. Ensure aggregate allocation checks serialize on the parent payment and invoice/sale rows. Enforce “allocated <= payment amount” and “allocated <= open balance” before insertion.
9. Commit before returning; all errors roll back the entire command. Keep list/read APIs company-scoped and paginated. Add integration tests for replay/mismatch, rollback, cross-company references, same-company wrong-party references, concurrent over-allocation, credit-limit races and session close races.

## Verification status

- Static chronological inspection: **complete** for all 15 migration files.
- Clean database migration chain: **not run**; PostgreSQL 16 cluster `main` was observed down and unreachable during the latest attempt. The disposable database could not be checked or created.
- Existing database: **not connected to or modified**.
- Migration files: **not modified**.
- Financial endpoints/services: **not implemented**.
