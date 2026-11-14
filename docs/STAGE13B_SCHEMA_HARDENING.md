# Stage 13B Schema Hardening

Date: 2026-11-12  
Status: remediation implemented locally; migrations remain unapplied. `chimp_pos_dev` was queried read-only and was not modified.

## Changes in this remediation

- Added `20261112110000_stage_13b_reconciliation_approvals.sql`, a preparatory migration that creates auditable, initially unresolved currency/session approval rows. It does not invent currency or session values. Approved reconciliation records are immutable.
- Added `financial.reconciliation.approve`, granted only to active system owner/admin roles by the preparatory migration; the approval trigger rejects unapproved actors.
- Revised `20261112120000_stage_13b_financial_integrity.sql`. It now aborts before financial DDL/backfills when historical sale, customer-account, or supplier-payment currency approval is unresolved; cash sale payment/refund session exceptions are unresolved; or an approved mapping conflicts with supported currency policy. Historical sale/payment/refund currency is backfilled only from explicit sale approval; customer-account and supplier-payment rows also require their own approvals when present.
- Added explicit currency to sale refunds and composite source-currency relationships.
- Added customer/supplier allocation-reversal, payment-reversal, and credit-note-reversal tables. They use positive immutable events, company-scoped idempotency keys/fingerprints, composite source party/currency foreign keys, and database caps on cumulative reversal amounts. Payment reversals cannot exceed original unallocated amount plus already reversed allocations.
- Allocation rows are insert-only after posting. Reversal rows and sale refunds cannot be updated or deleted. Derived account views include payment and credit-note reversals.
- Cash close, cash availability, and session summary calculations include customer payment reversals as cash out and supplier payment reversals as cash in. They count the reversal tables once and do not create duplicate `cash_movements` rows.
- Customer profile create/update requests no longer accept credit enablement, limit, or terms. Credit-policy updates use a separate PATCH route with `customers.credit` in middleware and service checks. Existing `customers.manage` grants are not deleted; an equivalent profile-management permission is granted to roles that already had customer management, while the generic profile DTO cannot change credit policy.
- Checkout locks the customer before the sale, then reads the derived account balance in a separate statement after locking. The documented lock order is party → payment → target documents sorted by ID → allocation rows sorted by ID → cash session.
- Added an opt-in PostgreSQL integration suite that refuses databases not named with the `chimp_pos_stage13b_test` prefix. It covers concurrent payment/invoice allocation, customer credit-limit checks, customer/supplier reversal, idempotent replay, rollback, and cash close races. It never runs migrations.
- Added [the historical reconciliation report](STAGE13B_HISTORICAL_RECONCILIATION.md), [the reversal lifecycle](STAGE13B_REVERSAL_LIFECYCLE.md), and the updated [migration review](STAGE13B_MIGRATION_REVIEW.md).

## Historical approval gate

Read-only inspection found seven sales, five sale payments, and five cash refunds in the target. None of those historical sales/payment/refund rows stores currency. The company currently uses USD; that is not historical evidence. Two cash payments and two cash refunds have no session ID. Existing sessions are dated November 10, after the earlier unlinked events.

The pre-migration approval tables seed all sales, existing customer account payments/credit notes, and supplier payments as pending. The primary migration stops unless each has a currency, evidence reference, approver, and approval timestamp. Supplier payments require independent evidence-backed approval; allocation currency alone is not accepted. The customer ledger is base-currency only, so non-base customer-account events require a separate multi-currency receivable/conversion design and are rejected by this migration.

For a cash event with a verified session, an authorized operator must first record the supported session attribution in the source event and approve the matching reconciliation row. Otherwise, a cash event may remain sessionless only as an approved, evidence-backed outside-session exception with a reconciled cash-ledger note. The migration does not assign sessions. No current decisions are recorded; see the reconciliation report.

## Enforcement split

### PostgreSQL

- Composite foreign keys/checks enforce tenant, party, source document, and currency consistency.
- Triggers enforce allocation/document limits, cumulative reversal caps, posted-event immutability, supplier invoice posting math, and open cash-session checks.
- Immutable reversal events feed derived balances and cash summaries. No manually maintained balances or duplicate cash movements are added.

### Rust transaction services

- `financial_reversals_service` executes payment/allocation reversals and credit-note reversals in a single transaction, checks permissions, replays idempotency only for matching fingerprints, locks parents in the documented order, and lets any failed step roll back the complete operation.
- Customer credit checks lock the party then read current derived balance in a later SQL statement. Future allocation/payment/invoice posting services must follow the same order and use idempotent request fingerprints.
- No account posting/reversal routes have been added. The reversal service is not reachable through HTTP until dedicated handlers, routes, permission checks, and PostgreSQL tests are reviewed.

### Permission middleware

- `customers.profile.manage` protects customer profile creation/editing.
- `customers.credit` protects the separate credit-policy endpoint and is checked again in the service.
- Existing `customers.manage` and custom grants are retained. The role's old grant is carried forward as profile management; credit fields are absent from the generic create/update DTOs. No custom-role grant is silently deleted.

## Verification state

- `cargo fmt` completed.
- `cargo check --all-targets` passed.
- `cargo test --all-targets` passed: 2 unit tests and 8 static migration/permission contract tests passed. The PostgreSQL scenario test returned without connecting because `STAGE13B_TEST_DATABASE_URL` and `STAGE13B_TEST_FIXTURES_JSON` were not configured; PostgreSQL concurrency and trigger behavior remain unverified.
- The preparatory and integrity migrations have not been applied anywhere. The integrity migration will currently stop because the seven sale-currency decisions and four cash-event session dispositions are pending.

## Outstanding risks and decisions

1. Business/accounting owner must approve currency evidence for all seven historical sales; confirm cancelled-sale payment/refund history for sales 8 and 9.
2. Business/accounting owner must resolve two cash payments and two cash refunds with missing session attribution; also verify session 1 and 3 close totals for the linked November 10 events.
3. If any sale currency is not USD, choose a multi-currency customer receivable/conversion design before approving it; current customer account balance logic only supports the company base currency.
4. Supplier payment approvals will be required for any target containing supplier payment rows. Current target has none.
5. Run the opt-in PostgreSQL integration suite against a freshly provisioned disposable DB populated with the documented fixtures. Concurrency safety is not claimed until these tests pass.
6. Implement and review account posting, allocation, invoice, and credit-note services/routes before exposing the new financial tables.
7. Validate all modified Rust modules against application behavior and verify the two cash-session close results against source cash records.
