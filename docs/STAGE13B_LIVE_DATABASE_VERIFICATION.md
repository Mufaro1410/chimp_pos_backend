# Stage 13B Live Database Verification

Date: 2026-11-12  
Target: existing `chimp_pos_dev` database  
Method: read-only PostgreSQL catalog and aggregate queries, using the project connection configuration without printing it. No DDL, migrations, or data changes were made.

## Result

- SQLx history contains all 15 expected migration versions, all marked successful, through `20261112100000` (`stage 13b accounts payables receivables`).
- All 15 stored SQLx checksums match the corresponding local migration files. The Stage 13A and 13B catalog objects inspected below match those migrations.
- Stage 13A/13B tables, columns, indexes, foreign keys, checks, and both account-balance views are present. No missing Stage 13A/13B object was found in the inspected catalog.
- The existing purchasing, sales, return/refund/void, and cash-register tables are present. Inspected tenant-scoped FKs and register/session checks are validated.
- Both views execute. The customer view currently returns 0 rows and the supplier view 1 row.
- The checked data anomaly counts were zero: cross-company invoice-line references; wrong-party customer/supplier allocations; invoice/PO supplier mismatch; credit-note party mismatch; allocations to ineligible sales or unposted invoices; payment over-allocation; invoice header/line-total mismatch; and invoices without lines.

## Schema verification

Stage 13A objects present:

- `customer_sequences`, `customers`, and `supplier_sequences`.
- `suppliers.supplier_number` and `suppliers.payment_terms_days`, with positive/range checks and company-scoped supplier-number uniqueness.
- Nullable `sales.customer_id`, a composite `(customer_id, company_id)` FK to `customers`, and the partial customer/date index.
- Customer lookup indexes, including partial phone/email indexes.

Stage 13B objects present:

- `customer_payments`, `customer_payment_sequences`, `customer_payment_allocations`.
- `supplier_invoices`, `supplier_invoice_items`, `supplier_payment_sequences`, `supplier_payments`, and `supplier_payment_allocations`.
- `customer_credit_notes` and `supplier_credit_notes`.
- `customer_account_balances` and `supplier_account_balances`.
- Positive amount, valid method/status, nonempty fingerprint/reason, document total/date/state, and sequence checks; company-scoped numbers and idempotency uniqueness; and the migration-defined indexes.

The customer/supplier and payment/invoice parents generally have unique `(id, company_id)` keys. Stage 13B payment, allocation, invoice, note, and actor relationships use company-aware foreign keys where defined. The account views also include company predicates in their correlated sums.

## Account-balance view behavior

- `customer_account_balances` is completed customer-linked sale totals, less their sale payments, less customer account payments, less customer credit notes. It is a party-wide net balance; allocation rows do not affect it. Refund/return rows do not automatically change receivables; an account credit note is the migration’s adjustment mechanism.
- `supplier_account_balances` is posted supplier invoice totals, less supplier payments, less supplier credit notes. It is also a party-wide net balance; allocations do not determine per-invoice balances.
- Each view aggregates each source separately, so joins do not multiply totals. Positive values mean customers owe the business and the business owes suppliers, respectively.

## Gaps and recommended forward migrations

These are enforcement gaps in the installed migration design, not observed corrupt rows in the checks above:

1. **Invoice-line tenant references:** `supplier_invoice_items.invoice_id` is company-composite, but its optional `purchase_order_item_id` and `goods_receipt_item_id` FKs reference only item `id`. A line can therefore reference another company’s item while its own invoice belongs to this company. Add/confirm composite unique keys `(id, company_id)` on both source item tables, then add composite FKs from invoice lines. The live data scan found no cross-company references. Use a new additive migration; do not edit the installed Stage 13B file.
2. **Party consistency:** same-company FKs do not require an allocation’s sale/invoice to belong to the payment’s customer/supplier. Customer credit notes can name a different customer’s sale; supplier credit notes can name a different supplier’s invoice; and a supplier invoice can name a PO for another supplier. Enforce these invariants with transactionally locked service checks and, if database enforcement is required, carefully designed composite keys or triggers in a new migration. The corresponding current-data counts were zero.
3. **Cross-row financial limits and states:** the schema does not cap allocation sums at payment or open-document balances, nor prohibit all allocations to void/draft targets at constraint level. Service writes must lock the payment and target, validate status and remaining amounts, and commit allocation/document changes atomically. Add database triggers only if equivalent enforcement is required for writes outside the service.
4. **Invoice arithmetic:** the header total is internally checked, but line totals are not constrained to reconcile with header subtotal/tax/discount, and a posted invoice is not made immutable by the database. Define the authoritative posting calculation and enforce posting/immutability with a forward migration or tightly controlled service transactions.
5. **Currency semantics:** supplier invoices carry `currency_code`, while supplier payments and both account views have no currency grouping/conversion. The supplier view can net amounts in different currencies. Before exposing balances, choose one currency policy and implement it consistently in a new migration and services.
6. **Cash-session requirement:** customer/supplier cash payments may omit `cash_session_id`; the existing check only prohibits a session for noncash payments. Require an open session for cash payments in the transaction workflow and decide whether to add database enforcement.
7. **Permission grants:** the Stage 13B grant logic uses role keys without also restricting grants to active system roles. Audit existing grants before changing them; role-permission rows do not record grant provenance. Correct future seeding in a forward migration after reviewing live role data.

## Existing structures confirmed

The live schema contains `purchase_orders`, `purchase_order_items`, `goods_receipts`, `goods_receipt_items`, `suppliers`, `supplier_products`; `sales`, `sale_items`, `sale_payments`; `sale_returns`, `sale_return_items`, `sale_voids`, `sale_refunds`; and `cash_registers`, `cash_register_sessions`, `cash_movements`. Relevant parent FKs pair IDs with `company_id`; cash sessions reference a register using `(register_id, company_id, location_id)`, have validated state/closing checks, and a partial unique index permits at most one open session per register. Refunds retain cash-session linkage and a check that selects exactly one return or void parent.

## Scope limits

This was a catalog and aggregate verification, not a full application integration test or concurrency test. Zero anomaly counts describe current rows only; they do not replace the missing database constraints or service-level validation described above. No database or migration file was changed.
