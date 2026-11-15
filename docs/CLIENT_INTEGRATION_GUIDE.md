# ChimpPOS client integration guide (web, desktop, mobile)

Base URL `…/api/v1`. All bodies are JSON. Money and quantities are **strings** (`"12.5000"`) – never parse them as floats.

Every error has the same shape:
```json
{ "error": { "code": "INSUFFICIENT_STOCK", "message": "…" } }
```
Branch on `code`, never on `message`. Every response carries `x-request-id`; log it and quote it in bug reports.

---
## 1. Authentication & sessions

| Call | Purpose |
|---|---|
| `POST /auth/register` | Create company + owner (public) |
| `POST /auth/login` `{company_id, username, password, device_label?}` | → `access_token`, `refresh_token`, `expires_in` (s), `refresh_expires_in` (s), `must_change_password`, `user` |
| `POST /auth/refresh` `{refresh_token, device_label?}` | Rotate: returns a **new** access + refresh token |
| `POST /auth/logout` `{refresh_token}` | End this device's session (always 204) |
| `POST /auth/logout-all` | End every session of the user, kills access tokens immediately |
| `GET  /auth/me` | `user_id, name, username, roles[], permissions[], must_change_password` – use `permissions` to show/hide UI |
| `POST /auth/change-password` `{current_password,new_password}` | Returns a fresh token pair; all *other* devices are signed out |
| `POST /auth/forgot-password` `{company_id, identifier}` → 202 always | Starts self-service reset (see note) |
| `POST /auth/reset-password` `{token,new_password}` → 204 | Completes it |
| `POST /users/{id}/reset-password` `{new_password, require_change=true}` → 204 | Manager/admin sets a temporary password (`users.manage`) |

**Client rules**
1. Keep the **refresh token in secure storage** (Keychain / Keystore / Tauri stronghold; never localStorage). Keep the access token in memory.
2. Refresh when ~80 % of `expires_in` has passed or on any `401`. Do it **single-flight** (one refresh at a time per device) and **persist the new refresh token before anything else** – the old one is dead after the grace window (30 s).
3. `401 INVALID_REFRESH_TOKEN` ⇒ the session is over: show the login screen. (It also happens if a refresh token is replayed – that revokes the whole session on purpose.)
4. `must_change_password = true` (after an admin reset) ⇒ send the user to *change password*. Until then every other call returns `403 PASSWORD_CHANGE_REQUIRED`.
5. `429 ACCOUNT_LOCKED` (5 wrong passwords → 15 min) and `429 RATE_LIMITED` (+ `Retry-After` header): show the wait, do not retry in a loop.
6. Passwords: 15–128 characters, not equal to the username. Show the rule; the server returns `WEAK_PASSWORD`.
7. Company lookup: login needs the company UUID – store it on the device after first sign-in (or add a company-code lookup later).

*Self-service reset*: the token is only delivered by the server log in development (`PASSWORD_RESET_DELIVERY=log`). **Email/SMS delivery is not wired up**; until it is, shops reset passwords through a manager (`/users/{id}/reset-password`) or the operator CLI (`chimpPOS_backend reset-password <company_id> <username>`).

---
## 2. Offline operation

Design: the device keeps a **local copy** of the catalogue (pull) and an **outbox** of operations made while offline (push). Everything is idempotent, so retrying is always safe.

### 2.1 First start / new device
```
GET /sync/bootstrap  →  { server_time, company{…}, units[], permissions[], entities[], operation_types[], max_operations_per_push }
for entity in [locations, categories, products, variants, stock, customers, registers]:
    cursor = null
    loop:
        GET /sync/pull/{entity}?limit=1000&cursor={cursor}
        upsert page.items into the local DB
        cursor = page.next_cursor
        until page.has_more == false
    save cursor for the entity
```
Items are keyed by `id` (stock by `(location_id, variant_id)`). `variants` hold prices (`selling_price`), `products` the name/category, `stock` the on-hand quantity. Rows with `is_active=false` are deactivations: keep them locally but hide them (there are no hard deletes).

### 2.2 Staying up to date
Repeat the same pull with the **saved cursor** every 30–60 s while online, on app focus, and right after a successful push. The feed deliberately re-sends rows from the last ~60 s (so a late-committing write is never missed): **upsert by id, never "insert"**. Only advance a saved cursor after the page was stored. If the server says `INVALID_CURSOR`, discard that entity's cursor and re-pull it from scratch.

Which permission each feed needs: `products.view` (locations, categories, products, variants), `inventory.view` (stock), `customers.view`, `cash_register.view`. A `403` on one feed just means that role does not use it.

### 2.3 Selling while offline
For each sale create **one operation** at the moment of sale and store it in the outbox with a fresh UUID v4 `client_op_id`:
```json
{ "client_op_id": "…", "op_type": "sale.checkout", "client_created_at": "2026-11-14T09:30:00Z",
  "payload": { "location_id": 1, "register_id": 1, "customer_id": null,
               "items":    [ {"variant_id": 12, "quantity": "2", "unit_price": "1.50"} ],
               "payments": [ {"payment_method": "cash", "amount": "3.00", "tendered_amount": "5.00"} ] } }
```
Print a **local receipt number** that is clearly not the server's; the server assigns the real `sale_number` on sync and returns it.

| `op_type` | payload |
|---|---|
| `sale.checkout` | as above (needs `sales.create` + `sales.checkout`; credit sales also need `customers.credit`) |
| `expense.create` | same fields as `POST /expenses` (no `idempotency_key` needed) |
| `stock.adjust` | same fields as `POST /inventory/adjustments` |
| `customer.payment` | `{customer_id, …POST /customers/{id}/payments body}` |
| `cash.movement` | `{session_id, …POST /cash-sessions/{id}/movements body}` |

**Open a cash session while online**, before going offline. Offline sales are attached to the register's open session.

### 2.4 Uploading
```
POST /sync/push   { "device_id": "tablet-1", "operations": [ … up to 100, in the order they happened … ] }
→ { "results": [ { client_op_id, op_type, status, result, error{code,message}, retryable } ], "server_time" }
```
Handle each result by `status`:

| status | meaning | client action |
|---|---|---|
| `applied` | done | store `result` (`sale_id`, `sale_number`…) against the local record; remove from outbox |
| `duplicate` | done earlier (a retry) | same as `applied` |
| `in_progress` | another request is still processing it | resend later (a few seconds) |
| `error` + `retryable:true` | transient server problem | resend **the identical operation** later |
| `rejected` | a business rule failed; **will not succeed on its own** | move to a "needs attention" list, show `error.code`, continue with the next operations |

Rules: never edit an operation after its first send (same `client_op_id` + different payload ⇒ `IDEMPOTENCY_CONFLICT`); never reuse an id; send in order; a request with a repeated id or more than 100 operations is rejected whole (`400`).

### 2.5 Conflicts a manager must resolve
A rejected operation is **kept on the server**, nothing is lost. Common `error.code`s: `INSUFFICIENT_STOCK` (offline sales exceeded server stock), `DISCOUNT_PERMISSION_REQUIRED` (sold below the *current* list price because the cashier's cached price was stale), `PERMISSION_DENIED`, `CASH_SESSION…` (session closed meanwhile).

```
GET  /sync/operations?status=rejected        (sync.use: own operations; sync.manage: everyone's)
POST /sync/operations/{id}/retry             (sync.manage) re-run as the original user, after fixing the cause (e.g. a stock adjustment)
POST /sync/operations/{id}/retry {"run_as_reviewer": true}
                                             re-run under the manager's authority – use for price conflicts; the sale is
                                             attributed to the manager and the journal records the approval
POST /sync/operations/{id}/dismiss {"note":"…"}   give up (goods never left, duplicate, …)
```
Build a small "Sync issues" screen on top of these; show the original `payload` so the manager sees what was sold.

### 2.6 Closing the day
Do not close a cash session while the outbox still holds operations for it: push first, resolve the rejected ones, then `POST /cash-sessions/{id}/close`.

### 2.7 What is *not* offline
Creating products, customers, users, purchase orders, supplier invoices, returns/refunds/voids and reports need a connection. Stock quantities in the local copy are a snapshot: they can be stale, which is why oversell conflicts exist.
