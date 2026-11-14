# ChimpPOS Stage 12 — Cash Register & Shift Management

This document is the acceptance checklist for Stage 12. Run it against a disposable development company/database, not production data.

## 1. Apply migrations

From the backend project root:

```bash
sqlx migrate run
```

The Stage 12 migration creates registers, sessions, manual cash movements, payment/refund links and permissions. The follow-up migration reconciles permissions using stable system-role keys.

Verify migrations:

```sql
SELECT version, description, success
FROM _sqlx_migrations
ORDER BY version;
```

Verify the Stage 12 tables and payment links:

```sql
SELECT table_name
FROM information_schema.tables
WHERE table_schema = 'public'
  AND table_name IN ('cash_registers', 'cash_register_sessions', 'cash_movements')
ORDER BY table_name;

SELECT table_name, column_name, data_type
FROM information_schema.columns
WHERE table_schema = 'public'
  AND table_name IN ('sale_payments', 'sale_refunds')
  AND column_name = 'cash_session_id'
ORDER BY table_name;
```

Verify role grants:

```sql
SELECT r.role_key, p.permission_key
FROM roles r
JOIN role_permissions rp ON rp.role_id = r.id
JOIN permissions p ON p.id = rp.permission_id
WHERE r.is_system_role = TRUE
  AND p.permission_key LIKE 'cash_register.%'
ORDER BY r.role_key, p.permission_key;
```

Expected: owner/admin have all five permissions; manager/cashier have view, open, close and movement, but not manage.

## 2. Start the API

```bash
cargo fmt --check
cargo check
cargo run
```

The API listens on `http://127.0.0.1:3000`.

Set your development token and IDs in the shell after logging in and creating/selecting an active inventory location:

```bash
export API=http://127.0.0.1:3000
export TOKEN='YOUR_ACCESS_TOKEN'
export LOCATION_ID='YOUR_ACTIVE_LOCATION_ID'
```

Use `-i` with curl to inspect HTTP status codes. Replace IDs returned by each request in later requests.

## 3. Register and open a shift

Create a register (Owner/Admin permission):

```bash
curl -i -X POST "$API/api/v1/cash-registers" \
  -H "Authorization: Bearer $TOKEN" \
  -H 'Content-Type: application/json' \
  -d '{"location_id":1,"code":"FRONT-01","name":"Front Counter"}'
```

Expected: `201 Created`. Save the returned `id` as `REGISTER_ID`.

Open the register session:

```bash
curl -i -X POST "$API/api/v1/cash-registers/$REGISTER_ID/sessions" \
  -H "Authorization: Bearer $TOKEN" \
  -H 'Content-Type: application/json' \
  -d '{"opening_float":50.00,"opening_notes":"Opening float counted"}'
```

Expected: `201 Created`. Save the returned `id` as `SESSION_ID`.

Try opening the same register again. Expected: `409 SESSION_ALREADY_OPEN`.

Get the open session:

```bash
curl -i "$API/api/v1/cash-registers/$REGISTER_ID/sessions/open" \
  -H "Authorization: Bearer $TOKEN"
```

Expected: `200 OK` and the same session ID.

## 4. Verify cash checkout integration

Create a normal draft sale using the existing sales API and an active product variant at the same location. Save its ID and version.

Cash checkout must include the register ID:

```bash
curl -i -X POST "$API/api/v1/sales/SALE_ID/checkout" \
  -H "Authorization: Bearer $TOKEN" \
  -H 'Content-Type: application/json' \
  -d '{
    "expected_version":1,
    "idempotency_key":"11111111-1111-4111-8111-111111111111",
    "register_id":1,
    "payments":[
      {"payment_method":"cash","amount":12.50,"tendered_amount":20.00}
    ]
  }'
```

Use the actual sale version, register ID and sale total. Expected: `200 OK` or the success status already used by the checkout handler. The cash payment must be linked to the active cash session and change given must be calculated.

Verify that the same request with the same idempotency key does not create another payment or stock movement. A retry using a different register or different payment details must return `409 IDEMPOTENCY_KEY_REUSED`.

Negative cases:

- Cash checkout without `register_id`: `409 CASH_REGISTER_REQUIRED`.
- Cash checkout against a register with no open session: `409 CASH_SESSION_NOT_OPEN`.
- Card/mobile-money-only checkout with `register_id` supplied: `400 INVALID_PAYMENT`.
- Card/mobile-money-only checkout without a register: succeeds without a cash-session link.

For mixed payments, only the cash payment row receives `cash_session_id`.

## 5. Verify cash movements and session summary

Record cash-in:

```bash
curl -i -X POST "$API/api/v1/cash-sessions/$SESSION_ID/movements" \
  -H "Authorization: Bearer $TOKEN" \
  -H 'Content-Type: application/json' \
  -d '{
    "movement_type":"cash_in",
    "amount":10.00,
    "reference":"FLOAT-ADJUSTMENT",
    "notes":"Additional float",
    "idempotency_key":"22222222-2222-4222-8222-222222222222"
  }'
```

Expected: `201 Created`. Repeat the same payload with the same key; it must return the original movement without duplication. Reuse the key with a different amount or session; expected: `409 IDEMPOTENCY_CONFLICT`.

Test `cash_out`, `paid_out` and `cash_drop`. An outgoing movement larger than available drawer cash must return `409 INSUFFICIENT_REGISTER_CASH`.

Get the session summary:

```bash
curl -i "$API/api/v1/cash-sessions/$SESSION_ID/summary" \
  -H "Authorization: Bearer $TOKEN"
```

Expected: `200 OK`, including cash sales, refunds, cash-in, cash-out, paid-outs and cash drops.

List movements:

```bash
curl -i "$API/api/v1/cash-sessions/$SESSION_ID/movements" \
  -H "Authorization: Bearer $TOKEN"
```

## 6. Verify cash refund and void integration

Create a valid return for a completed sale using the existing returns endpoint. Save the returned `return_id`.

Cash refund request:

```bash
curl -i -X POST "$API/api/v1/sales/returns/RETURN_ID/refunds" \
  -H "Authorization: Bearer $TOKEN" \
  -H 'Content-Type: application/json' \
  -d '{
    "idempotency_key":"33333333-3333-4333-8333-333333333333",
    "payment_method":"cash",
    "amount":5.00,
    "register_id":1,
    "reference":"REFUND-001"
  }'
```

Expected: cash refund is linked to the active session. The register must have enough available cash. A cash refund without a register or with a closed session must be rejected.

A non-cash refund must omit `register_id` and must not be linked to a cash session.

For a full sale void, use the existing void endpoint and supply refund lines that total the sale amount. If any refund line is cash, include `register_id`; the cash portion must be available in the drawer and linked to the session. Non-cash refund lines remain unlinked.

## 7. Close and reconcile the session

Close the session after all transactions are complete:

```bash
curl -i -X POST "$API/api/v1/cash-sessions/$SESSION_ID/close" \
  -H "Authorization: Bearer $TOKEN" \
  -H 'Content-Type: application/json' \
  -d '{"closing_cash_count":67.50,"closing_notes":"Counted by cashier and supervisor"}'
```

Expected: `200 OK`. The API stores:

```text
expected_cash = opening_float
              + cash_sales
              - cash_refunds
              + cash_in
              - cash_out
              - paid_out
              - cash_drops

cash_variance = closing_cash_count - expected_cash
```

Use the actual counted amount based on the transactions you performed. Verify that the returned `status` is `closed`, and `expected_cash`, `closing_cash_count` and `cash_variance` are populated.

After closing:

- New cash movements must be rejected.
- Cash checkout/refunds must not use the closed session.
- A new session can be opened on the same register.
- The old session and its movements remain readable.

## 8. Database integrity checks

```sql
-- No register can have more than one open session.
SELECT company_id, register_id, COUNT(*)
FROM cash_register_sessions
WHERE status = 'open'
GROUP BY company_id, register_id
HAVING COUNT(*) > 1;

-- Cash payment rows should have a session; non-cash rows should not.
SELECT id, payment_method, cash_session_id
FROM sale_payments
WHERE (payment_method = 'cash' AND cash_session_id IS NULL)
   OR (payment_method <> 'cash' AND cash_session_id IS NOT NULL);

-- Non-cash refunds should not have a cash session.
SELECT id, payment_method, cash_session_id
FROM sale_refunds
WHERE payment_method <> 'cash' AND cash_session_id IS NOT NULL;

-- Inspect session reconciliation.
SELECT id, status, opening_float, expected_cash,
       closing_cash_count, cash_variance
FROM cash_register_sessions
ORDER BY id DESC;
```

The first three queries should return no rows.

## 9. Role/permission acceptance

- Owner/Admin: list and create registers; open/close sessions; view summaries and movements; record movements; process sales/refunds according to their sales permissions.
- Manager: view registers/sessions, open/close sessions, record movements; cannot create/manage register setup.
- Cashier: view registers/sessions, open/close sessions, record movements and checkout sales; cannot create/manage register setup or perform refunds unless separately granted `sales.refund`.
- A user without the relevant permission must receive `403 Forbidden`.

## Stage 12 exit criteria

Do not start Stage 13 until all of the following are true:

- All migrations apply cleanly.
- `cargo fmt --check` and `cargo check` pass locally.
- Every workflow above has been exercised against PostgreSQL.
- The SQL integrity queries return no invalid rows.
- Permission tests pass for Owner, Admin, Manager and Cashier.
- Checkout, refunds, movements and retries create no duplicate financial records.
