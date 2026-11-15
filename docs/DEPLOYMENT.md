# ChimpPOS deployment & operations

Tested in development: build, migrations from scratch, all four test suites, production-mode config checks, HSTS/CORS/rate-limit,
TLS to PostgreSQL, backup → restore → boot. **Not tested here:** the Docker files, systemd unit and Caddy config (no Docker/systemd in the test environment) – do one dry run on a staging machine first.

## 1. What you need
* A Linux server (1 vCPU / 1 GB is enough for one shop), PostgreSQL 15+ (managed or local), a domain name, a reverse proxy that does HTTPS (Caddy config included).
* Rust ≥ 1.85 only to build; the result is one binary (~10 MB). Migrations are embedded in it.

## 2. Configuration (environment variables)
| Variable | Default | Notes |
|---|---|---|
| `APP_ENV` | development | `production` turns on safety checks (below), HSTS and the strict rate limit |
| `DATABASE_URL` | – | add `?sslmode=require` for remote/managed databases (TLS is built in) |
| `JWT_SECRET` | – | ≥ 32 chars, not a placeholder. `openssl rand -base64 48`. **Changing it signs everyone out** |
| `CORS_ALLOWED_ORIGINS` | allow all (dev) | **required in production**, comma separated, no `*`. Tauri: `tauri://localhost,http://tauri.localhost` |
| `HOST` / `PORT` | 127.0.0.1 / 3000 | keep `127.0.0.1` when the proxy is on the same machine |
| `TRUST_PROXY` | false | `true` only if your proxy sets `X-Forwarded-For` and is the only way in; used for per-client rate limiting |
| `RUN_MIGRATIONS` | false | `true` applies migrations at start; prefer `chimpPOS_backend migrate` |
| `JWT_TTL_SECONDS` / `REFRESH_TTL_DAYS` | 3600 / 30 | access / refresh lifetime |
| `AUTH_RATE_LIMIT_PER_MINUTE` | 30 prod, 600 dev | per client IP, login/refresh/reset endpoints |
| `LOGIN_MAX_ATTEMPTS` / `LOGIN_LOCKOUT_MINUTES` | 5 / 15 | per-account lockout |
| `DB_MAX_CONNECTIONS` | 10 | statement timeout 30 s and idle-in-transaction timeout 60 s are always applied |
| `ANALYZE_INTERVAL_SECONDS` | 300 | planner-statistics refresh (see §7) |
| `SYNC_RETENTION_DAYS` | 90 | applied/dismissed offline-journal rows older than this are purged; rejected ones are never purged |
| `LOG_FORMAT` / `RUST_LOG` | text / `warn,chimpPOS_backend=info,tower_http=info` | `json` for log shippers |
| `PASSWORD_RESET_DELIVERY` | (off) | `log` is development only and is **refused in production** |

In production the server refuses to start if: `JWT_SECRET` is short or looks like a placeholder, `CORS_ALLOWED_ORIGINS` is empty or contains `*`, or reset delivery is `log`.

## 3a. Docker Compose
```
cp .env.production.example .env.production    # fill in POSTGRES_PASSWORD, JWT_SECRET, CORS_ALLOWED_ORIGINS, API_DOMAIN
docker compose --env-file .env.production up -d --build
```
Order: db → `migrate` (one-shot) → api → caddy (automatic HTTPS on 80/443). Only Caddy is published.

## 3b. Bare metal / VM (systemd + Caddy)
```
cargo build --release                         # on the server or a CI box
sudo useradd --system --no-create-home chimppos
sudo install -m755 target/release/chimpPOS_backend /opt/chimppos/
sudo install -d -m750 -o root -g chimppos /etc/chimppos
sudo cp .env.production.example /etc/chimppos/chimppos.env   # edit; chmod 640
sudo cp deploy/chimppos.service /etc/systemd/system/ && sudo systemctl enable --now chimppos
sudo cp deploy/Caddyfile /etc/caddy/Caddyfile                # set your domain; reload caddy
```
The unit runs `migrate` before every start, so an upgrade is: replace the binary, `systemctl restart chimppos`.

## 4. First run checklist
1. `curl https://api.example.com/health` → `{"status":"ok","database":"connected"}`
2. Register the first company (`POST /auth/register`) **once**, then decide whether public registration should stay open (put it behind your proxy's allow-list or remove the route if only you create shops).
3. Run `python3 scripts/smoke_test.py https://api.example.com` against **staging**, not production (it creates throw-away companies).

## 5. Backups (do this before you have real data)
```
DATABASE_URL=postgres://… BACKUP_DIR=/var/backups/chimppos KEEP_DAYS=14 BACKUP_RSYNC_TARGET=user@other-host:/backups ./scripts/backup.sh
# cron: 30 2 * * * …/backup.sh >> /var/log/chimppos-backup.log 2>&1
```
Custom-format dump, verified after writing, rotated, optional off-site copy. **A backup on the same disk is not a backup** – set the rsync target (or copy to object storage).
**Restore drill (do it once a month):**
```
ADMIN_URL=postgres://admin@host/postgres ./scripts/restore.sh backups/chimppos_X.dump chimp_pos_restored
```
It restores into a *new* database, runs `ANALYZE`, and prints row counts. Point a staging instance at it to check the app boots.
Managed PostgreSQL with point-in-time recovery is a good complement, not a replacement for the drill.

## 6. Monitoring
* **Liveness/DB:** `GET /health` (use for the load balancer / uptime monitor).
* **Logs:** JSON lines with request id, status and latency; errors are `ERROR`-level. Grep a customer's `x-request-id`.
* **Alert on:** health failing, 5xx rate, the log line `refresh token reuse detected` (possible stolen token), `periodic ANALYZE failed`, disk > 80 %, last backup older than 26 h.
* **Offline queue health:** `SELECT status, count(*) FROM sync_operations GROUP BY 1;` – a growing `rejected` count means managers are not resolving conflicts.

## 7. Performance notes (measured)
With 2 000 products / 600–1 000 sales: list/search/report endpoints 4–150 ms, checkout p95 ≈ 120 ms under 8-way concurrency (release build), full 2 000-variant catalogue sync in 2 pages ≈ 0.05 s.
**Planner statistics:** PostgreSQL has none after a restore and stale ones after a bulk import. We measured `/inventory/stock` at 5–7 s and `/reports/sales/by-category` hitting the 30 s timeout until `ANALYZE` ran (then 9–14 ms). The server now runs `ANALYZE` at start and every 5 min, `migrate` and `restore.sh` run it too. After a **manual** bulk import run `psql -c ANALYZE`. Keep autovacuum on.

## 8. Security checklist
* HTTPS only (proxy); HSTS is sent in production. Port 3000 must not be reachable from the internet.
* Rotate `JWT_SECRET` if it leaks (everyone signs in again). Keep `.env` files out of git (`.gitignore` already excludes `.env`).
* Refresh tokens are stored hashed; reuse of a rotated token revokes the whole session. Passwords: Argon2id.
* Lockout after 5 bad passwords; per-IP rate limit on public auth endpoints (needs `TRUST_PROXY=true` behind a proxy, otherwise all clients share the proxy's IP).
* Every successful write is recorded in the append-only `audit_logs` table (`GET /audit-logs`).
* Run `cargo audit` in CI and update dependencies regularly.

## 9. Runbooks
| Situation | Action |
|---|---|
| Owner forgot password / locked out, no manager available | `chimpPOS_backend reset-password <company_id> <username>` (type the new password on stdin) – clears lockout, ends all sessions |
| `VersionMissing(…)` at migrate | a migration that the database recorded is missing from this build – restore the missing file; never edit applied migrations |
| Reports/lists suddenly slow | `psql -c ANALYZE`; check autovacuum is on |
| Rejected offline operations piling up | managers use `/sync/operations` (see client guide §2.5) |
| Need to roll back an upgrade | stop the service, restore the previous binary; migrations are additive, but if a migration must be undone restore the pre-upgrade backup (take one before every upgrade) |
| Suspected token theft | `POST /auth/logout-all` for the user, reset their password via `/users/{id}/reset-password` |

## 10. Known limitations
* Password-reset **delivery** (email/SMS) is not implemented (development log delivery only).
* Rate limiting is per process; several instances each keep their own counters (the account lockout is shared via the database).
* Payment methods on customer/supplier payments: `cash, card, mobile_money, other` (no dedicated bank transfer).
* Offline sales conflict handling is manual by design (see client guide); there is no automatic stock-borrowing.
* `docs/openapi.yaml` predates the sync, expenses, session and password endpoints and has not been regenerated.
