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

---
# Part 2 (14–15 Nov 2026): offline sync, sessions & passwords, production readiness

## Added
* **Offline sync** – `GET /sync/bootstrap`, `GET /sync/pull/{locations|categories|products|variants|stock|customers|registers}`,
  `POST /sync/push` (sale.checkout, expense.create, stock.adjust, customer.payment, cash.movement),
  `GET /sync/operations`, `POST /sync/operations/{id}/retry` (optionally `run_as_reviewer`), `POST /sync/operations/{id}/dismiss`.
  Trigger-maintained `updated_at`, keyset delta feeds with a 60 s safety margin, idempotent journal (`sync_operations`).
  New permissions `sync.use`, `sync.manage`.
* **Sessions** – rotating refresh tokens with reuse detection, `POST /auth/refresh|logout|logout-all`, `token_version` access-token revocation.
* **Passwords** – change, admin reset (`users.manage`, owner-protected), forgot/reset tokens (log delivery only), forced change after admin reset,
  account lockout, per-IP rate limit, timing-safe login, JSON error bodies on all auth endpoints, richer `GET /auth/me` (roles + permissions).
* **Operations** – CLI (`serve`, `migrate`, `reset-password`), production config guard, security headers, request ids, access logs,
  panic catching, DB statement/idle timeouts, TLS to PostgreSQL (`tls-rustls`), periodic `ANALYZE`, Dockerfile, compose, systemd, Caddy, backup/restore scripts, CI template.
* `GET /inventory/stock` now paginated (`limit` default 1000, max 5000, `offset`).

## Bugs found by the new tests
| Bug | Fix |
|---|---|
| Access-token revocation by timestamp was only second-accurate (a token issued in the same second as a password reset survived) | exact per-user `token_version` embedded in tokens |
| Login returned instantly for unknown users (username enumeration by timing) | equalising dummy hash |
| Manager "retry" of a price-conflict offline sale could never succeed (ran as the cashier) | `run_as_reviewer` |
| Stale planner statistics made `/inventory/stock` take 5–7 s and a report hit the 30 s timeout after bulk loads / restores | periodic + post-migrate + post-restore `ANALYZE` |
| `/inventory/stock` unbounded | pagination |
| `sqlx::migrate!` not re-embedded when a migration is added | `build.rs` (earlier) |

## Verified
`scripts/run_all_tests.sh`: smoke (≈110 checks), offline sync, auth/sessions/passwords, concurrency (last unit sold by 8 cashiers, same checkout ×8, same offline op from 8 devices, till overdraw, double session open, lost-update on adjustments). `cargo test`. Backup → restore → boot. Load test (`scripts/load_test.py`).
