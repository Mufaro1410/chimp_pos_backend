#!/usr/bin/env python3
"""
ChimpPOS end-to-end smoke test (stdlib only).

Usage:  python3 scripts/smoke_test.py [BASE_URL]      (default http://127.0.0.1:3000)
Registers a throw-away company and exercises every module. Exit code 1 on any failure.
"""
import json, sys, uuid, urllib.request, urllib.error, time

BASE = (sys.argv[1] if len(sys.argv) > 1 else "http://127.0.0.1:3000").rstrip("/")
API = BASE + "/api/v1"
failures = []
PW = "CorrectHorseBattery!42"
SUFFIX = uuid.uuid4().hex[:6]


def call(method, path, body=None, token=None, expect=None, label=None):
    url = path if path.startswith("http") else API + path
    data = json.dumps(body).encode() if body is not None else None
    req = urllib.request.Request(url, data=data, method=method)
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
    ok = True
    if expect is not None:
        exp = expect if isinstance(expect, (list, tuple)) else [expect]
        ok = status in exp
    name = label or f"{method} {path}"
    print(f"  [{'ok' if ok else 'FAIL'}] {status} {name}")
    if not ok:
        failures.append(name)
        print("        ->", json.dumps(js)[:400])
    return status, js


def D(x):  # decimal string helper
    return str(x)


def sec(t):
    print(f"\n== {t}")


# ------------------------------------------------------------------ auth
sec("Auth")
call("GET", BASE + "/health", expect=200)
s, reg = call("POST", "/auth/register", {
    "company": {"name": f"Smoke Shop {SUFFIX}", "default_currency": "USD"},
    "owner": {"name": "Owner", "username": "owner", "email": f"o{SUFFIX}@example.com", "password": PW},
}, expect=[200, 201])
cid = reg["company_id"]
s, lg = call("POST", "/auth/login", {"company_id": cid, "username": "owner", "password": PW}, expect=200)
T = lg["access_token"]
call("GET", "/auth/me", token=T, expect=200)
s, rf = call("POST", "/auth/refresh", token=T, expect=200)
call("GET", "/shop", token=T, expect=200)
call("GET", "/products", expect=401, label="unauthenticated -> 401")
call("POST", "/auth/login", {"company_id": cid, "username": "owner", "password": "wrong-password-123"}, expect=401, label="bad password -> 401")

# --------------------------------------------------------------- catalogue
sec("Catalogue lookups")
s, units = call("GET", "/units", token=T, expect=200)
unit_id = units[0]["id"]
s, locs = call("GET", "/locations", token=T, expect=200)
loc = locs[0]["id"]
assert locs[0]["is_default"], "default location missing"
s, cat = call("POST", "/categories", {"name": "Drinks"}, token=T, expect=201)
s, cat2 = call("POST", "/categories", {"name": "Soft Drinks", "parent_id": cat["id"]}, token=T, expect=201)
call("PUT", f"/categories/{cat['id']}", {"expected_version": cat["version"], "name": "Drinks", "description": None, "parent_id": cat2["id"], "is_active": True}, token=T, expect=400, label="category cycle rejected")
call("GET", "/categories", token=T, expect=200)
s, wh = call("POST", "/locations", {"name": "Warehouse", "code": "wh1", "location_type": "warehouse"}, token=T, expect=201)
call("PUT", f"/locations/{loc}", {"expected_version": locs[0]["version"], "name": "Main Store", "is_active": False}, token=T, expect=409, label="default location cannot be deactivated")

# ---------------------------------------------------------------- products
sec("Products & variants")
s, p1 = call("POST", "/products", {
    "name": "Coca-Cola", "category_id": cat2["id"],
    "variant": {"name": "500ml", "sku": f"CC500-{SUFFIX}", "unit_id": unit_id, "selling_price": "1.50", "cost_price": "1.00"},
}, token=T, expect=[200, 201])
pid = p1.get("id") or p1["product"]["id"]
s, vs = call("GET", f"/products/{pid}/variants", token=T, expect=200)
vlist = vs if isinstance(vs, list) else vs.get("items", vs)
v1 = vlist[0]["id"]
s, v2r = call("POST", f"/products/{pid}/variants", {"name": "2L", "sku": f"CC2L-{SUFFIX}", "unit_id": unit_id, "selling_price": "3.00", "cost_price": "2.00"}, token=T, expect=[200, 201])
v2 = v2r.get("id") or v2r["variant"]["id"]
call("GET", "/products?limit=10&offset=0&search=coca", token=T, expect=200)
call("GET", f"/products/{pid}", token=T, expect=200)

# ----------------------------------------------------------------- suppliers / purchasing
sec("Suppliers, purchasing, receiving")
s, sup = call("POST", "/suppliers", {"name": "Delta Beverages", "payment_terms_days": 30}, token=T, expect=[200, 201])
sid = sup["id"]
call("GET", "/suppliers", token=T, expect=200)
s, po = call("POST", "/purchases", {"supplier_id": sid, "items": [
    {"variant_id": v1, "ordered_quantity": "100", "unit_cost": "1.00"},
    {"variant_id": v2, "ordered_quantity": "20", "unit_cost": "2.00"}]}, token=T, expect=[200, 201])
poid = po["id"]
s, sub = call("POST", f"/purchases/{poid}/submit", {"expected_version": po["version"]}, token=T, expect=[200, 204])
s, pod = call("GET", f"/purchases/{poid}", token=T, expect=200)
items = pod["items"]
call("POST", f"/purchases/{poid}/receipts", {"location_id": loc, "idempotency_key": str(uuid.uuid4()), "items": [
    {"purchase_order_item_id": items[0]["id"], "quantity_received": "100"},
    {"purchase_order_item_id": items[1]["id"], "quantity_received": "20"}]}, token=T, expect=[200, 201], label="receive goods")
call("GET", f"/purchases/{poid}/receipts", token=T, expect=200)
s, stock = call("GET", f"/inventory/stock?location_id={loc}", token=T, expect=200)
rows = stock if isinstance(stock, list) else stock.get("items", [])
q1 = [r for r in rows if r["variant_id"] == v1][0]["quantity_on_hand"]
assert float(q1) == 100.0, f"stock after receipt should be 100, got {q1}"
adj_key = str(uuid.uuid4())
call("POST", "/inventory/adjustments", {"location_id": loc, "variant_id": v1, "movement_type": "adjustment_out", "quantity_delta": "-5", "notes": "broken", "idempotency_key": adj_key}, token=T, expect=[200, 201], label="stock adjustment")
call("POST", "/inventory/adjustments", {"location_id": loc, "variant_id": v1, "movement_type": "adjustment_out", "quantity_delta": "-5", "notes": "broken", "idempotency_key": adj_key}, token=T, expect=[200, 201], label="adjustment replay is idempotent")
call("GET", "/inventory/movements?limit=20", token=T, expect=200)

# ------------------------------------------------------------------ cash + sale
sec("Cash register & cash sale")
s, reg_ = call("POST", "/cash-registers", {"location_id": loc, "code": "R1", "name": "Main Counter"}, token=T, expect=[200, 201])
rid = reg_["id"]
call("GET", "/cash-registers", token=T, expect=200)
s, sess = call("POST", f"/cash-registers/{rid}/sessions", {"opening_float": "100.00"}, token=T, expect=[200, 201])
sess_id = sess["id"]
call("GET", f"/cash-registers/{rid}/sessions/open", token=T, expect=200)

s, sale = call("POST", "/sales", {"location_id": loc, "items": [{"variant_id": v1, "quantity": "2", "unit_price": "1.50"}]}, token=T, expect=[200, 201])
sale_id = sale["id"]
s, sale = call("PUT", f"/sales/{sale_id}", {"expected_version": sale["version"], "location_id": loc, "items": [
    {"variant_id": v1, "quantity": "3", "unit_price": "1.50"}, {"variant_id": v2, "quantity": "1", "unit_price": "3.00"}]}, token=T, expect=200, label="edit draft sale")
total = sale["total_amount"]
ck_key = str(uuid.uuid4())
body = {"expected_version": sale["version"], "idempotency_key": ck_key, "register_id": rid,
        "payments": [{"payment_method": "cash", "amount": D(total), "tendered_amount": "10.00"}]}
s, out = call("POST", f"/sales/{sale_id}/checkout", body, token=T, expect=200, label="checkout (cash)")
call("POST", f"/sales/{sale_id}/checkout", body, token=T, expect=200, label="checkout replay is idempotent")
call("GET", f"/sales/{sale_id}/receipt", token=T, expect=200)
s, stock = call("GET", f"/inventory/stock?location_id={loc}&variant_id={v1}", token=T, expect=200)
rows = stock if isinstance(stock, list) else stock.get("items", [])
assert float(rows[0]["quantity_on_hand"]) == 92.0, f"stock after sale should be 92, got {rows[0]['quantity_on_hand']}"
s, summ = call("GET", f"/cash-sessions/{sess_id}/summary", token=T, expect=200)
print("        session summary:", json.dumps(summ)[:300])
call("GET", "/sales?limit=10", token=T, expect=200)

# -------------------------------------------------- insufficient stock protection
s, big = call("POST", "/sales", {"location_id": loc, "items": [{"variant_id": v2, "quantity": "500", "unit_price": "3.00"}]}, token=T, expect=[200, 201])
call("POST", f"/sales/{big['id']}/checkout", {"expected_version": big["version"], "idempotency_key": str(uuid.uuid4()), "register_id": rid,
     "payments": [{"payment_method": "cash", "amount": "1500", "tendered_amount": "1500"}]}, token=T, expect=[400, 409, 422], label="oversell rejected")
call("POST", f"/sales/{big['id']}/cancel", {"expected_version": big["version"]}, token=T, expect=[200, 204], label="cancel draft sale")

# ------------------------------------------------------------------ customers / credit sale
sec("Customers, credit sale, payments, statements")
s, cust = call("POST", "/customers", {"name": "Walk-in Wendy", "phone": "0771234567"}, token=T, expect=[200, 201])
cust_id = cust["id"]
s, cust = call("PATCH", f"/customers/{cust_id}/credit-policy", {"expected_version": cust["version"], "credit_enabled": True, "credit_limit": "200.00", "payment_terms_days": 14}, token=T, expect=200)
s, csale = call("POST", "/sales", {"location_id": loc, "customer_id": cust_id, "items": [{"variant_id": v2, "quantity": "10", "unit_price": "3.00"}]}, token=T, expect=[200, 201])
s, cout = call("POST", f"/sales/{csale['id']}/checkout", {"expected_version": csale["version"], "idempotency_key": str(uuid.uuid4()), "register_id": rid,
     "payments": [{"payment_method": "cash", "amount": "10.00", "tendered_amount": "10.00"}]}, token=T, expect=200, label="credit sale (partial payment)")
s, acct = call("GET", f"/customers/{cust_id}/account", token=T, expect=200)
print("        customer account:", json.dumps(acct)[:250])
s, outst = call("GET", f"/customers/{cust_id}/outstanding-sales", token=T, expect=200)
oitems = outst["items"] if isinstance(outst, dict) else outst
s, cpay = call("POST", f"/customers/{cust_id}/payments", {"idempotency_key": str(uuid.uuid4()), "amount": "8.00", "payment_method": "cash",
     "cash_session_id": sess_id, "allocations": [{"document_id": oitems[0]["document_id"] if "document_id" in oitems[0] else oitems[0]["id"], "amount": "8.00"}]}, token=T, expect=[200, 201], label="customer payment w/ allocation")
cpay_id = cpay.get("id") or cpay.get("payment_id") or (cpay.get("payment") or {}).get("id")
call("GET", f"/customers/{cust_id}/statement?limit=20", token=T, expect=200)
s, cn = call("POST", f"/customers/{cust_id}/credit-notes", {"idempotency_key": str(uuid.uuid4()), "amount": "2.00", "reason": "goodwill"}, token=T, expect=[200, 201], label="customer credit note")
cn_id = cn.get("id") or cn.get("credit_note_id") or (cn.get("credit_note") or {}).get("id")
s, pd = call("GET", f"/customer-payments/{cpay_id}", token=T, expect=200, label="get customer payment + allocations")
alloc = pd["allocations"][0]
s, rev = call("POST", f"/customer-payments/{cpay_id}/reversals", {"idempotency_key": str(uuid.uuid4()), "amount": "8.00", "reason": "wrong customer", "payment_method": "cash", "cash_session_id": sess_id,
     "allocations": [{"allocation_id": alloc["allocation_id"], "amount": "8.00", "idempotency_key": str(uuid.uuid4())}]}, token=T, expect=[200, 201], label="reverse customer payment")
if cn_id:
    call("POST", f"/customer-credit-notes/{cn_id}/reversals", {"idempotency_key": str(uuid.uuid4()), "amount": "2.00", "reason": "issued in error"}, token=T, expect=[200, 201], label="reverse customer credit note")
s, acct2 = call("GET", f"/customers/{cust_id}/account", token=T, expect=200)
print("        customer account after reversals:", json.dumps(acct2)[:250])

# ------------------------------------------------------------------ returns / refunds / void
sec("Returns, refunds, voids")
s, sdet = call("GET", f"/sales/{sale_id}", token=T, expect=200)
item_id = [i for i in sdet["items"] if i["variant_id"] == v1][0]["id"]
s, ret = call("POST", f"/sales/{sale_id}/returns", {"reason": "customer changed mind", "idempotency_key": str(uuid.uuid4()),
     "items": [{"sale_item_id": item_id, "quantity": "1", "item_condition": "resellable"}]}, token=T, expect=[200, 201], label="create return")
ret_id = ret.get("id") or (ret.get("return") or {}).get("id")
call("POST", f"/sales/returns/{ret_id}/refunds", {"idempotency_key": str(uuid.uuid4()), "payment_method": "cash", "amount": "1.50", "register_id": rid}, token=T, expect=[200, 201], label="refund return (cash)")
call("GET", f"/sales/{sale_id}/refunds", token=T, expect=200)
call("GET", f"/sales/{sale_id}/returns", token=T, expect=200)
s, s3 = call("POST", "/sales", {"location_id": loc, "items": [{"variant_id": v1, "quantity": "1", "unit_price": "1.50"}]}, token=T, expect=[200, 201])
call("POST", f"/sales/{s3['id']}/checkout", {"expected_version": s3["version"], "idempotency_key": str(uuid.uuid4()), "register_id": rid,
     "payments": [{"payment_method": "cash", "amount": "1.50", "tendered_amount": "2.00"}]}, token=T, expect=200, label="checkout sale to void")
call("POST", f"/sales/{s3['id']}/void", {"idempotency_key": str(uuid.uuid4()), "reason": "cashier error", "register_id": rid,
     "refunds": [{"payment_method": "cash", "amount": "1.50"}]}, token=T, expect=[200, 201], label="void sale")
call("GET", f"/sales/{s3['id']}/void", token=T, expect=200)

# ------------------------------------------------------------------ supplier financials
sec("Supplier invoice, payment, credit note, reversal")
s, inv = call("POST", f"/suppliers/{sid}/invoices", {"idempotency_key": str(uuid.uuid4()), "invoice_number": f"INV-{SUFFIX}", "purchase_order_id": poid,
     "invoice_date": time.strftime("%Y-%m-%d"), "currency_code": "USD", "additional_charges": "0",
     "lines": [{"description": "Coca-Cola 500ml x100", "quantity": "100", "unit_cost": "1.00", "tax_amount": "0", "discount_amount": "0"},
               {"description": "Coca-Cola 2L x20", "quantity": "20", "unit_cost": "2.00", "tax_amount": "0", "discount_amount": "0"}]}, token=T, expect=[200, 201], label="supplier invoice")
inv_id = inv.get("id") or (inv.get("invoice") or {}).get("id")
call("GET", f"/supplier-invoices/{inv_id}", token=T, expect=200)
call("GET", f"/suppliers/{sid}/invoices", token=T, expect=200)
call("GET", f"/suppliers/{sid}/outstanding-invoices", token=T, expect=200)
s, spay = call("POST", f"/suppliers/{sid}/payments", {"idempotency_key": str(uuid.uuid4()), "amount": "40.00", "payment_method": "other", "allocations": [{"document_id": inv_id, "amount": "40.00"}]}, token=T, expect=[200, 201], label="supplier payment")
spay_id = spay.get("id") or spay.get("payment_id") or (spay.get("payment") or {}).get("id")
s, sacct = call("GET", f"/suppliers/{sid}/account", token=T, expect=200)
print("        supplier account:", json.dumps(sacct)[:250])
s, spd = call("GET", f"/supplier-payments/{spay_id}", token=T, expect=200, label="get supplier payment + allocations")
call("POST", f"/supplier-payments/{spay_id}/reversals", {"idempotency_key": str(uuid.uuid4()), "amount": "40.00", "reason": "bounced", "payment_method": "other",
     "allocations": [{"allocation_id": spd["allocations"][0]["allocation_id"], "amount": "40.00", "idempotency_key": str(uuid.uuid4())}]}, token=T, expect=[200, 201], label="reverse supplier payment")
call("GET", f"/suppliers/{sid}/statement", token=T, expect=200)

# ------------------------------------------------------------------ expenses
sec("Expenses")
today = time.strftime("%Y-%m-%d")
ekey = str(uuid.uuid4())
ebody = {"idempotency_key": ekey, "category": "Utilities", "description": "Electricity token", "amount": "12.50", "payment_method": "cash", "cash_session_id": sess_id}
s, ex = call("POST", "/expenses", ebody, token=T, expect=201, label="cash expense")
call("POST", "/expenses", ebody, token=T, expect=200, label="expense replay is idempotent")
call("POST", "/expenses", {**ebody, "amount": "13.00"}, token=T, expect=409, label="expense key reuse w/ different body")
call("POST", "/expenses", {"idempotency_key": str(uuid.uuid4()), "category": "Rent", "description": "Shop rent", "amount": "300", "payment_method": "bank_transfer"}, token=T, expect=201, label="bank expense")
call("POST", "/expenses", {"idempotency_key": str(uuid.uuid4()), "category": "Misc", "description": "too much cash", "amount": "99999", "payment_method": "cash", "cash_session_id": sess_id}, token=T, expect=409, label="expense beyond till cash rejected")
call("GET", "/expenses", token=T, expect=200)
call("GET", f"/expenses/summary", token=T, expect=200)
exid = ex["expense"]["id"]
call("POST", f"/expenses/{exid}/void", {"expected_version": ex["expense"]["version"], "reason": "entered twice"}, token=T, expect=200, label="void cash expense (returns cash to till)")

# ------------------------------------------------------------------ reports
sec("Reports")
for r in ["dashboard", "sales/summary", "sales/by-product", "sales/by-category", "sales/by-payment-method", "sales/transactions",
          "sales/returns", "sales/gross-profit", "inventory/stock", "inventory/valuation", "inventory/movements", "inventory/activity",
          "purchasing/summary", "purchasing/by-supplier", "purchasing/goods-received", "purchasing/outstanding-orders",
          f"suppliers/{sid}/purchase-history", "cash/registers", "cash/sessions", "cash/movements"]:
    s, js = call("GET", f"/reports/{r}", token=T, expect=200, label=f"report {r}")
s, gp = call("GET", "/reports/sales/gross-profit", token=T, expect=200, label="gross profit")
print("        gross profit:", json.dumps(gp)[:300])

# ------------------------------------------------------------------ close session
sec("Close cash session")
s, summ = call("GET", f"/cash-sessions/{sess_id}/summary", token=T, expect=200)
expected = summ["calculated_expected_cash"]
assert float(expected) == 116.0, f"till arithmetic wrong: expected 116.00, got {expected}"
print("        summary:", json.dumps(summ)[:400])
s, closed = call("POST", f"/cash-sessions/{sess_id}/close", {"closing_cash_count": "115.00"}, token=T, expect=200, label="close session (1.00 short)")
print("        close result:", json.dumps(closed)[:250])
call("POST", f"/cash-sessions/{sess_id}/close", {"closing_cash_count": "115.00"}, token=T, expect=[400, 409], label="closing twice is rejected")
call("POST", "/expenses", {"idempotency_key": str(uuid.uuid4()), "category": "Misc", "description": "closed till", "amount": "1", "payment_method": "cash", "cash_session_id": sess_id}, token=T, expect=409, label="no expense against a closed session")

# ------------------------------------------------------------------ RBAC
sec("RBAC & users")
s, roles = call("GET", "/roles", token=T, expect=200)
rl = roles if isinstance(roles, list) else roles.get("items", [])
cashier_role = [r for r in rl if r.get("role_key") == "cashier"][0]
s, nu = call("POST", "/users", {"name": "Cathy Cashier", "username": "cathy", "initial_password": PW, "role_ids": [cashier_role["id"]]}, token=T, expect=[200, 201], label="create cashier")
s, lg2 = call("POST", "/auth/login", {"company_id": cid, "username": "cathy", "password": PW}, expect=200, label="cashier login")
CT = lg2["access_token"]
call("GET", "/products", token=CT, expect=200, label="cashier can view products")
call("POST", "/products", {"name": "x", "variant": {"name": "y", "unit_id": unit_id, "selling_price": "1"}}, token=CT, expect=403, label="cashier cannot create products")
call("GET", "/audit-logs", token=CT, expect=403, label="cashier cannot read audit log")
call("GET", "/users", token=CT, expect=403, label="cashier cannot list users")
s, cs = call("POST", "/sales", {"location_id": loc, "items": [{"variant_id": v1, "quantity": "1", "unit_price": "1.50"}]}, token=CT, expect=[200, 201], label="cashier creates draft sale")
call("PUT", f"/sales/{cs['id']}", {"expected_version": cs["version"], "location_id": loc, "items": [{"variant_id": v1, "quantity": "2", "unit_price": "1.50"}]}, token=CT, expect=200, label="cashier edits draft sale")
s, al = call("GET", "/audit-logs?limit=5", token=T, expect=200, label="owner reads audit log")
print("        latest audit entries:", len(al) if isinstance(al, list) else al)

# ------------------------------------------------------------------ tenant isolation
sec("Multi-tenant isolation")
s, reg2 = call("POST", "/auth/register", {"company": {"name": f"Other Shop {SUFFIX}", "default_currency": "USD"},
     "owner": {"name": "Other", "username": "other", "email": f"x{SUFFIX}@example.com", "password": PW}}, expect=[200, 201])
s, lg3 = call("POST", "/auth/login", {"company_id": reg2["company_id"], "username": "other", "password": PW}, expect=200)
OT = lg3["access_token"]
call("GET", f"/products/{pid}", token=OT, expect=404, label="other company cannot read our product")
call("GET", f"/sales/{sale_id}", token=OT, expect=404, label="other company cannot read our sale")
call("GET", f"/customers/{cust_id}/account", token=OT, expect=404, label="other company cannot read our customer")
call("GET", f"/expenses/{exid}", token=OT, expect=404, label="other company cannot read our expense")
# fresh company works end-to-end with default location (regression for the bug fixed)
s, ol = call("GET", "/locations", token=OT, expect=200)
assert len(ol) == 1 and ol[0]["is_default"], "new company must have a default location"

print("\n" + "=" * 60)
if failures:
    print(f"{len(failures)} FAILED:")
    for f in failures:
        print("  -", f)
    sys.exit(1)
print("ALL CHECKS PASSED")
