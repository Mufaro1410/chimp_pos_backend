#!/usr/bin/env python3
"""Offline-sync end-to-end test (stdlib only). Usage: python3 scripts/offline_sync_test.py [BASE_URL]"""
import json, sys, uuid, time, urllib.request, urllib.error

BASE = (sys.argv[1] if len(sys.argv) > 1 else "http://127.0.0.1:3000").rstrip("/")
API = BASE + "/api/v1"
PW = "CorrectHorseBattery!42"
SUFFIX = uuid.uuid4().hex[:6]
failures = []


def call(method, path, body=None, token=None, expect=None, label=None):
    req = urllib.request.Request(API + path, data=json.dumps(body).encode() if body is not None else None, method=method)
    req.add_header("content-type", "application/json")
    if token:
        req.add_header("authorization", f"Bearer {token}")
    try:
        with urllib.request.urlopen(req, timeout=30) as r:
            status, raw = r.status, r.read()
    except urllib.error.HTTPError as e:
        status, raw = e.code, e.read()
    try:
        js = json.loads(raw) if raw else None
    except Exception:
        js = raw.decode(errors="replace")
    exp = expect if isinstance(expect, (list, tuple)) else [expect]
    ok = expect is None or status in exp
    print(f"  [{'ok' if ok else 'FAIL'}] {status} {label or method + ' ' + path}")
    if not ok:
        failures.append(label or path)
        print("        ->", json.dumps(js)[:400])
    return status, js


def check(cond, label):
    print(f"  [{'ok' if cond else 'FAIL'}] {label}")
    if not cond:
        failures.append(label)


def op(op_type, payload, op_id=None):
    return {"client_op_id": op_id or str(uuid.uuid4()), "op_type": op_type, "payload": payload,
            "client_created_at": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())}


def stock_of(token, loc, variant):
    s, js = call("GET", f"/inventory/stock?location_id={loc}&variant_id={variant}", token=token, expect=200, label="read stock")
    rows = js if isinstance(js, list) else js["items"]
    return float(rows[0]["quantity_on_hand"])


print("== Setup")
s, reg = call("POST", "/auth/register", {"company": {"name": f"Offline Shop {SUFFIX}", "default_currency": "USD"},
    "owner": {"name": "Owner", "username": "owner", "email": f"o{SUFFIX}@example.com", "password": PW}}, expect=201)
cid = reg["company_id"]
s, lg = call("POST", "/auth/login", {"company_id": cid, "username": "owner", "password": PW}, expect=200)
T = lg["access_token"]
unit = call("GET", "/units", token=T, expect=200)[1][0]["id"]
loc = call("GET", "/locations", token=T, expect=200)[1][0]["id"]
s, p = call("POST", "/products", {"name": "Bread", "variant": {"name": "Loaf", "sku": f"B-{SUFFIX}", "unit_id": unit, "selling_price": "2.00", "cost_price": "1.00"}}, token=T, expect=201)
v = call("GET", f"/products/{p['id']}/variants", token=T, expect=200)[1]
v = (v if isinstance(v, list) else v["items"])[0]["id"]
call("POST", "/inventory/adjustments", {"location_id": loc, "variant_id": v, "movement_type": "adjustment_in", "quantity_delta": "10", "idempotency_key": str(uuid.uuid4())}, token=T, expect=201)
call("POST", "/categories", {"name": "Bakery"}, token=T, expect=201)
s, reg_ = call("POST", "/cash-registers", {"location_id": loc, "code": "R1", "name": "Till"}, token=T, expect=201)
s, sess = call("POST", f"/cash-registers/{reg_['id']}/sessions", {"opening_float": "50"}, token=T, expect=201)
s, cust = call("POST", "/customers", {"name": "Credit Carl"}, token=T, expect=201)
s, cust = call("PATCH", f"/customers/{cust['id']}/credit-policy", {"expected_version": cust["version"], "credit_enabled": True, "credit_limit": "100", "payment_terms_days": 7}, token=T, expect=200)
s, csale = call("POST", "/sales", {"location_id": loc, "customer_id": cust["id"], "items": [{"variant_id": v, "quantity": "2", "unit_price": "2.00"}]}, token=T, expect=201)
call("POST", f"/sales/{csale['id']}/checkout", {"expected_version": csale["version"], "idempotency_key": str(uuid.uuid4()), "register_id": reg_["id"], "payments": [{"payment_method": "cash", "amount": "1.00", "tendered_amount": "1.00"}]}, token=T, expect=200, label="online credit sale (owes 3.00)")
# stock now 8

print("\n== Bootstrap & pull")
s, b = call("GET", "/sync/bootstrap", token=T, expect=200)
check("sync.use" in b["permissions"] and b["company"]["default_currency"] == "USD", "bootstrap carries permissions and company")
for ent in b["entities"]:
    s, r = call("GET", f"/sync/pull/{ent}", token=T, expect=200, label=f"pull {ent} (full snapshot)")
    check(len(r["items"]) >= 1 and r["has_more"] is False, f"{ent}: snapshot has rows")
s, r = call("GET", "/sync/pull/variants", token=T, expect=200)
price0 = [i for i in r["items"] if i["id"] == v][0]["selling_price"]
cursor = r["next_cursor"]
# paging with limit=1 over products+variants
s, pg = call("GET", "/sync/pull/locations?limit=1", token=T, expect=200)
check(pg["has_more"] is False and len(pg["items"]) == 1, "single-row page, nothing more")
call("PUT", f"/products/{p['id']}/variants/{v}", {"expected_version": 1, "name": "Loaf", "sku": f"B-{SUFFIX}", "unit_id": unit, "selling_price": "2.50", "cost_price": "1.00", "is_active": True}, token=T, expect=[200, 400, 409], label="change price (online)")
s, r2 = call("GET", f"/sync/pull/variants?cursor={cursor}", token=T, expect=200, label="delta pull after price change")
changed = [i for i in r2["items"] if i["id"] == v]
print("        price before:", price0, "-> delta:", changed[0]["selling_price"] if changed else None)
check(bool(changed) and changed[0]["selling_price"] != price0, "delta contains the changed variant")
call("GET", "/sync/pull/variants?cursor=garbage", token=T, expect=400, label="bad cursor rejected")
call("GET", "/sync/pull/nonsense", token=T, expect=404, label="unknown entity rejected")
call("GET", "/sync/pull/variants?limit=5000", token=T, expect=400, label="limit too large rejected")
s, st = call("GET", "/sync/pull/stock", token=T, expect=200)
check(any(float(i["quantity_on_hand"]) == 8.0 for i in st["items"]), "stock feed shows 8 on hand")

print("\n== Push: offline sale, expense, adjustment, customer payment, till movement")
sale_id, exp_id, adj_id, pay_id, mov_id = (str(uuid.uuid4()) for _ in range(5))
batch = {"device_id": "tablet-1", "operations": [
    op("sale.checkout", {"location_id": loc, "register_id": reg_["id"], "items": [{"variant_id": v, "quantity": "3", "unit_price": "2.50"}],
        "payments": [{"payment_method": "cash", "amount": "7.50", "tendered_amount": "10.00"}]}, sale_id),
    op("expense.create", {"category": "Transport", "description": "Delivery fuel", "amount": "5.00", "payment_method": "cash", "cash_session_id": sess["id"]}, exp_id),
    op("stock.adjust", {"location_id": loc, "variant_id": v, "movement_type": "adjustment_out", "quantity_delta": "-1", "notes": "spoiled"}, adj_id),
    op("customer.payment", {"customer_id": cust["id"], "amount": "3.00", "payment_method": "cash", "cash_session_id": sess["id"],
        "allocations": [{"document_id": csale["id"], "amount": "3.00"}]}, pay_id),
    op("cash.movement", {"session_id": sess["id"], "movement_type": "cash_in", "amount": "20.00", "reference": "float top-up"}, mov_id),
]}
s, res = call("POST", "/sync/push", batch, token=T, expect=200, label="push 5 operations")
for r in res["results"]:
    check(r["status"] == "applied", f"{r['op_type']} applied" + ("" if r["status"] == "applied" else f" -> {r}"))
sale_res = res["results"][0]["result"]
print("        offline sale ->", sale_res)
after = stock_of(T, loc, v)
check(after == 4.0, f"stock = 8 - 3 sold - 1 spoiled = 4 (got {after})")

print("\n== Push: idempotency")
s, res2 = call("POST", "/sync/push", batch, token=T, expect=200, label="resend same batch")
check(all(r["status"] == "duplicate" for r in res2["results"]), "every operation reported as duplicate")
check(res2["results"][0]["result"]["sale_id"] == sale_res["sale_id"], "duplicate returns the original sale id")
check(stock_of(T, loc, v) == 4.0, "stock unchanged by the resend")
tampered = {"operations": [op("expense.create", {"category": "Transport", "description": "Delivery fuel", "amount": "99.00", "payment_method": "cash", "cash_session_id": sess["id"]}, exp_id)]}
s, rt = call("POST", "/sync/push", tampered, token=T, expect=200)
check(rt["results"][0]["error"]["code"] == "IDEMPOTENCY_CONFLICT", "same op id with different payload is refused")
dupid = str(uuid.uuid4())
call("POST", "/sync/push", {"operations": [op("stock.adjust", {}, dupid), op("stock.adjust", {}, dupid)]}, token=T, expect=400, label="duplicate op id in one request rejected")
call("POST", "/sync/push", {"operations": []}, token=T, expect=400, label="empty batch rejected")
call("POST", "/sync/push", {"operations": [op("stock.adjust", {}) for _ in range(101)]}, token=T, expect=400, label="101 operations rejected")

print("\n== Push: conflicts go to the review queue")
over_id = str(uuid.uuid4())
s, rj = call("POST", "/sync/push", {"operations": [
    op("sale.checkout", {"location_id": loc, "register_id": reg_["id"], "items": [{"variant_id": v, "quantity": "6", "unit_price": "2.50"}],
        "payments": [{"payment_method": "cash", "amount": "15.00", "tendered_amount": "15.00"}]}, over_id),
    op("nonsense.type", {}),
    op("stock.adjust", {"location_id": loc, "variant_id": v}),
]}, token=T, expect=200, label="push oversell + bad ops")
r0, r1, r2_ = rj["results"]
check(r0["status"] == "rejected" and r0["error"]["code"] == "INSUFFICIENT_STOCK", f"oversell rejected as INSUFFICIENT_STOCK ({r0.get('error')})")
check(r1["error"]["code"] == "UNKNOWN_OP_TYPE", "unknown op type rejected")
check(r2_["error"]["code"] == "INVALID_PAYLOAD", "malformed payload rejected")
check(stock_of(T, loc, v) == 4.0, "rejected sale did not touch stock")
s, drafts = call("GET", "/sales?status=draft&limit=50", token=T, expect=200, label="no orphan draft sales")
dl = drafts if isinstance(drafts, list) else drafts.get("items", [])
check(len(dl) == 0, f"no orphan drafts left behind ({len(dl)})")
s, ops = call("GET", "/sync/operations?status=rejected", token=T, expect=200)
rej = [o for o in ops if o["client_op_id"] == over_id]
check(len(rej) == 1 and rej[0]["status"] == "rejected", "rejected sale is in the review queue")
s, rr = call("POST", f"/sync/operations/{rej[0]['id']}/retry", token=T, expect=200, label="retry before fixing stock")
check(rr["status"] == "rejected", "still rejected while stock is short")
call("POST", "/inventory/adjustments", {"location_id": loc, "variant_id": v, "movement_type": "adjustment_in", "quantity_delta": "10", "notes": "recount", "idempotency_key": str(uuid.uuid4())}, token=T, expect=201)
s, rr = call("POST", f"/sync/operations/{rej[0]['id']}/retry", token=T, expect=200, label="retry after fixing stock")
check(rr["status"] == "applied", f"retry applied ({rr})")
check(stock_of(T, loc, v) == 8.0, "stock = 4 + 10 - 6 = 8")
call("POST", f"/sync/operations/{rej[0]['id']}/retry", token=T, expect=409, label="applied operation cannot be retried")
s, ops = call("GET", "/sync/operations?status=rejected", token=T, expect=200)
unk = [o for o in ops if o["op_type"] == "stock.adjust"]
# bad payload ops were not journalled (rejected before insert) except malformed-but-known type
if unk:
    call("POST", f"/sync/operations/{unk[0]['id']}/dismiss", {"note": "garbage from test"}, token=T, expect=204, label="dismiss a rejected operation")

print("\n== Cashier permissions in sync")
s, roles = call("GET", "/roles", token=T, expect=200)
rl = roles if isinstance(roles, list) else roles["items"]
cr = [r for r in rl if r["role_key"] == "cashier"][0]
call("POST", "/users", {"name": "Cathy", "username": "cathy", "initial_password": PW, "role_ids": [cr["id"]]}, token=T, expect=201)
cl = call("POST", "/auth/login", {"company_id": cid, "username": "cathy", "password": PW}, expect=200)[1]
CT, CATHY = cl["access_token"], cl["user"]["id"]
call("GET", "/sync/bootstrap", token=CT, expect=200, label="cashier bootstrap")
call("GET", "/sync/pull/products", token=CT, expect=200, label="cashier pulls products")
cs_id = str(uuid.uuid4())
s, cp = call("POST", "/sync/push", {"device_id": "till-2", "operations": [
    op("sale.checkout", {"location_id": loc, "register_id": reg_["id"], "items": [{"variant_id": v, "quantity": "1", "unit_price": "2.50"}],
        "payments": [{"payment_method": "cash", "amount": "2.50", "tendered_amount": "5.00"}]}, cs_id),
    op("expense.create", {"category": "Misc", "description": "not allowed", "amount": "1.00", "payment_method": "other"}),
]}, token=CT, expect=200, label="cashier push")
check(cp["results"][0]["status"] == "applied", "cashier's offline sale applied")
check(cp["results"][1]["status"] == "rejected" and cp["results"][1]["error"]["code"] == "PERMISSION_DENIED", "cashier cannot push expenses")
s, mine = call("GET", "/sync/operations", token=CT, expect=200, label="cashier lists own operations")
check(all(o["user_id"] == CATHY for o in mine) and len(mine) == 2, f"cashier sees only own operations ({len(mine)})")
call("POST", "/sync/operations/1/retry", token=CT, expect=403, label="cashier cannot retry operations")
call("GET", "/sync/operations", expect=401, label="unauthenticated sync -> 401")

print("\n== Price changed while the cashier was offline")
price_id = str(uuid.uuid4())
s, pc = call("POST", "/sync/push", {"device_id": "till-2", "operations": [
    op("sale.checkout", {"location_id": loc, "register_id": reg_["id"], "items": [{"variant_id": v, "quantity": "1", "unit_price": "2.00"}],
        "payments": [{"payment_method": "cash", "amount": "2.00", "tendered_amount": "2.00"}]}, price_id)]}, token=CT, expect=200, label="cashier syncs a sale made at the old, lower price")
check(pc["results"][0]["error"]["code"] == "DISCOUNT_PERMISSION_REQUIRED", "rejected: below current list price needs sales.discount")
s, rq = call("GET", "/sync/operations?status=rejected", token=T, expect=200)
pid = [o for o in rq if o["client_op_id"] == price_id][0]["id"]
s, r1 = call("POST", f"/sync/operations/{pid}/retry", token=T, expect=200, label="plain retry (runs as the cashier)")
check(r1["status"] == "rejected", "plain retry cannot succeed: the cashier still lacks the permission")
call("POST", f"/sync/operations/{pid}/retry", {"run_as_reviewer": "yes"}, token=T, expect=400, label="malformed retry body rejected")
before = stock_of(T, loc, v)
s, r2 = call("POST", f"/sync/operations/{pid}/retry", {"run_as_reviewer": True}, token=T, expect=200, label="manager approves and runs it under their authority")
check(r2["status"] == "applied" and r2["result"]["total_amount"] == "2.0000", f"applied at the sold price ({r2.get('result')})")
check(stock_of(T, loc, v) == before - 1, "stock reduced by the approved sale")
s, ap = call("GET", "/sync/operations?status=applied", token=T, expect=200)
note = [o for o in ap if o["client_op_id"] == price_id][0]["resolution_note"]
check(note is not None and "Approved and executed by reviewer" in note, "journal records who approved it")
call("POST", f"/sync/operations/{pid}/retry", {"run_as_reviewer": True}, token=CT, expect=403, label="a cashier cannot approve")

print("\n== Tenant isolation")
s, reg2 = call("POST", "/auth/register", {"company": {"name": f"Other {SUFFIX}", "default_currency": "USD"},
    "owner": {"name": "O2", "username": "owner", "email": f"x{SUFFIX}@example.com", "password": PW}}, expect=201)
OT = call("POST", "/auth/login", {"company_id": reg2["company_id"], "username": "owner", "password": PW}, expect=200)[1]["access_token"]
s, orr = call("GET", "/sync/pull/variants", token=OT, expect=200)
check(len(orr["items"]) == 0, "other company's feed is empty")
s, oo = call("GET", "/sync/operations", token=OT, expect=200)
check(len(oo) == 0, "other company sees no operations")
same = op("stock.adjust", {"location_id": loc, "variant_id": v, "movement_type": "adjustment_in", "quantity_delta": "5"}, sale_id)
s, x = call("POST", "/sync/push", {"operations": [same]}, token=OT, expect=200)
check(x["results"][0]["status"] != "duplicate", "same client_op_id in another company is independent")

print("\n" + "=" * 60)
if failures:
    print(f"{len(failures)} FAILED:"); [print("  -", f) for f in failures]; sys.exit(1)
print("ALL OFFLINE CHECKS PASSED")
