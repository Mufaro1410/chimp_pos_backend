# Backend completion notes (13 Nov 2026)

## Bugs fixed
| # | Bug | Effect | Fix |
|---|-----|--------|-----|
| 1 | Tree did not compile (12 errors from half-finished pagination) | Nothing ran | Duplicate `ProductListQuery`, `AuthContext`->`AuthUser`, missing imports/match arms |
| 2 | New companies had no inventory location | Stock, receiving, sales impossible for any shop registered after the inventory migration | `register_company` creates "Main Store"; migration backfills existing companies |
| 3 | New Manager/Cashier roles missed permissions added by later migrations | Cashier could not edit a draft sale; Manager could not receive goods, pay suppliers... | Canonical permission lists in `auth_service.rs` + additive reconciliation migration |
| 4 | `customer_payment_allocations` / `supplier_payment_allocations` insert omitted `customer_id`/`supplier_id` | Every customer/supplier payment WITH allocations returned 500 | `financial_operations_service::insert_allocations` |
| 5 | Credit-note inserts omitted `request_fingerprint` | Every customer/supplier credit note returned 500 | Both inserts |
| 6 | Trigger `enforce_supplier_invoice_posting` had a variable named `line_total` (ambiguous with the column) | Every supplier invoice returned 500 | Migration `20261113090500` replaces the function |
| 7 | `sale_refunds` insert omitted `currency_code` | Every refund and every void returned 500 | `sales_returns_service` (both inserts) |
| 8 | Immutability trigger forbade the `completed -> cancelled` transition used by voids | Every void failed | Migration `20261113091000` allows exactly that transition when a void record exists |
| 9 | Dashboard SQL used bare alias `day` | `/reports/dashboard` returned 500 | Alias renamed |
| 10 | No tracing subscriber installed | All `tracing::error!` output was silently dropped | `tracing-subscriber` in `main.rs` |
| 11 | JWT lived 900s with no refresh | Cashiers logged out every 15 minutes | `JWT_TTL_SECONDS` (default 3600) + `POST /auth/refresh` |
| 12 | No `build.rs` | New migration files were not embedded by `sqlx::migrate!` until a clean rebuild | `build.rs` |

## Added
- Lookups: `GET /units`, `GET|POST /categories`, `PUT /categories/{id}`, `GET|POST /locations`, `PUT /locations/{id}`
- Expenses: `GET|POST /expenses`, `GET /expenses/{id}`, `GET /expenses/summary`, `POST /expenses/{id}/void`
  (cash expenses create a `paid_out` till movement atomically; idempotent)
- Financial reversals routed: `POST /customer-payments/{id}/reversals`, `/supplier-payments/{id}/reversals`,
  `/customer-credit-notes/{id}/reversals`, `/supplier-credit-notes/{id}/reversals`
  and `GET /customer-payments/{id}`, `GET /supplier-payments/{id}` (lists allocation ids needed to reverse)
- `GET /reports/sales/gross-profit` (revenue - COGS using cost snapshot on `sale_items.unit_cost`)
- `GET /audit-logs` (every successful write is recorded: user, method, path, status)
- `POST /auth/refresh`
- CORS (`CORS_ALLOWED_ORIGINS`), 30 s request timeout, graceful shutdown, `HOST`/`PORT`, opt-in `RUN_MIGRATIONS`
- 401/403 now return the standard JSON error body

## Known limitations (by design / next steps)
- Payment methods on customer/supplier payments, reversals: `cash | card | mobile_money | other` (DB constraint). Use `other` for bank transfer.
- Gross profit is before returns; lines created before the cost snapshot fall back to the variant's current cost.
- Offline synchronisation is not implemented (checkout, adjustments, expenses, payments already accept idempotency keys, which is the foundation).
- Token refresh requires a still-valid token; there are no long-lived refresh tokens / password-reset flow yet.
- Money-affecting reports are single-currency per company (matches the schema).

## Verify
    cp .env.example .env   # edit values
    cargo run
    python3 scripts/smoke_test.py   # ~110 checks across every module, fresh company each run
