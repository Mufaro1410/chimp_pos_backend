# Milestone 4 — API Completion and Consistency

## Status

Milestone 4 remains **incomplete**. Milestone 4A typed contracts are documented below; pagination/filtering and response consistency still require implementation and review. No PostgreSQL behavior is claimed as verified.

## Milestone 4A — Typed Contract Completion

All 50 operations that were route inventory placeholders now have operation-specific summaries, implemented request/response schemas, handler-observed success status codes, relevant error response codes, path/query parameters, bearer/permission metadata, and request/response examples. Schemas for request and response DTOs were derived from the public serialized/deserialized Rust model fields; runtime enum values were added only where service validation defines them. The exact `204` no-body endpoints remain bodyless. Replay-sensitive return/refund/void/receipt endpoints document both `201` creation and `200` replay responses. Checkout documents its actual `200` response.

The extended route checker now parses `required_permission` rules from `src/utils/permission_middleware.rs` and compares the exact mapped permission to `x-required-permission`. It derives protected, auth-only, and public route sets from the router's authentication middleware layers, rather than maintaining a separate public route allowlist. It also checks request and successful-response schemas, local `$ref` targets, model field coverage, duplicate YAML keys, and missing/stale routes.

Contracts that remain inherently ambiguous from the current implementation:

- `403` may be an empty middleware response or a handler `ApiErrorResponse` for business permission checks. The existing API therefore has no single exact body contract for every 403. The shared `SecurityError` description preserves the middleware's empty response behavior; business-specific 403 bodies remain an API consistency limitation.
- Some Rust DTO fields are plain `String`/`Decimal` with their business rules enforced in service code, not type-level constraints. Where service behavior is a fixed accepted set, the schema documents it; other semantic validation is described only when established by handlers/services.
- The report handlers return `serde_json::Value`, so their schemas remain report-family aggregate schemas rather than model-derived DTOs. Historical and dynamically aggregated fields are documented according to the report contract already described in this document; service changes can still create drift.
- `GET /health` actively queries PostgreSQL and can return an empty `503`; it is public but not an unauthenticated database-independent liveness check.

Validation for this sub-milestone:

- `python3 scripts/validate_openapi.py` — passed for all 97 operations, including route/auth/permission mapping, request/success schema presence, local references, duplicate keys, and Rust DTO field sets.
- `openapi-spec-validator 0.9.0 docs/openapi.yaml` — passed formal OpenAPI 3.1 validation. The validator was installed only in a temporary `/tmp` virtual environment.
- The schemas were generated/updated with `scripts/sync_openapi_rust_models.py` and `scripts/type_openapi_placeholders.py`; examples and operation-specific statuses were then reviewed against handler signatures and idempotency response branches.

## Complete API endpoint inventory

The router currently registers 97 method/path operations:

| Area | Operations |
|---|---|
| Health and authentication | `GET /health`; `POST /api/v1/auth/register`; `POST /api/v1/auth/login`; `GET /api/v1/auth/me` |
| Shop, users, roles | `GET, PUT /api/v1/shop`; `GET, POST /api/v1/users`; `GET, PUT /api/v1/users/{id}`; `PUT /api/v1/users/{id}/roles`; `GET /api/v1/roles`; `GET /api/v1/roles/{id}`; `PUT /api/v1/roles/{id}/permissions`; `GET /api/v1/permissions` |
| Products and variants | `GET, POST /api/v1/products`; `GET, PUT, DELETE /api/v1/products/{id}`; `GET, POST /api/v1/products/{product_id}/variants`; `PUT, DELETE /api/v1/products/{product_id}/variants/{variant_id}` |
| Inventory | `GET /api/v1/inventory/stock`; `GET /api/v1/inventory/movements`; `POST /api/v1/inventory/adjustments` |
| Purchasing and receiving | `GET, POST /api/v1/purchases`; `GET, PUT /api/v1/purchases/{id}`; `POST /api/v1/purchases/{id}/submit`; `POST /api/v1/purchases/{id}/cancel`; `GET, POST /api/v1/purchases/{id}/receipts` |
| Customers and suppliers | `GET, POST /api/v1/customers`; `PUT /api/v1/customers/{id}`; `PATCH /api/v1/customers/{id}/credit-policy`; `GET, POST /api/v1/suppliers`; `PUT /api/v1/suppliers/{id}` |
| Customer/supplier accounts | `GET /api/v1/customers/{id}/account`, `/statement`, `/outstanding-sales`; `POST /api/v1/customers/{id}/payments`, `/credit-notes`; `GET /api/v1/suppliers/{id}/account`, `/statement`, `/outstanding-invoices`, `/invoices`; `POST /api/v1/suppliers/{id}/invoices`, `/payments`, `/credit-notes`; `GET /api/v1/supplier-invoices/{id}` |
| Sales, checkout and after-sale | `GET, POST /api/v1/sales`; `GET, PUT /api/v1/sales/{id}`; `POST /api/v1/sales/{id}/checkout`, `/cancel`; `GET /api/v1/sales/{id}/receipt`, `/refunds`, `/returns`, `/void`; `POST /api/v1/sales/{id}/returns`, `/void`; `POST /api/v1/sales/returns/{return_id}/refunds` |
| Cash registers | `GET, POST /api/v1/cash-registers`; `POST /api/v1/cash-registers/{register_id}/sessions`; `GET /api/v1/cash-registers/{register_id}/sessions/open`; `GET /api/v1/cash-sessions/{session_id}`, `/summary`, `/movements`; `POST /api/v1/cash-sessions/{session_id}/movements`, `/close` |
| Reporting | `GET /api/v1/reports/dashboard`; sales `summary`, `by-product`, `by-category`, `by-payment-method`, `transactions`, `returns`, `gross-profit`; inventory `stock`, `valuation`, `movements`, `activity`; purchasing `summary`, `by-supplier`, `goods-received`, `outstanding-orders`; supplier `purchase-history`; cash `registers`, `sessions`, `movements` |

Category management and dedicated barcode lookup/management routes are not registered. Products and variants carry category, SKU, and barcode values through their existing models. No symmetric CRUD endpoints were added without an existing business need.

## Changes made

- Added typed OpenAPI contracts for the 50 previously undocumented operations. These contracts were generated from a manually maintained operation-to-DTO mapping and then checked against the Rust DTO model definitions. They still require handler-level review for exact response DTO selection, status codes, and error behaviour.
- Added [`scripts/validate_openapi.py`](../scripts/validate_openapi.py), which compares Axum method/path declarations with OpenAPI, parses actual Rust permission rules, derives authentication-layer coverage, checks request/success schema presence and local references, compares configured DTO fields/types/requiredness to Rust models, and rejects duplicate YAML mapping keys.
- Added [`scripts/type_openapi_placeholders.py`](../scripts/type_openapi_placeholders.py) to generate typed contracts for the 50 previously undocumented operations. The operation-to-schema mapping is manually maintained and is not itself proof that a handler returns that DTO.
- Added [`scripts/add_openapi_route_inventory.py`](../scripts/add_openapi_route_inventory.py) as a one-time helper for generating route inventory stubs. It is not a source of truth for handler schemas; review generated output before rerunning it.
- No Rust handler behavior or database schema was changed in this milestone.

## Authentication, permissions, and tenant scope

- Public operations: `GET /health`, `POST /api/v1/auth/register`, and `POST /api/v1/auth/login`.
- `GET /api/v1/auth/me` requires a valid bearer token but no business permission.
- All other registered routes are behind authentication and the existing permission middleware. Each protected operation documents `x-required-permission`. The custom checker parses `required_permission` in `src/utils/permission_middleware.rs` and compares the extracted permission value with the OpenAPI extension for each registered operation. This is a source-pattern parser, not a Rust compiler or runtime authorization test.
- JWT access tokens are bearer tokens with a 900 second lifetime. No refresh or token rotation endpoint is implemented. Clients must send users through login again after expiry.
- The authenticated company ID is supplied by verified JWT context; services scope data access by that company. Cross-company database behavior was not verified in this milestone.
- User response models omit password hashes and authentication secrets. Registration and user creation remain public-to-registration/admin operations respectively; admin user creation accepts an initial password supplied by an administrator and has no invitation/reset delivery flow.
- Middleware currently returns bare HTTP status errors for authentication and permission failures, while most handlers return `{ error: { code, message } }`. This is an existing response-contract inconsistency.

## Pagination and filters

| Endpoint group | Implemented query support |
|---|---|
| Users | `limit`, `offset` |
| Purchase orders | `supplier_id`, `status`, `limit`, `offset` |
| Inventory stock | `location_id`, `variant_id`; no pagination |
| Inventory movements | `location_id`, `variant_id`, `limit`, `offset` |
| Sales | `status`, `location_id`, `limit`, `offset` |
| Customer and supplier directories | No search, filters, or pagination; returns full company list |
| Product and variant lists | No search, category/status filters, or pagination; product list returns full company list |
| Cash registers | `location_id`; no pagination |
| Customer/supplier statements and outstanding documents | `limit`, `offset` |
| Reports | Date range, interval, location, product variant, supplier, status, threshold, payment method, and/or bounded pagination according to each report query |

Filters are not uniformly validated or standardized. The most material client-facing gap is unbounded product/customer/supplier/sales and cash-register list responses. Sorting is not generally supported. Adding category CRUD, barcode lookup, more search, or filter options should follow product requirements and a concrete client workflow.

## Request, response, and serialization consistency

- Financial amounts and quantities use Rust `Decimal`; serialization is currently mixed between decimal strings and JSON numbers across endpoint families. Clients should parse amounts with decimal-safe handling rather than binary floating-point arithmetic.
- IDs use UUIDs for company/user/role identity and integer IDs for most product, sales, purchasing, inventory, cash, and party records.
- Date-only fields serialize as ISO dates; timestamps use UTC/RFC 3339 date-time values.
- Not-found behavior is generally 404 for entity reads, but error envelopes/status handling are not uniform across authentication middleware and legacy handler paths.
- The 50 newly documented operations now have typed request/response schemas, examples, and documented success/error response entries. The schemas are based on a manually maintained operation-to-model mapping. The checker verifies configured DTO fields/types/requiredness, but it does not prove that each operation references the exact DTO returned by its handler, nor does it fully verify documented status codes, examples, or error bodies against runtime behaviour. Handler-level contract review remains necessary.
- Existing request validation varies by model. Some newer models reject unknown JSON fields; older models do not. Axum extractor rejections may use framework error bodies rather than the API error envelope.

## Frontend integration requirements

- Configure an API base URL per environment; local backend currently binds to `http://127.0.0.1:3000`.
- Send `Authorization: Bearer <access_token>` on private routes. Keep the 15-minute token in platform-appropriate protected storage, avoid logging it, and prompt for login after 401/expiry. There is no refresh token flow.
- Load `/api/v1/auth/me` after login for user/company identity, then `/api/v1/shop` for tenant settings. Role/permission catalogue routes are available to authorized administrators.
- Tauri-specific capabilities should be behind adapters so the same React client can run in a browser. No direct PostgreSQL access is supported or appropriate.
- The server does not install a CORS layer. Browser/web deployment must configure an explicit trusted-origin CORS policy at the service/proxy layer before web clients call the API. Do not use wildcard origins with credentials.
- Handle 403 as an authorization outcome, 404 as a missing or out-of-scope resource, 409 as a state/version conflict, and parse the common handler error envelope where present. Authentication middleware currently emits status-only failures.

## OpenAPI and validation

`docs/openapi.yaml` now lists the registered 97 method/path operations. A structural validation run passed:

```text
python3 scripts/validate_openapi.py
OpenAPI structure and route coverage valid (97 operations).
```

The previous Codex run recorded `openapi-spec-validator 0.9.0` as installed and passing. That result conflicts with the later test summary in the earlier version of this report, which said the validator was unavailable. In the review environment used for the latest corrections, `openapi-spec-validator` was not installed, so the formal validator could not be independently rerun. Install it in a Python virtual environment and rerun it before marking formal validation complete. The custom checker validates route and permission relationships beyond formal OpenAPI syntax validation.

## Tests and results

- `cargo fmt --check` — passed.
- `cargo check --all-targets` — passed.
- `cargo test --all-targets` — passed: 10 unit tests; the opt-in PostgreSQL integration test binaries reported success but **skipped their database scenarios** because their dedicated test database environment variables were not configured. No PostgreSQL behavior is claimed as verified.
- `git diff --check` — passed.
- `python3 scripts/validate_openapi.py` — passed for all 97 registered operations after the contract corrections described below.
- Formal OpenAPI validator — previously reported as passing by Codex, but not independently rerun in the latest review environment because `openapi-spec-validator` is not installed there. Re-run it locally to resolve the conflicting historical record.
- The latest changes touched only OpenAPI generation/documentation and validation-script documentation/imports. Rust code, migrations, and databases were not changed or accessed.

## Corrections made during review

- Fixed the generator condition that accidentally added a `201` response to `GET /api/v1/sales/{id}/refunds` and `GET /api/v1/sales/{id}/void`. The idempotent `200`/creation `201` pair is now limited to the relevant POST operations.
- Corrected the public `GET /health` description and example to match `src/handlers/health.rs`: `{ "status": "ok", "database": "connected" }` on success, and an empty `503` response when the database query fails.
- Updated this report to remove stale claims that the 50 operations remain inventory placeholders and that permission values are checked only by manual review.
- Corrected the validator script's module description to reflect the DTO comparison it actually performs. The validator still has the limitations stated above.

## Outstanding limitations

1. Review all 50 operation-to-DTO mappings against their actual Rust handlers, especially exact success status codes, response DTOs, and error behaviour. The current checker does not prove this mapping end-to-end.
2. Decide and implement pagination/search for high-volume product, customer, supplier, sales, and cash-register lists; no client-specific search semantics are assumed here.
3. Standardize framework extractor errors and authentication/permission failures with the handler error envelope without breaking existing consumers. Middleware returns empty status responses for authentication/authorization failures; permission lookup database failures also return an empty `500`, while many handlers use the API error envelope.
4. Define and configure allowed web origins before enabling browser deployment; no refresh-token flow exists.
5. Add integration/contract tests for route access, tenant isolation, and response schemas against a dedicated disposable PostgreSQL database. PostgreSQL validation remains deferred; `chimp_pos_dev` and all migrations were left untouched.
