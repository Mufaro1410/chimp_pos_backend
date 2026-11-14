# Functional Backend Readiness for Desktop-First Delivery

Assessment date: 2026-11-12  
Scope: backend crate and current working tree. This assessment does not inspect or modify a database. Stage 13A/13B migrations present in the working tree remain unapplied unless separately verified; do not infer their live status from this document.

## Delivery architecture

1. **Shared backend:** Rust/Axum/PostgreSQL owns authentication, authorization, validation, business rules, transactions, and persistence.
2. **Primary client:** React + TypeScript + Tauri desktop application for administration and POS.
3. **Web deployment:** reuse the React frontend where practical. Browser clients use the same versioned REST API and do not connect directly to PostgreSQL.
4. **Later mobile stage:** retain the existing Expo/React Native app unchanged until its planned integration stage. It will use the same backend for selling and selected operational tasks, with offline selling/synchronization designed as a specific API capability.

Tauri capabilities such as local printing, hardware access, and filesystem integration must sit behind frontend adapters. The shared React application must remain usable without those adapters in a browser. Shared rules belong in the backend; clients may provide presentation and local interaction logic but must not independently implement financial or inventory decisions.

## Readiness decision

**Backend foundation: partially ready. Client integration and production readiness: not yet established.** Existing REST slices can support early contract definition and limited integration work after build/test status is verified. Do not treat this as approval to start the Tauri implementation or to enable account posting. The repository contains the backend crate; it does not establish the state of client applications elsewhere.

The backend uses Rust 2024, Axum, SQLx/PostgreSQL, decimal monetary values, JWT authentication, and Argon2 password hashing. Routes are versioned under `/api/v1`; authenticated route groups run authentication and permission middleware. Services own most database transactions. Tenant IDs are derived from authenticated identity and are commonly applied in SQL; PostgreSQL row-level security was not found in the repository audit.

## Capability assessment against desktop priorities

| Desktop capability | Current backend evidence | Readiness |
|---|---|---|
| Authentication | Register, login, and authenticated identity (`/api/v1/auth/register`, `/login`, `/me`); JWT and password hashing | Foundation exists; registration policy and deployment security need review |
| User and role administration | New shop-scoped `/api/v1/users`, `/api/v1/roles`, and `/api/v1/permissions` endpoints; role assignment/custom-role permission editing use existing RBAC tables | Implemented locally; database-backed integration suite is opt-in and has not run in this environment |
| Shop configuration | New authenticated `/api/v1/shop` GET/PUT uses the existing company configuration columns and optimistic versioning | Implemented locally; currency changes are blocked when known financial/purchasing records exist; integration test remains pending |
| Dashboard | Health endpoint only | No operational dashboard/aggregate API identified |
| Products, categories, variants | Product and variant routes/services; category/unit/location schema | Product/variant foundation exists; verify category/configuration management and API pagination |
| Barcode management | No dedicated barcode management API established by this audit | Contract and persistence behavior need confirmation |
| Inventory and adjustments | Stock, movement, adjustment endpoints; service transactions and locking | Core functions exist; add client contract and automated regression coverage |
| Suppliers and purchasing | Supplier profiles and purchase order lifecycle | Supplier profile/purchasing foundations exist; full supplier financial account workflow is incomplete |
| Goods receiving | Purchase receipt endpoint and transactional stock movement flow | Foundation exists; needs integration coverage and stable response contract |
| Sales and checkout | Draft/list/detail/update/checkout/receipt/cancel; idempotency and stock/session logic | Core POS flow exists; test coverage and API contract are release gates |
| Returns and refunds | Return, refund, void, list/detail paths | Core routes exist; verify behavior with automated DB tests and finalized API contract |
| Cash registers | Register/session open/current/summary/movements/close | Core routes exist; concurrency and reconciliation behavior needs demonstrated tests |
| Customer management | Company-scoped profile APIs; credit policy separated behind `customers.credit` | Profile and policy APIs exist locally; migration and permission changes remain subject to Stage 13B gates |
| Reports | No general reporting endpoints identified | Not ready; define report needs, filters, limits, and data semantics |

The route inventory is based on `src/routes/mod.rs` and the corresponding handlers/services in this working tree. It is not a claim that every route has been validated against an applied schema or deployed API.

## Shared API and browser-readiness gates

Before frontend feature work depends on an endpoint, publish and maintain a versioned contract covering request/response schemas, status/error shapes, authentication, permissions, tenant behavior, idempotency, pagination, decimal/currency representation, and compatibility policy. Generate or validate an OpenAPI description from the same source as the handlers so desktop, web, and later mobile clients consume one contract.

The current repository audit did not establish a maintained OpenAPI contract, bounded pagination across list endpoints, or a general audit-event stream. These should be addressed as backend platform work. Define CORS/origin policy for browser deployment and deployment configuration for API binding, TLS termination, secrets, timeouts, request limits, readiness, and graceful shutdown. The current application binds to loopback in `main`; production exposure requires deliberate deployment configuration.

Frontend-facing contracts should use decimal-safe representations (never binary floating point for money), stable error codes, and explicit retry/idempotency behavior. Keep Tauri APIs behind adapters and ensure browser implementations/fallbacks exist. Offline mobile selling is a later backend design item: define operation IDs, conflict resolution, catalog/price versioning, stock reconciliation, and replay semantics before promising offline checkout.

## Stage 13B financial boundary

The working tree contains Stage 13B account schema and local remediation work, including reconciliation approval gates, reversal event structures, customer credit-policy separation, and a transactional reversal service. The account posting and reversal operations do not yet have dedicated HTTP routes. The new migration files are pending local work and must not be treated as applied. Do not expose direct database access to any client, and do not enable account posting based only on the presence of schema or service code.

The latest local Stage 13B review records unresolved historical currency and cash-session decisions and an unrun PostgreSQL integration suite. It also identifies account invoice/payment/allocation/credit-note workflows as incomplete. Therefore customer/supplier balances and account transactions are not ready for a desktop workflow. Follow `STAGE13B_MIGRATION_REVIEW.md` and `STAGE13B_HISTORICAL_RECONCILIATION.md`; preserve their no-apply gate until authorized reconciliation, migration review, and disposable PostgreSQL integration tests are complete.

## Quality and operational readiness

The repository includes an opt-in Stage 13B PostgreSQL test suite, but its PostgreSQL scenarios have not been demonstrated against a disposable test database according to the current hardening/review documents. Static tests are not evidence of concurrency safety or migration behavior. The initial backend audit reports that `cargo check --all-targets` failed due to a type mismatch in `src/handlers/sales.rs`; a later local hardening note reports checks passed. Because these records conflict and no checks were run for this readiness document, current build status is **unverified** and must be reconciled by a fresh CI run.

Readiness also requires automated coverage for tenant isolation, permissions, idempotent retries, concurrent stock/credit/allocation operations, checkout/returns/refunds, cash-session close races, rollback, and migration clean-install/upgrade paths. Database tests must use a dedicated disposable database and must never target `chimp_pos_dev`. Add bounded pagination, structured tracing without secrets, audit events for sensitive mutations, and database readiness/shutdown behavior before deployment.

## Recommended delivery sequence

1. **Stabilize backend baseline:** reconcile current compile/test status in CI; resolve existing local build errors without discarding uncommitted work. Establish clean-install and upgrade validation against disposable PostgreSQL databases.
2. **Freeze and validate existing API slices:** document auth, catalog, inventory, purchasing/receiving, sales, returns/refunds, and cash contracts; test tenant scoping, permission rules, idempotency, and concurrency.
3. **Complete Stage 13B safely:** obtain business-owner decisions in the historical reconciliation report; review the unapplied migrations; test them and reversal/allocation concurrency on disposable PostgreSQL; implement account posting endpoints and service transactions; do not apply them to `chimp_pos_dev` without the separate approval process.
4. **Close desktop administration gaps:** validate the new administration APIs against disposable PostgreSQL, then define dashboard and reporting APIs and confirm barcode behavior.
5. **Establish shared client contract:** publish API schema, error/pagination/currency policies, browser CORS/deployment requirements, and client compatibility rules. Then begin the Tauri React application with browser-safe adapters.
6. **Integrate web and mobile deliberately:** deploy the shared React frontend to web where practical; keep Expo intact until its integration stage, then consume the same contracts. Design and test offline selling/synchronization as a later explicit capability.

## Outstanding readiness blockers

- Current build status is unverified because repository audit notes conflict; fresh formatting, compilation, and automated CI results are needed.
- Stage 13B migration approval is blocked on seven historical sale currency approvals, four cash-event session dispositions, review of cancelled sale histories/session close treatment, RBAC review, and disposable PostgreSQL test execution.
- Customer/supplier account posting APIs and complete transactional workflows are not available for client integration.
- Dashboard and reports are not established as backend APIs; barcode API coverage needs confirmation. New user/role and shop configuration APIs still need a passing disposable PostgreSQL integration run.
- A stable published REST contract, consistent bounded pagination, and browser deployment policy are not established by the inspected backend tree.
- Tenant-isolation and critical financial concurrency behavior require demonstrated integration tests; no production-readiness conclusion should be drawn from static SQL/code inspection.
- Operational deployment configuration, audit coverage, and offline mobile synchronization semantics remain to be completed or explicitly accepted.

No Tauri application work, migrations, or database operations were performed for this assessment.
