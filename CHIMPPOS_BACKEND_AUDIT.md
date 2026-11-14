# ChimpPOS Backend — Initial Repository Audit

Audit date: 2026-11-12  
Scope: current working tree, including the uncommitted Stage 13A/13B work already present. This report does not apply migrations or modify the database. No secret values were read or included.

## Current architecture

- Rust 2024 crate using Axum 0.8, Tokio, SQLx/PostgreSQL, `rust_decimal`, UUID, Argon2 password hashing and JWT authentication.
- `src/main.rs` loads dotenv variables, creates a 10-connection PostgreSQL pool and serves on `127.0.0.1:3000`. `AppState` holds the pool and JWT secret.
- `src/routes/mod.rs` groups public auth/health routes and authenticated, permission-checked product/inventory, purchasing, trading-party, sales, returns and cash-register routes. Handlers translate service errors into HTTP responses; domain SQL and transaction logic primarily lives in `src/services`.
- SQL migrations are the authoritative schema and permission seed history. There is no separate test directory or test module found.
- Current uncommitted changes add trading parties, customer-linked/credit sales, Stage 13B schema and related cash calculations. Findings below describe that on-disk state; these changes were not made by this audit.

## Existing database schema

Chronological migration history (all listed migrations were read):

1. `20261103080748_initial_schema.sql`: companies and users, with company-scoped usernames/emails, user status and initial role field.
2. `20261104063217_add_rbac.sql`: global permission catalogue; company roles, role-permission assignments and company-bound user-role assignments; removes `users.role`.
3. `20261104082230_add_default_role_support.sql`: system-role flags, stable role keys, active state, constraints and indexes.
4. `20261104083337_seed_default_roles.sql`: Owner/Admin/Manager/Cashier roles and initial permission assignments.
5. `20261105130108_create_products.sql`: products with company ownership and decimal price.
6. `20261105150953_inventory_foundation.sql`: units, categories, product variants, locations, stock balances/movements, suppliers and supplier-product relations. Stock/price/cost values use `NUMERIC(19,4)`; company-scoped composite foreign keys are used extensively.
7. `20261106055432_create_purchasing.sql`: purchase order and receipt sequences, purchase orders/items and goods receipts/items, including receipt idempotency and historical item snapshots.
8. `20261106062106_add_purchasing_permissions.sql`: purchasing permissions and role grants.
9. `20261106064404_add_goods_receipt_sequences.sql`: receipt-number sequence table.
10. `20261106091750_create_sales.sql`: sale sequence, sales/items/payments, decimal totals and checkout idempotency; adds sales permissions.
11. `20261107155928_sales_returns_refunds_voids.sql`: return headers/lines, voids and refunds with idempotency keys and request fingerprints.
12. `20261108094955_cash_registers_and_sessions.sql`: registers, one-open-session-per-register index, manual cash movements, cash-session links on cash payments/refunds and permissions.
13. `20261110100000_reconcile_cash_register_permissions.sql`: cash permission reconciliation for default roles.
14. `20261111110000_stage_13a_trading_parties.sql`: customer table/number sequence, supplier numbering and payment terms, optional customer relation on sales.
15. `20261112100000_stage_13b_accounts_payables_receivables.sql`: account receipts/payments and allocations, supplier invoices and lines, customer/supplier credit notes, account-balance views, sequences and financial permissions.

Important schema shape: integer identity IDs are used for operational records; company IDs are UUIDs. Most operational monetary and quantity columns use fixed-scale decimal `NUMERIC(19,4)` and prices/tax rates have explicit precision. Many cross-company relations are guarded by `(id, company_id)` composite foreign keys. Some Stage 13B item references use only globally unique item IDs and do not carry an explicit company composite FK, which is less uniform than the earlier schema.

## Implemented functionality

- **Authentication and authorization:** register/login/me endpoints; Argon2 password hashing and JWT middleware; permission middleware looks up active user roles and permissions. Routes apply authentication before permission checks. Permissions are seeded and evolved through migrations.
- **Tenant isolation:** handlers derive `company_id` and `user_id` from authenticated identity; services generally scope reads/writes by company and migrations use company-scoped unique keys/FKs. This is application-enforced; no PostgreSQL RLS layer was found.
- **Products and variants:** create/list/get/update/archive products and create/list/update/deactivate variants, with category/unit/location references, version checks in update paths, and product row locking in mutation paths.
- **Inventory:** stock listing, movement history and manual adjustment. Adjustment is transactional, idempotency-key serialized and locks balance rows; movement ledger and balance update are coupled.
- **Purchasing and goods receipts:** draft order create/update, submit/cancel/list/detail, receipt history and goods receipt posting. Receipt flow uses a transaction, company/key advisory lock, PO/item/balance locks, idempotency lookup and stock movement creation.
- **Sales and checkout:** drafts, item snapshots, updates, list/detail/receipt, cancellation and checkout with payments. Checkout runs in a transaction, serializes idempotency keys, locks sale/customer/stock rows and links cash payments to sessions. Stage 13A adds optional customer association and constrained customer-credit checkout.
- **Returns, refunds and voids:** return creation, refunds, sale voiding, reads/lists. These flows use transactions, idempotency keys/fingerprints, sale/item/return locks, stock movements and cash-session affordability checks.
- **Cash registers and sessions:** register create/list, open/current/get session, summary, manual cash movement and close. Database partial unique index prevents multiple open sessions; movement and close paths lock rows and calculate cash from source records.
- **Customers and suppliers:** company-scoped list/create/update with generated company sequence numbers and optimistic version checks. Existing supplier records are upgraded by migration rather than replaced.
- **Stage 13B financial foundation:** schema exists for customer receipts, allocations, supplier invoice posting/payment allocations, and credit notes; read-only balance views exist. `docs/STAGE13B_IMPLEMENTATION_NOTES.md` explicitly says corresponding transactional HTTP/service workflows are not delivered yet.

## Incomplete functionality

- Stage 13B financial schema is not backed by endpoints/services: no create/list/post/void APIs for customer account payments, supplier invoices/payments, allocations or credit notes; account balances are not exposed through handlers. The tables should not be written by clients directly.
- No automated unit, integration, database migration or concurrency tests were found. In particular no tests cover tenant-boundary access, duplicate requests, fingerprint mismatch, stock races, credit-limit races, cash reconciliation, returns/void interleavings or migration upgrades.
- No user/role administration routes or audit-log implementation are present despite related permissions/schema concepts; no general audit event stream is found.
- No explicit pagination was seen in list services; inspect and add bounded pagination before growing datasets.
- No health detail/readiness check or graceful shutdown path is evident; `/health` exists but database readiness behavior needs inspection.
- `main` binds loopback only, which may be intentional for local deployment but needs deployment configuration before a remotely accessed service.

## Critical risks ranked by severity

### Critical

1. **The current tree does not compile.** `cargo check --all-targets` fails in `src/handlers/sales.rs:54`: one new match arm returns a `String` where the other arms return `&str` (`SalesError::CreditPermissionRequired`). Until fixed, the API cannot be built or deployed. The error is in pre-existing uncommitted work and was not changed during this audit.
2. **Financial account workflows are schema-only.** Stage 13B creates payable/receivable tables, but there is no service-level transaction/authorization/idempotency/allocation-total enforcement for writes. Exposing or directly inserting into these tables would allow inconsistent financial state. The notes acknowledge this boundary.

### High

3. **No automated tests, and compile currently blocks test execution.** The behaviors most vulnerable to duplicate/concurrent requests (stock, checkout, returns/refunds, cash and credit limits) lack regression coverage. `cargo test --all-targets` was attempted; see verification below.
4. **Permission policy requires careful reconciliation.** `permission_middleware` and service-level permission checks coexist, while migrations evolve permission grants. Ensure every mutation checks the intended permission and roles have deliberate least-privilege grants, especially `customers.credit`, refund/void, cash close/movement and new supplier financial actions.
5. **Tenant safety is not database-enforced globally.** Most code consistently binds `company_id`, and composite FKs help, but any missed predicate in raw SQL can become cross-tenant exposure. Stage 13B invoice line references to PO/receipt items are single-column FKs; they should be company-scoped or validated by transactional service logic.
6. **Auditability for financial actions is incomplete.** There is no generalized audit log and Stage 13B records lack a uniform immutable posting/reversal workflow. `updated_at`/version fields are not a substitute for an actor/action history.

### Medium

7. **Idempotency coverage is uneven.** The core high-impact flows use keys and, in several cases, fingerprints/advisory locks. Draft creation/update and several simple writes do not. Every future financial write must define replay behavior and reject the same key with a different request payload.
8. **Concurrent credit-limit correctness needs focused tests.** Checkout locks the customer and consults the derived account balance before accepting unpaid amount, which is a sound serialization approach if all credit-changing operations acquire the same lock. Future customer receipts/credit notes must follow that lock discipline.
9. **Numeric scale and rounding policy needs an explicit contract.** SQL uses four decimal places and Rust `Decimal`, but API validation/rounding consistency should be specified and tested at boundaries. Avoid floating-point conversion in clients/adapters and ensure calculated line totals match database `ROUND(...,4)` constraints.
10. **Migration correctness is unverified against PostgreSQL.** Static review found extensive company scoping and checks, but no DB was changed or migration run. Stage 13B migration and all prior migrations need a clean-database and upgrade-path validation on a disposable PostgreSQL instance.
11. **List endpoints may load unbounded result sets and perform N+1 reads.** Several list services fetch IDs then load records individually; enforce pagination and consider batched queries.

## Recommended improvements

- Fix the compile error, then run formatter, full target checks and tests in CI before accepting Stage 13A/13B.
- Implement Stage 13B as transactional services first, then handlers/routes: payment/invoice/credit-note writes, allocation validation, idempotent replay fingerprint checks, sequence allocation, permissions, row locking and cash-session reconciliation. Keep posted source documents immutable and represent corrections with reversals/credit notes.
- Add database-backed integration tests using isolated disposable databases; do not point tests at a developer or production database. Cover migration chain and multi-company fixture isolation.
- Add concurrency tests for same idempotency key, simultaneous checkout/receipt/return, overselling, credit-limit consumption and cash-session close versus movement/payment.
- Review every SQL operation for company predicates and composite company FKs. Add missing composite constraints where feasible, particularly Stage 13B line-to-PO/receipt references and allocation relationships.
- Centralize decimal validation/rounding rules and assert API values cannot exceed precision/scale or produce totals inconsistent with persisted rounding.
- Add a structured audit trail for privileged and financial mutations (actor, company, entity, action, timestamp, request/correlation ID, before/after or immutable event payload); keep secrets/passwords out of it.
- Add bounded pagination, query indexes based on real filters, request timeouts/body limits, structured tracing, readiness checks and graceful shutdown.
- Review registration policy and deployment binding/configuration before production; ensure users cannot self-register into an unintended company or obtain privileged access.

## Exact files requiring changes

### Immediate compile repair

- `src/handlers/sales.rs` — fix the incompatible match-arm message type.

### Stage 13B implementation

- `src/routes/mod.rs`
- `src/handlers/mod.rs`
- `src/handlers/trading_parties.rs` (extend or add account handlers)
- `src/models/mod.rs`
- `src/models/trading_parties.rs` (or dedicated account models)
- `src/services/mod.rs`
- `src/services/trading_parties_service.rs` (or dedicated account services)
- `src/services/sales_service.rs` (credit balance/receipts consistency)
- `src/services/cash_register_service.rs` (reconciliation with new financial writes)
- `src/utils/permission_middleware.rs`
- `migrations/20261112100000_stage_13b_accounts_payables_receivables.sql` (follow-up additive constraints/indexes only after validating live migration state; do not edit/re-run an applied migration)
- `docs/STAGE13B_IMPLEMENTATION_NOTES.md`

### Quality/security and maintainability work

- `src/services/auth_service.rs`, `src/handlers/auth.rs`, `src/utils/auth_middleware.rs`, `src/utils/jwt.rs`, `src/main.rs` — registration policy, auth boundaries and production configuration review.
- `src/services/product_service.rs`, `src/services/inventory_service.rs`, `src/services/purchasing_service.rs`, `src/services/sales_service.rs`, `src/services/sales_returns_service.rs`, `src/services/cash_register_service.rs`, `src/services/trading_parties_service.rs` — tenant predicate, concurrency, pagination and decimal review.
- `migrations/*.sql` — company-FK and schema invariants, via new additive migrations rather than rewriting applied history.
- Add new `tests/` integration tests and service-level tests; no such files currently exist.

## Recommended implementation sequence

1. Repair `src/handlers/sales.rs` compile issue and run `cargo fmt` / `cargo check --all-targets`.
2. Freeze Stage 13A/13B migration state and validate the full migration chain on a disposable PostgreSQL database; do not mutate the existing developer database as part of the audit.
3. Specify Stage 13B API contracts, state transitions, allocation and credit-note semantics, idempotency replay rules, and permission matrix.
4. Implement transactional account services (customer receipts/allocations, supplier invoice draft/post, supplier payment/allocations, credit notes) with tenant-scoped locks, validation and immutable posting rules.
5. Add handlers/models/routes and connect permissions; ensure cash-session calculations use only source financial records once.
6. Add isolated integration and concurrency tests covering tenant isolation, balances, idempotency, rollback, cash reconciliation and credit limits.
7. Add database constraints through a new migration where service checks alone are insufficient; verify clean install and upgrade path.
8. Add pagination, audit events, operational readiness and deployment configuration; run CI checks before release.

## Verification performed

- Repository inventory and source/migration inspection completed; environment files and logs were excluded from file listing and no secret values were read.
- `cargo check --all-targets`: **failed** at `src/handlers/sales.rs:54` with E0308 due to `String` versus `&str` in the error response match.
- `cargo test --all-targets`: started but the audit cannot report a passing test suite; no test files/modules were found and the build is blocked by the compile error. No database commands, migrations, resets, drops or writes were run.
