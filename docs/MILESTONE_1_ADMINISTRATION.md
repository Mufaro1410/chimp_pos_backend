# Backend Implementation — Milestone 1: Administration

Date: 2026-11-12  
Status: endpoints implemented locally; PostgreSQL behavior is not yet verified against a disposable database. No pending or applied migration was changed or run, and `chimp_pos_dev` was not accessed.

## Implemented endpoints

All new routes are under `/api/v1`. Tenant identity is taken from the authenticated JWT context, never from the request body.

| Method and path | Purpose | Required permission |
|---|---|---|
| `GET /api/v1/shop` | Read authenticated shop details/configuration | `settings.view` |
| `PUT /api/v1/shop` | Update shop details using `expected_version` | `settings.manage` |
| `GET /api/v1/users?limit=&offset=` | List shop users (default limit 50, max 100) | `users.view` |
| `POST /api/v1/users` | Create active user and assign initial shop roles | `users.manage` |
| `GET /api/v1/users/{id}` | Retrieve shop user and assigned active roles | `users.view` |
| `PUT /api/v1/users/{id}` | Update name/email and activate, deactivate, or suspend | `users.manage` |
| `PUT /api/v1/users/{id}/roles` | Replace role assignments | `roles.manage` |
| `GET /api/v1/roles` | List shop roles and permissions | `roles.view` |
| `GET /api/v1/roles/{id}` | Retrieve a role and its permissions | `roles.view` |
| `PUT /api/v1/roles/{id}/permissions` | Replace permissions on an existing custom role | `roles.manage` |
| `GET /api/v1/permissions` | List the global permission catalogue | `roles.view` |

Authentication and existing auth endpoints are documented in [openapi.yaml](openapi.yaml): `POST /api/v1/auth/register`, `POST /api/v1/auth/login`, and `GET /api/v1/auth/me`.

## Authorization and safety behavior

- New endpoint routes use the existing authentication and permission middleware. No second authorization implementation was added.
- Owner/Admin/Manager/Cashier permission grants were not changed. Existing migration and registration seeding determine their current behavior: Owner has all permissions; Admin lacks `roles.manage` and `settings.manage`; Manager and Cashier retain their existing grants.
- Shop, user, role, and assignment queries use the authenticated `company_id`. Foreign-shop user/role identifiers are not returned or accepted for assignment.
- Role assignment and custom-role permission replacement are transactional. They validate all IDs against the current shop/catalog before replacing existing grants.
- Built-in system roles cannot be edited. Custom roles cannot be granted the owner-only `roles.manage` or `settings.manage` permission through the update endpoint.
- A company-row lock serializes user status changes, role assignment, and custom-role permission changes. The service rejects removing the last active user with `users.manage`, including self-deactivation and self-role removal. It also rejects removing `users.manage` from the final active administrator role.
- User responses do not include password hashes, tokens, or other authentication secrets. User profile writes use optimistic `expected_version` checks.
- Shop currency changes are rejected if products, customers, suppliers, sales, purchase orders, cash sessions, customer/supplier payments, or supplier invoices exist. This avoids silently reinterpreting existing price or account data; the database integration path remains to be run.
- Application validation covers field lengths/formats, email, usernames, initial password length, supported currency shape, country code, timezone, locale, unique role/permission IDs, and known status values. Errors raised by administration handlers use the existing `{ "error": { "code", "message" } }` structure. Middleware authentication/authorization failures remain bare status responses to preserve existing behavior.

## Database changes

None. Existing `companies`, `users`, `roles`, `permissions`, `role_permissions`, and `user_roles` support the implemented operations. No migration was created or applied. Existing migrations remain untouched.

## Files created or modified

Created:

- `src/models/administration.rs`
- `src/services/administration_service.rs`
- `src/handlers/administration.rs`
- `tests/administration_pg.rs`
- `docs/openapi.yaml`
- `docs/MILESTONE_1_ADMINISTRATION.md`

Modified:

- `src/models/mod.rs`
- `src/services/mod.rs`
- `src/handlers/mod.rs`
- `src/routes/mod.rs`
- `src/utils/permission_middleware.rs`
- `Cargo.toml` and `Cargo.lock` (test-only `tower` utility dependency)
- `docs/FUNCTIONAL_BACKEND_READINESS.md`

All pre-existing uncommitted files and pending migrations were preserved.

## Tests and results

- `cargo fmt --check`: **passed**.
- `cargo check --all-targets`: **passed**.
- `cargo test --all-targets`: **passed** (6 unit tests, 8 Stage 13B static contract tests, and two opt-in PostgreSQL test binaries completed successfully).
- `docs/openapi.yaml` parses as YAML. No OpenAPI semantic validator or generated contract-drift check is configured.
- Administration unit tests cover input validation and route-to-permission mapping. A router test confirms the administration endpoints reject requests without authentication.
- `tests/administration_pg.rs` covers tenant scoping, foreign-shop role rejection, shop versioning/update, user create/update, role assignment/custom permission update, invalid inputs, permission denial, and last-admin protection. It requires `CHIMPPOS_ADMIN_TEST_DATABASE_URL` and a database name beginning `chimp_pos_admin_test`.
- The PostgreSQL test body **was skipped** because no disposable administration database was configured. The existing Stage 13B PostgreSQL test was also skipped for missing test configuration. Their harnesses returning successfully does not demonstrate database behavior.
- No database connection, migration, or query against `chimp_pos_dev` was made as part of this milestone.

## Known limitations and outstanding decisions

1. User creation takes an administrator-supplied `initial_password`; no invitation email, password setup, reset, or forced-password-change flow exists. It is therefore user creation, not a complete invitation lifecycle.
2. There is no API to create or delete custom roles. Existing custom roles can have permissions updated; defining the custom-role lifecycle and naming policy remains outstanding.
3. Database integration behavior (including shop scoping, permissions, rollback, role races, and last-admin serialization) must pass against a freshly provisioned disposable PostgreSQL database before this functionality is considered verified.
4. Permission changes and user administration are not written to a general audit log; define and implement audit retention/actor requirements before production use.
5. Currency changes are intended for initial setup before operational records exist. They are checked transactionally, but other business write paths do not currently share the company-row lock; if concurrent first-time setup and sales/session creation is allowed operationally, add coordinated database enforcement before enabling that workflow.
6. The OpenAPI file is hand-maintained and not generated or contract-tested from handlers. Add CI validation and contract-drift checks.
7. User/role list assembly currently performs bounded follow-up reads for role details. Consider batching if query volume becomes material.

No Tauri client work was started. Milestone 1 should not be considered production-complete until the disposable PostgreSQL test is run and invitation/audit decisions are resolved or explicitly deferred.
