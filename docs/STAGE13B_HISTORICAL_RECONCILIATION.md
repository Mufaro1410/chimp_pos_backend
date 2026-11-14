# Stage 13B Historical Reconciliation

Date: 2026-11-12  
Target: `chimp_pos_dev`  
Method: read-only PostgreSQL queries; no database or migration changes were made.

## Company currency

| Company ID | Current default currency | Historical evidence |
|---|---|---|
| `e2ba7142-7f27-4c8a-b99c-4353dbd9ab09` | USD | Sales and sale-payment tables do not store currency. The current default is not proof of historical transaction currency. |

## Historical sales and associated payments

No customer name or other customer personal information is included. `created_at` is shown as stored, in the Harare (`+02`) offset. Currency is absent from the historical sale and payment records.

| Sale ID | Created | Status | Total | Currency evidence | Payment rows |
|---:|---|---|---:|---|---|
| 2 | 2026-11-06 13:22:31 +02 | draft | 9.0000 | Not stored | None |
| 3 | 2026-11-06 13:28:31 +02 | draft | 9.0000 | Not stored | None |
| 4 | 2026-11-06 13:34:21 +02 | completed | 9.0000 | Not stored | Payment 1 |
| 5 | 2026-11-08 09:39:32 +02 | completed | 4.5000 | Not stored | Payment 2 |
| 7 | 2026-11-10 19:50:28 +02 | completed | 4.5000 | Not stored | Payment 3 |
| 8 | 2026-11-10 20:43:55 +02 | cancelled | 4.5000 | Not stored | Payment 4 |
| 9 | 2026-11-10 21:49:18 +02 | cancelled | 4.5000 | Not stored | Payment 5 |

| Payment ID | Sale ID | Created | Method | Amount | Cash session | Currency evidence |
|---:|---:|---|---|---:|---|---|
| 1 | 4 | 2026-11-06 13:52:56 +02 | cash | 9.0000 | Missing | Not stored; parent sale has no currency |
| 2 | 5 | 2026-11-08 09:40:19 +02 | cash | 4.5000 | Missing | Not stored; parent sale has no currency |
| 3 | 7 | 2026-11-10 19:52:01 +02 | cash | 4.5000 | 1 | Not stored; parent sale has no currency |
| 4 | 8 | 2026-11-10 20:44:45 +02 | cash | 4.5000 | 1 | Not stored; parent sale has no currency |
| 5 | 9 | 2026-11-10 21:49:53 +02 | cash | 4.5000 | 3 | Not stored; parent sale has no currency |

## Historical refunds

| Refund ID | Sale ID | Created | Method | Amount | Return / void ID | Cash session |
|---:|---:|---|---|---:|---|---|
| 1 | 4 | 2026-11-07 21:10:45 +02 | cash | 4.5000 | Return 1 | **Missing** |
| 2 | 5 | 2026-11-08 09:41:05 +02 | cash | 4.5000 | Void 1 | **Missing** |
| 3 | 7 | 2026-11-10 20:24:06 +02 | cash | 4.5000 | Return 2 | 1 |
| 4 | 8 | 2026-11-10 20:47:01 +02 | cash | 4.5000 | Void 2 | 1 |
| 5 | 9 | 2026-11-10 21:55:59 +02 | cash | 4.5000 | Void 3 | 3 |

Refund currency is not stored; it can only be associated with its parent sale after a business owner approves that sale's historical currency. Refunds 1 and 2 are the two confirmed cash refunds with null session attribution. Cash payments 1 and 2 also have null sessions and require review because session close totals are session-scoped.

## Cash-register sessions

These are the cash sessions present in the read-only query. Neither session covers the November 6, November 7, or November 8 payment/refund events. Do not map those events to a session based on proximity or current opening floats.

| Session ID | Register / location | Opened | Closed | State | Opening float | Stored expected cash |
|---:|---|---|---|---|---:|---:|
| 1 | 1 / 1 | 2026-11-10 18:40:41 +02 | 2026-11-10 20:50:40 +02 | closed | 100.0000 | 100.0000 |
| 3 | 1 / 1 | 2026-11-10 21:28:36 +02 | 2026-11-10 21:59:15 +02 | closed | 100.0000 | 100.0000 |

On November 10, sale payment 3 and refunds 3/4 are associated with session 1; payment 4 is also associated with session 1 although the sale was cancelled. Payment 5 and refund 5 are associated with session 3. The stored expected-cash values equal their opening floats; that alone does not confirm whether each associated transaction was included correctly in the close calculation.

## Proposed human decision register

Complete this table from original receipts, POS journal, accounting records, or other contemporaneous evidence. `approved_currency` must be an explicit ISO 4217 currency code supplied by an authorized business/accounting owner. Do not fill it from the current company default without evidence. For cash events, choose a verified session only when contemporaneous evidence supports it; otherwise document an approved outside-session exception and the corresponding cash-ledger reconciliation. An outside-session exception must not be counted in a register session.

| Record | Current facts | Proposed decision fields | Human approval required |
|---|---|---|---|
| Sale 2 | draft; 2026-11-06; 9.0000 | `approved_currency`, evidence reference, approver user, approval time | Yes. Confirm whether this draft should receive a historical currency label. |
| Sale 3 | draft; 2026-11-06; 9.0000 | same fields | Yes. |
| Sale 4 | completed; 2026-11-06; 9.0000; payment 1; refund 1 | currency fields; payment 1 session disposition; refund 1 session disposition; evidence; approver | Yes. Currency and two cash events need independent support. |
| Sale 5 | completed; 2026-11-08; 4.5000; payment 2; refund 2 | currency fields; payment 2 session disposition; refund 2 session disposition; evidence; approver | Yes. Currency and two cash events need independent support. |
| Sale 7 | completed; 2026-11-10; 4.5000; payment 3; refund 3 | currency fields; confirm session 1 event attribution | Yes. Confirm currency and that session 1's close captured both events. |
| Sale 8 | cancelled; 2026-11-10; 4.5000; payment 4; refund 4 | currency fields; confirm session 1 event attribution and cancellation/refund relationship | Yes. Confirm why a payment exists for a cancelled sale and whether it was fully returned. |
| Sale 9 | cancelled; 2026-11-10; 4.5000; payment 5; refund 5 | currency fields; confirm session 3 event attribution and cancellation/refund relationship | Yes. Confirm why a payment exists for a cancelled sale and whether it was fully returned. |

For each cash payment/refund, record: `event_type`, `event_id`, `approved_disposition` (`verified_session` or `outside_session_exception`), `approved_session_id` only for a verified session, `cash-ledger evidence reference`, `reconciliation amount`, `authorized approver`, `approved_at`, and `notes`. For each sale, record: `sale_id`, `approved_currency`, `currency evidence reference`, `authorized approver`, `approved_at`, and `notes`. The migration requires an active system owner/admin with `financial.reconciliation.approve`; these are proposed audit fields, not populated approvals.

## Approval status and migration gate

All seven sale currency decisions remain **pending**. Currency for the five payments and five refunds must follow the approved parent sale currency only after that decision. Cash-session disposition remains **pending** for payments 1/2 and refunds 1/2. A business owner/accounting approver must resolve sale 8 and sale 9's cancelled-sale/payment/refund histories and verify the November 10 cash-session closes. Until decisions and evidence are recorded, the migration must stop before applying any currency backfill or cash attribution.

The read-only query also found zero customer account payments, customer credit notes, customer allocations, supplier invoices/payments, supplier allocations, or supplier credit notes. The revised approval gate seeds currency-approval rows for any such customer-account or supplier-payment records if present in another target; none require a current decision for this database.

This report records observed data only. No currency was inferred; no session was assigned; no records were changed.
