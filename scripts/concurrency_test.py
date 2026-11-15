#!/usr/bin/env python3
"""Concurrency / race-condition test (stdlib only). Usage: python3 scripts/concurrency_test.py [BASE_URL]"""
import json, sys, uuid, threading, urllib.request, urllib.error

BASE = (sys.argv[1] if len(sys.argv) > 1 else "http://127.0.0.1:3000").rstrip("/")
API = BASE + "/api/v1"
PW = "CorrectHorseBattery!42"
SUFFIX = uuid.uuid4().hex[:6]
failures = []


def call(method, path, body=None, token=None):
    req = urllib.request.Request(API + path, data=json.dumps(body).encode() if body is not None else None, method=method)
    req.add_header("content-type", "application/json")
    if token:
        req.add_header("authorization", f"Bearer {token}")
    try:
        with urllib.request.urlopen(req, timeout=60) as r:
            raw, st = r.read(), r.status
    except urllib.error.HTTPError as e:
        raw, st = e.read(), e.code
    try:
        return st, (json.loads(raw) if raw else None)
    except Exception:
        return st, None


def race(n, fn):
    """Run fn(i) in n threads released at the same instant; return the results in order."""
    barrier, out = threading.Barrier(n), [None] * n

    def worker(i):
        barrier.wait()
        out[i] = fn(i)

    ts = [threading.Thread(target=worker, args=(i,)) for i in range(n)]
    [t.start() for t in ts]
    [t.join() for t in ts]
    return out


def check(cond, label):
    print(f"  [{'ok' if cond else 'FAIL'}] {label}")
    if not cond:
        failures.append(label)


def stock(token, loc, v):
    st, js = call("GET", f"/inventory/stock?location_id={loc}&variant_id={v}", token=token)
    rows = js if isinstance(js, list) else js["items"]
    return float(rows[0]["quantity_on_hand"])


st, reg = call("POST", "/auth/register", {"company": {"name": f"Race Shop {SUFFIX}", "default_currency": "USD"},
    "owner": {"name": "Owner", "username": "owner", "email": f"o{SUFFIX}@example.com", "password": PW}})
T = call("POST", "/auth/login", {"company_id": reg["company_id"], "username": "owner", "password": PW})[1]["access_token"]
unit = call("GET", "/units", token=T)[1][0]["id"]
loc = call("GET", "/locations", token=T)[1][0]["id"]
p = call("POST", "/products", {"name": "Last Unit", "variant": {"name": "x", "sku": f"L-{SUFFIX}", "unit_id": unit, "selling_price": "5.00", "cost_price": "2.00"}}, token=T)[1]
v = call("GET", f"/products/{p['id']}/variants", token=T)[1]
v = (v if isinstance(v, list) else v["items"])[0]["id"]
rg = call("POST", "/cash-registers", {"location_id": loc, "code": "R1", "name": "Till"}, token=T)[1]
sess = call("POST", f"/cash-registers/{rg['id']}/sessions", {"opening_float": "100"}, token=T)[1]


def adjust(q):
    return call("POST", "/inventory/adjustments", {"location_id": loc, "variant_id": v, "movement_type": "adjustment_in", "quantity_delta": str(q), "idempotency_key": str(uuid.uuid4())}, token=T)


def draft(qty=1):
    st, s = call("POST", "/sales", {"location_id": loc, "items": [{"variant_id": v, "quantity": str(qty), "unit_price": "5.00"}]}, token=T)
    return s


def checkout(s, key=None):
    return call("POST", f"/sales/{s['id']}/checkout", {"expected_version": s["version"], "idempotency_key": key or str(uuid.uuid4()),
        "register_id": rg["id"], "payments": [{"payment_method": "cash", "amount": "5.00", "tendered_amount": "5.00"}]}, token=T)[0]


print("== 1. Eight cashiers sell the LAST unit at the same instant")
adjust(1)
drafts = [draft() for _ in range(8)]
codes = race(8, lambda i: checkout(drafts[i]))
print("        statuses:", sorted(codes))
check(codes.count(200) == 1, f"exactly one sale succeeded ({codes.count(200)})")
check(all(c in (200, 409) for c in codes), "all others refused cleanly (409), no 500s")
check(stock(T, loc, v) == 0.0, "stock is exactly 0, never negative")

print("\n== 2. The same checkout (same idempotency key) fired 8 times at once")
adjust(5)
d = draft()
key = str(uuid.uuid4())
codes = race(8, lambda i: checkout(d, key))
print("        statuses:", sorted(codes))
check(all(c == 200 for c in codes), "every retry gets the same successful answer")
check(stock(T, loc, v) == 4.0, f"stock reduced exactly once (5 -> 4, got {stock(T, loc, v)})")

print("\n== 3. The same offline operation pushed from 8 devices at once")
op_id = str(uuid.uuid4())
payload = {"location_id": loc, "register_id": rg["id"], "items": [{"variant_id": v, "quantity": "1", "unit_price": "5.00"}],
           "payments": [{"payment_method": "cash", "amount": "5.00", "tendered_amount": "5.00"}]}
res = race(8, lambda i: call("POST", "/sync/push", {"device_id": f"dev-{i}", "operations": [{"client_op_id": op_id, "op_type": "sale.checkout", "payload": payload}]}, token=T))
statuses = sorted(r[1]["results"][0]["status"] for r in res)
print("        outcomes:", statuses)
check(statuses.count("applied") == 1, f"exactly one device applied it ({statuses.count('applied')})")
check(all(s in ("applied", "duplicate", "in_progress") for s in statuses), "the rest saw duplicate / in_progress, never an error")
check(stock(T, loc, v) == 3.0, f"sold once (4 -> 3, got {stock(T, loc, v)})")
sales = call("GET", "/sales?limit=100", token=T)[1]
sl = sales if isinstance(sales, list) else sales["items"]
check(len([s for s in sl if s["status"] == "completed"]) == 3, f"exactly 3 completed sales in total ({len([s for s in sl if s['status'] == 'completed'])})")

print("\n== 4. Eight expenses of 30.00 against a till holding 100 + 5 + 5 + 5 cash")
till = 100 + 5 + 5 + 5
codes = race(8, lambda i: call("POST", "/expenses", {"idempotency_key": str(uuid.uuid4()), "category": "Race", "description": f"e{i}", "amount": "30.00",
        "payment_method": "cash", "cash_session_id": sess["id"]}, token=T)[0])
print("        statuses:", sorted(codes))
ok = codes.count(201)
check(ok == 3 and codes.count(409) == 5, f"exactly 3 fit in the till (3 x 30 = 90 <= {till}), 5 refused ({ok} accepted)")
summ = call("GET", f"/cash-sessions/{sess['id']}/summary", token=T)[1]
check(float(summ["calculated_expected_cash"]) == till - 90, f"till is {till - 90}.00, never overdrawn (got {summ['calculated_expected_cash']})")

print("\n== 5. Five tills try to open a session on the same register")
rg2 = call("POST", "/cash-registers", {"location_id": loc, "code": "R2", "name": "Till 2"}, token=T)[1]
codes = race(5, lambda i: call("POST", f"/cash-registers/{rg2['id']}/sessions", {"opening_float": "10"}, token=T)[0])
print("        statuses:", sorted(codes))
check(codes.count(201) == 1 and all(c in (201, 409) for c in codes), "exactly one session opened")

print("\n== 6. Concurrent stock adjustments never lose an update")
before = stock(T, loc, v)
codes = race(10, lambda i: adjust(1)[0])
check(all(c == 201 for c in codes), "all adjustments accepted")
check(stock(T, loc, v) == before + 10, f"stock rose by exactly 10 ({before} -> {stock(T, loc, v)})")

print("\n" + "=" * 60)
if failures:
    print(f"{len(failures)} FAILED:"); [print("  -", f) for f in failures]; sys.exit(1)
print("ALL CONCURRENCY CHECKS PASSED")
