#!/usr/bin/env python3
"""
Authentication / session / password end-to-end test (stdlib only).

Start the server with:  REFRESH_ROTATION_GRACE_SECONDS=2 PASSWORD_RESET_DELIVERY=log \
                        AUTH_RATE_LIMIT_PER_MINUTE=1000 cargo run
Usage: python3 scripts/auth_test.py [BASE_URL] [SERVER_LOG_PATH]
"""
import json, re, sys, time, uuid, urllib.request, urllib.error

BASE = (sys.argv[1] if len(sys.argv) > 1 else "http://127.0.0.1:3000").rstrip("/")
LOG = sys.argv[2] if len(sys.argv) > 2 else "/tmp/server.log"
API = BASE + "/api/v1"
PW = "CorrectHorseBattery!42"
PW2 = "Another-Strong-Passphrase-77"
SUFFIX = uuid.uuid4().hex[:6]
failures = []


def call(method, path, body=None, token=None, expect=None, label=None, headers=None, raw=False):
    req = urllib.request.Request(API + path, data=json.dumps(body).encode() if body is not None else None, method=method)
    req.add_header("content-type", "application/json")
    if token:
        req.add_header("authorization", f"Bearer {token}")
    for k, v in (headers or {}).items():
        req.add_header(k, v)
    try:
        with urllib.request.urlopen(req, timeout=30) as r:
            status, data, hdrs = r.status, r.read(), r.headers
    except urllib.error.HTTPError as e:
        status, data, hdrs = e.code, e.read(), e.headers
    try:
        js = json.loads(data) if data else None
    except Exception:
        js = data.decode(errors="replace")
    exp = expect if isinstance(expect, (list, tuple)) else [expect]
    ok = expect is None or status in exp
    print(f"  [{'ok' if ok else 'FAIL'}] {status} {label or method + ' ' + path}")
    if not ok:
        failures.append(label or path)
        print("        ->", json.dumps(js)[:300])
    return (status, js, hdrs) if raw else (status, js)


def check(cond, label):
    print(f"  [{'ok' if cond else 'FAIL'}] {label}")
    if not cond:
        failures.append(label)


def code(js):
    return (js or {}).get("error", {}).get("code") if isinstance(js, dict) else None


print("== Login, /me, refresh rotation")
s, reg = call("POST", "/auth/register", {"company": {"name": f"Auth Shop {SUFFIX}", "default_currency": "USD"},
    "owner": {"name": "Owner", "username": "owner", "email": f"o{SUFFIX}@example.com", "password": PW}}, expect=201)
cid = reg["company_id"]
s, lg = call("POST", "/auth/login", {"company_id": cid, "username": "owner", "password": PW, "device_label": "laptop"}, expect=200)
A1, R1 = lg["access_token"], lg["refresh_token"]
check(len(R1) == 64 and lg["must_change_password"] is False and lg["refresh_expires_in"] > 86400, "login returns refresh token")
s, me = call("GET", "/auth/me", token=A1, expect=200)
check("sales.checkout" in me["permissions"] and "owner" in me["roles"] and me["username"] == "owner", "/me lists roles and permissions")
s, t2 = call("POST", "/auth/refresh", {"refresh_token": R1}, expect=200, label="refresh (rotation)")
A2, R2 = t2["access_token"], t2["refresh_token"]
check(R2 != R1 and A2 != A1, "refresh returns a new pair")
call("GET", "/auth/me", token=A2, expect=200, label="new access token works")
s, t2b = call("POST", "/auth/refresh", {"refresh_token": R1}, expect=200, label="old token inside grace window still works (lost-response retry)")
time.sleep(3.2)
s, js = call("POST", "/auth/refresh", {"refresh_token": R1}, expect=401, label="old token after grace = reuse detected")
check(code(js) == "INVALID_REFRESH_TOKEN", "reuse reports INVALID_REFRESH_TOKEN")
call("POST", "/auth/refresh", {"refresh_token": R2}, expect=401, label="reuse revoked the whole session family")
call("POST", "/auth/refresh", {"refresh_token": t2b["refresh_token"]}, expect=401, label="including the sibling token")
call("POST", "/auth/refresh", {"refresh_token": "x" * 64}, expect=401, label="unknown refresh token")
call("POST", "/auth/refresh", {"refresh_token": ""}, expect=401, label="empty refresh token")

print("\n== Logout")
s, lg = call("POST", "/auth/login", {"company_id": cid, "username": "owner", "password": PW}, expect=200)
A, R = lg["access_token"], lg["refresh_token"]
call("POST", "/auth/logout", {"refresh_token": R}, expect=204)
call("POST", "/auth/refresh", {"refresh_token": R}, expect=401, label="refresh after logout fails")
call("POST", "/auth/logout", {"refresh_token": "nonsense"}, expect=204, label="logout with unknown token is a quiet no-op")
l1 = call("POST", "/auth/login", {"company_id": cid, "username": "owner", "password": PW}, expect=200)[1]
l2 = call("POST", "/auth/login", {"company_id": cid, "username": "owner", "password": PW}, expect=200)[1]
call("POST", "/auth/logout-all", token=l1["access_token"], expect=204)
call("POST", "/auth/refresh", {"refresh_token": l1["refresh_token"]}, expect=401, label="logout-all ends device 1")
call("POST", "/auth/refresh", {"refresh_token": l2["refresh_token"]}, expect=401, label="logout-all ends device 2")
call("GET", "/auth/me", token=l2["access_token"], expect=401, label="logout-all also kills outstanding access tokens immediately")

print("\n== Error shape and enumeration resistance")
t0 = time.time(); s1, e1 = call("POST", "/auth/login", {"company_id": cid, "username": "owner", "password": "wrong-password-123"}, expect=401, label="wrong password"); d1 = time.time() - t0
t0 = time.time(); s2, e2 = call("POST", "/auth/login", {"company_id": cid, "username": "nobody", "password": "wrong-password-123"}, expect=401, label="unknown user"); d2 = time.time() - t0
check(e1 == e2 and code(e1) == "INVALID_CREDENTIALS", "identical JSON error for wrong password and unknown user")
print(f"        timing: known user {d1*1000:.0f} ms, unknown user {d2*1000:.0f} ms")
check(d2 > d1 * 0.5, "unknown-user login spends comparable time (no timing oracle)")

print("\n== Change password")
l = call("POST", "/auth/login", {"company_id": cid, "username": "owner", "password": PW}, expect=200)[1]
other = call("POST", "/auth/login", {"company_id": cid, "username": "owner", "password": PW}, expect=200)[1]
A, R = l["access_token"], l["refresh_token"]
call("POST", "/auth/change-password", {"current_password": "not-my-password-123", "new_password": PW2}, token=A, expect=400, label="wrong current password")
call("POST", "/auth/change-password", {"current_password": PW, "new_password": "short"}, token=A, expect=400, label="weak new password")
call("POST", "/auth/change-password", {"current_password": PW, "new_password": PW}, token=A, expect=400, label="same password refused")
s, ch = call("POST", "/auth/change-password", {"current_password": PW, "new_password": PW2}, token=A, expect=200)
check("refresh_token" in ch, "change-password returns a fresh session for this device")
call("GET", "/auth/me", token=A, expect=401, label="pre-change access token is invalidated")
call("GET", "/auth/me", token=ch["access_token"], expect=200, label="new access token works")
call("POST", "/auth/refresh", {"refresh_token": other["refresh_token"]}, expect=401, label="other devices are signed out")
call("POST", "/auth/login", {"company_id": cid, "username": "owner", "password": PW}, expect=401, label="old password no longer works")
call("POST", "/auth/login", {"company_id": cid, "username": "owner", "password": PW2}, expect=200, label="new password works")
OWNER = ch["access_token"]

print("\n== Account lockout")
roles = call("GET", "/roles", token=OWNER, expect=200)[1]
rl = roles if isinstance(roles, list) else roles["items"]
rid = {r["role_key"]: r["id"] for r in rl}
call("POST", "/users", {"name": "Locky", "username": "locky", "initial_password": PW, "role_ids": [rid["cashier"]]}, token=OWNER, expect=201)
for i in range(5):
    call("POST", "/auth/login", {"company_id": cid, "username": "locky", "password": "wrong-password-123"}, expect=401, label=f"failed attempt {i+1}")
s, js = call("POST", "/auth/login", {"company_id": cid, "username": "locky", "password": PW}, expect=429, label="locked: even the right password is refused")
check(code(js) == "ACCOUNT_LOCKED", "lockout reports ACCOUNT_LOCKED")
users = call("GET", "/users", token=OWNER, expect=200)[1]
ul = users if isinstance(users, list) else users["items"]
locky = [u for u in ul if u["username"] == "locky"][0]["id"]

print("\n== Administrator password reset")
call("POST", f"/users/{locky}/reset-password", {"new_password": PW2}, token=OWNER, expect=204, label="owner resets cashier password (clears lockout)")
s, lk = call("POST", "/auth/login", {"company_id": cid, "username": "locky", "password": PW2}, expect=200, label="cashier logs in with the temporary password")
check(lk["must_change_password"] is True, "must_change_password is set")
LA = lk["access_token"]
s, js = call("GET", "/products", token=LA, expect=403, label="blocked until the password is changed")
check(code(js) == "PASSWORD_CHANGE_REQUIRED", "blocked with PASSWORD_CHANGE_REQUIRED")
call("GET", "/auth/me", token=LA, expect=200, label="/me still allowed")
s, ch2 = call("POST", "/auth/change-password", {"current_password": PW2, "new_password": "My-Own-Passphrase-2026"}, token=LA, expect=200)
call("GET", "/products", token=ch2["access_token"], expect=200, label="normal access after changing it")
call("POST", f"/users/{locky}/reset-password", {"new_password": PW2}, token=ch2["access_token"], expect=403, label="cashier cannot reset passwords")
call("POST", f"/users/{uuid.uuid4()}/reset-password", {"new_password": PW2}, token=OWNER, expect=404, label="reset unknown user")
call("POST", f"/users/{locky}/reset-password", {"new_password": "short"}, token=OWNER, expect=400, label="weak temporary password refused")
me_id = call("GET", "/auth/me", token=OWNER, expect=200)[1]["user_id"]
call("POST", f"/users/{me_id}/reset-password", {"new_password": PW}, token=OWNER, expect=400, label="cannot admin-reset your own account")
call("POST", "/users", {"name": "Adam Admin", "username": "adam", "initial_password": PW, "role_ids": [rid["admin"]]}, token=OWNER, expect=201)
AD = call("POST", "/auth/login", {"company_id": cid, "username": "adam", "password": PW}, expect=200)[1]["access_token"]
s, js = call("POST", f"/users/{me_id}/reset-password", {"new_password": PW}, token=AD, expect=403, label="admin cannot reset the owner's password")
check(code(js) == "OWNER_PROTECTED", "reports OWNER_PROTECTED")

print("\n== Forgot / reset password (token delivered via server log in dev)")
def log_tokens():
    try:
        return re.findall(r"PASSWORD RESET TOKEN for '([^']+)'.*?: ([0-9a-f]{64})", open(LOG).read())
    except FileNotFoundError:
        return []
before = len(log_tokens())
call("POST", "/auth/forgot-password", {"company_id": cid, "identifier": "nobody"}, expect=202, label="forgot for unknown user still 202")
check(len(log_tokens()) == before, "no token issued for unknown user")
call("POST", "/auth/forgot-password", {"company_id": cid, "identifier": "ADAM"}, expect=202, label="forgot for known user (case-insensitive)")
time.sleep(0.3)
toks = [t for u, t in log_tokens() if u == "adam"]
check(len(toks) >= 1, "reset token was issued")
if toks:
    tok = toks[-1]
    call("POST", "/auth/reset-password", {"token": tok, "new_password": "short"}, expect=400, label="weak password refused (token not consumed)")
    call("POST", "/auth/reset-password", {"token": "0" * 64, "new_password": PW2}, expect=400, label="bad token refused")
    call("POST", "/auth/reset-password", {"token": tok, "new_password": PW2}, expect=204, label="reset with valid token")
    call("POST", "/auth/reset-password", {"token": tok, "new_password": PW2}, expect=400, label="token is single-use")
    call("POST", "/auth/login", {"company_id": cid, "username": "adam", "password": PW}, expect=401, label="old password dead")
    call("POST", "/auth/login", {"company_id": cid, "username": "adam", "password": PW2}, expect=200, label="new password works")
    call("GET", "/auth/me", token=AD, expect=401, label="sessions from before the reset are invalid")

print("\n== Headers, CORS, request id")
s, js, h = call("GET", "/auth/me", token=OWNER, expect=200, raw=True, label="response headers")
check(h.get("x-content-type-options") == "nosniff" and h.get("x-frame-options") == "DENY", "security headers present")
check(h.get("cache-control") == "no-store", "responses are not cacheable")
check(bool(h.get("x-request-id")), "x-request-id is returned")
s, js, h = call("GET", "/auth/me", token=OWNER, expect=200, raw=True, headers={"x-request-id": "abc-123"}, label="request id is propagated")
check(h.get("x-request-id") == "abc-123", "client-supplied request id echoed back")
req = urllib.request.Request(API + "/products", method="OPTIONS", headers={"Origin": "http://localhost:5173", "Access-Control-Request-Method": "GET", "Access-Control-Request-Headers": "authorization,content-type"})
with urllib.request.urlopen(req) as r:
    check(r.status in (200, 204) and bool(r.headers.get("access-control-allow-origin")), "CORS preflight allowed")

print("\n" + "=" * 60)
if failures:
    print(f"{len(failures)} FAILED:"); [print("  -", f) for f in failures]; sys.exit(1)
print("ALL AUTH CHECKS PASSED")
