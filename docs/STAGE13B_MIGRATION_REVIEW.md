# Stage 13B Financial Integrity Migration Review

Date: 2026-11-12  
Status: remediation review; revised migrations are unapplied. No database was modified.

## Recommendation: DO NOT APPLY YET

The schema and Rust remediation is implemented locally, but the historical approval rows remain unresolved and the PostgreSQL integration suite has not been run. A separate prerequisite migration seeds pending, auditable approvals; the integrity migration intentionally aborts until those approvals and cash-event dispositions are recorded. Do not expose account posting APIs or apply the integrity migration until the human decisions below are complete and PostgreSQL tests pass on a disposable database.

## Findings by severity

### Critical — human approval required

1. **Historical currencies remain unknown.** The live database has seven sales, five sale payments, and five refunds, with no stored currency in those records. The current company default is USD and is not historical evidence. The revised migration no longer copies the current default onto historical sales; it reads only evidence-backed approval records. All seven sale approvals are pending.
2. **Four historical cash events lack session attribution.** Two cash sale payments and two cash refunds have null `cash_session_id`. The available sessions were opened November 10, after the November 6–8 events. The migration does not assign them. An authorized accounting owner must support a real session mapping and update the source before approval, or record an evidence-backed outside-session exception and reconcile the cash ledger. All four dispositions are pending.
3. **Cancelled-sale financial history needs review.** Sales 8 and 9 are cancelled but each has a cash payment and cash refund. Amounts match, but the owner must verify the actual settlement/return and session-close treatment from source records.

### High — implementation/test gate

1. **Concurrency behavior is not demonstrated yet.** The code now acquires customer/payment/document/allocation/session locks in the documented order, reads customer balances after a separate customer-lock statement, and includes allocation/payment/credit reversals in the relevant totals. The opt-in PostgreSQL tests compile but were not run against PostgreSQL. No concurrency-safety claim is made until those tests pass on the correct PostgreSQL version and isolation level.
2. **Posting APIs are still absent.** The reversal service is transactional and permission-aware, but customer receipts, supplier payments, allocations, invoices, credit notes, and reversals do not have HTTP posting routes. Do not expose direct table writes. Future allocation/payment/invoice posting services must follow the same lock order and idempotency rules.
3. **Customer account currency remains base-currency only.** The migration explicitly aborts if an approved historical sale currency differs from current company default. If contemporaneous evidence shows any historical sale was non-USD, decide how to convert or support multi-currency customer receivables before applying.
4. **Cash attribution of pre-existing session-linked events needs owner confirmation.** The live report shows November 10 sales/refunds tied to sessions 1 and 3, while those sessions store expected cash equal to opening float. Verify that session totals reflect the actual events before relying on the stored closing values.

### Medium — operational

1. **The migration is a long transaction.** The integrity migration backfills rows, adds/validates constraints, replaces foreign keys, creates reversal tables/triggers, and replaces views. Current development row counts are small, but production table sizes/lock durations are unknown. Test against a representative clone and schedule a lock-monitoring window.
2. **Custom-role grants are preserved.** The new `customers.profile.manage` grant is copied from existing `customers.manage` grants, without deleting the old permissions. Generic customer profile DTOs cannot set credit fields; credit-policy PATCH requires `customers.credit` in middleware and service. Custom grant inventory should still be reviewed before production enablement.
3. **Supplier payment approvals are presently empty-set.** The live target has no supplier payments, invoices, allocations, or supplier credit notes. Any nonempty target requires independent payment currency evidence; allocation currency alone is not accepted.

## Implemented protections

- The approval-gate migration records currency/session approval evidence and seeds unresolved rows without guessing. The financial-integrity preflight rejects missing approvers, timestamps, evidence, unresolved cash events, unsupported non-base historical sale currency, and supplier payment mappings inconsistent with allocated invoice currency.
- Historical sales/payment/refund currency backfill comes from approved sale mapping; supplier payment currency comes from an independent approved mapping. Refunds now carry explicit currency linked to the source sale.
- Allocation rows and sale refunds are protected from update/delete. Customer/supplier allocation reversals, payment reversals, and credit-note reversals use immutable positive events, company-scoped idempotency, source party/currency composite foreign keys, and cumulative caps.
- Derived account balances include payment and credit-note reversals. Cash calculations include cash customer-payment reversals as outflows and supplier-payment reversals as inflows, once, without duplicate manual movements.
- Customer profile management and credit-policy edits are separate permissions/API shapes. Generic create/update cannot enable credit or change limits/terms; the credit-policy route and service both require `customers.credit`. Existing custom grants are not silently removed.
- SQLx runs the migrations transactionally by default. If the integrity migration aborts, its DDL/backfills roll back. The prerequisite approval migration may already be committed if the migration runner proceeds to the failing later migration; its pending rows are deliberate and persistent for review.

## Test and verification state

- `cargo fmt` completed.
- `cargo check --all-targets` passed.
- `cargo test --all-targets` passed: 2 unit tests and 8 static migration/permission contract tests passed. The PostgreSQL integration test returned without connecting because its opt-in variables were not set.
- The PostgreSQL suite refuses a database unless its name starts `chimp_pos_stage13b_test`. It requires `STAGE13B_TEST_DATABASE_URL` and `STAGE13B_TEST_FIXTURES_JSON`; it does not run migrations. It covers payment and invoice allocation races, customer credit limit checks, payment/allocation/credit-note reversals, idempotent replay, transaction rollback, and cash-session close racing with a cash reversal. This suite has not executed against PostgreSQL.
- Read-only data evidence and decision worksheet are in [STAGE13B_HISTORICAL_RECONCILIATION.md](STAGE13B_HISTORICAL_RECONCILIATION.md). Full reversal sequencing is in [STAGE13B_REVERSAL_LIFECYCLE.md](STAGE13B_REVERSAL_LIFECYCLE.md).
- Neither migration has been applied. No previously applied migration or database was changed.

## Human decisions before approval

1. Supply evidence-backed currency approval for each sale ID 2, 3, 4, 5, 7, 8, and 9. Confirm that each approved customer-sale currency is USD or choose a multi-currency/conversion design.
2. Resolve missing cash-session attribution for sale payment IDs 1 and 2 and refund IDs 1 and 2. Use a verified session only when evidence supports it; otherwise approve an outside-session exception and reconcile the ledger.
3. Verify sale 8 and 9 cancellation/payment/refund histories and confirm November 10 sessions 1 and 3 include the linked transactions in their close reconciliation.
4. Review role grants in the actual deployment database, including any custom roles, before enabling account endpoints.
5. Provision a disposable PostgreSQL database with prepared fixtures, apply only there after human approvals, and pass the integration suite. Review migration lock duration on representative production-sized data.

Until those actions are complete, the recommendation remains **DO NOT APPLY**.
