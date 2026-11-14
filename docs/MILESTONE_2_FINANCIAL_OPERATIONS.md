# Milestone 2 — Financial Operations

## Implementation status

Implemented the initial customer receivables and supplier payables endpoints against the applied Stage 13B base schema. No migration was added or applied. Payment and credit-note reversal endpoints are not exposed because the reversal tables in `20261112120000_stage_13b_financial_integrity.sql` remain pending.

## Endpoints

All endpoints are under `/api/v1`, require authentication, enforce company scoping, and use the standard `{ error: { code, message } }` error envelope.

| Method and path | Permission | Behavior |
| --- | --- | --- |
| `GET /customers/{id}/account` | `customers.view` | Derived customer balance and company base currency |
| `GET /customers/{id}/statement` | `customers.view` | Paginated customer account events |
| `GET /customers/{id}/outstanding-sales` | `customers.view` | Paginated completed sales with outstanding amounts |
| `POST /customers/{id}/payments` | `customers.payment` | Post idempotent receipt and optional multi-sale allocations |
| `POST /customers/{id}/credit-notes` | `customers.credit` | Post credit note, optionally linked to a sale |
| `GET /suppliers/{id}/account` | `suppliers.view` | Derived supplier balance; rejects mixed currency ledgers |
| `GET /suppliers/{id}/statement` | `suppliers.view` | Paginated supplier ledger events |
| `GET /suppliers/{id}/outstanding-invoices` | `suppliers.view` | Paginated posted invoices and outstanding amounts |
| `GET /suppliers/{id}/invoices` | `suppliers.view` | Paginated supplier invoices and lines |
| `POST /suppliers/{id}/invoices` | `suppliers.invoice` | Create and post supplier invoice plus lines atomically |
| `GET /supplier-invoices/{id}` | `suppliers.view` | Retrieve invoice and lines |
| `POST /suppliers/{id}/payments` | `suppliers.payment` | Post supplier payment and optional multi-invoice allocations |
| `POST /suppliers/{id}/credit-notes` | `suppliers.invoice` | Post a supplier credit note against an invoice |

Request and response schemas, authentication security, permission descriptions and error responses are in [`openapi.yaml`](openapi.yaml).

## Workflows and integrity

- Customer receipts can remain partly or wholly unallocated; allocations are supplied at receipt creation and can cover multiple completed sales. Supplier payments similarly may be partly allocated across posted invoices.
- Each payment locks its party row and target documents, in ascending document ID order, checks the remaining amount, then writes payment and allocations in one transaction. The cash-session row is locked and must still be open for cash payments. Supplier invoice creation writes a draft header and lines, then posts within the same transaction.
- Company predicates are applied to party, sales, invoices, purchase orders, receipts and cash sessions. Existing composite constraints provide company ownership checks for financial records. Cross-company targets are returned as unavailable or rejected.
- Money is represented with `rust_decimal::Decimal`, limited to four fractional digits, and written to PostgreSQL `NUMERIC(19,4)`.
- Payment and invoice requests use a company-scoped advisory transaction lock and a UUID idempotency key plus canonical request fingerprint. Reusing a key with changed contents conflicts. Credit notes use a company-scoped idempotency key and compare their stored request fields.
- Account totals are read from the existing derived views and posted events. No balance column or manual cash movement is written. Cash receipt/payment rows already feed the cash-session summaries; the handlers do not create a second `cash_movements` row.
- Posted financial records have no update/delete endpoint. Corrections through reversal events remain unavailable until the forward migration is reviewed and applied.
- Supplier invoice posting supports lines, line tax and discount amounts, document-level additional charges, and links to same-company purchase orders/goods receipts when supplied. A PO or receipt does not itself create a payable.

## Currency and reversals

The applied base schema has currency only on supplier invoices. It has no currency column on sales, customer receipts/allocations/credits, or supplier payments/allocations/credits. To avoid silently mixing ledgers, new supplier invoices and supplier account postings are restricted to the configured company base currency. Supplier account reads reject detected non-base currency records. Customer receipts and credit notes are likewise recorded in company base currency; when the sales currency column exists, it is checked against that currency.

The live-database review found historical completed sales without stored currency evidence. Therefore, the customer balance currency label follows company configuration but is not proof of the historical sales' actual currency. Resolve the historical currency approval/reconciliation report before relying on these balances for financial close or migration approval.

Payment and credit-note reversals are intentionally not available. Allocation changes are only possible by appending allocation rows at payment creation; there is no standalone allocation endpoint. The applied schema does not provide allocation reversal events or payment reversal events. Do not edit/delete posted rows directly.

## Database changes

None. Existing base Stage 13B tables, views and permissions are used. Pending migration files were not changed or applied.

## Tests and results

- Added unit tests for positive and bounded allocation requests, duplicate targets, and cash-method/session consistency.
- Added an opt-in PostgreSQL integration test covering partial multi-sale allocation, idempotent replay, cash-session attribution without duplicate cash movement, over-allocation rejection, cross-company target rejection, permission denial, supplier invoice posting with taxes/discounts/charges and idempotency, partial supplier payment, currency rejection, over-credit rejection, and transactional invoice rollback.
- The integration test requires `CHIMPPOS_FINANCIAL_TEST_DATABASE_URL` and `CHIMPPOS_FINANCIAL_TEST_FIXTURES_JSON`, and refuses a database whose name does not start with `chimp_pos_finance_test`. It does not run migrations. It was not run in this environment because no disposable PostgreSQL fixture was configured. This is not evidence of PostgreSQL integration correctness.
- Concurrent allocation races, simultaneous credit checks, closing-session races, and reversal workflows still need PostgreSQL integration coverage before claiming concurrency safety.

## Known limitations and outstanding work

1. Human approval and reconciliation of historical sale currency and refund session attribution are outstanding.
2. Base-schema records cannot represent per-transaction currency for most customer and supplier events. Review and apply the pending currency-hardening migration only after the documented preflight approvals.
3. Payment/allocation/credit-note reversals are not implemented or exposed; posted rows have application-level immutability only under current endpoints, not database trigger enforcement from the pending migration.
4. Credit-limit enforcement is not implemented. The current applied schema does not expose a safely enforceable credit-policy workflow for this API.
5. The PostgreSQL integration suite needs a dedicated disposable database and fixture seed, then must be run alongside concurrency/race tests.
6. Verify the pending financial-integrity migration's cash-session reversal summary behavior before enabling reversal posting.
