#!/usr/bin/env python3
"""
Volume / latency test (stdlib only).
Usage: python3 scripts/load_test.py [BASE_URL] [PRODUCTS=2000] [SALES=1000]
Creates a throw-away company with many products and sales, then times the endpoints that matter.
"""
import json, sys, time, uuid, threading, urllib.request, urllib.error, statistics

BASE = (sys.argv[1] if len(sys.argv) > 1 else "http://127.0.0.1:3000").rstrip("/")
N_PRODUCTS = int(sys.argv[2]) if len(sys.argv) > 2 else 2000
N_SALES = int(sys.argv[3]) if len(sys.argv) > 3 else 1000
API = BASE + "/api/v1"
PW = "CorrectHorseBattery!42"
SUFFIX = uuid.uuid4().hex[:6]
THREADS = 8


def call(method, path, body=None, token=None):
    req = urllib.request.Request(API + path, data=json.dumps(body).encode() if body is not None else None, method=method)
    req.add_header("content-type", "application/json")
    if token:
        req.add_header("authorization", f"Bearer {token}")
    t0 = time.perf_counter()
    try:
        with urllib.request.urlopen(req, timeout=120) as r:
            raw, st = r.read(), r.status
    except urllib.error.HTTPError as e:
        raw, st = e.read(), e.code
    ms = (time.perf_counter() - t0) * 1000
    try:
        return st, (json.loads(raw) if raw else None), ms
    except Exception:
        return st, None, ms


def parallel(n, fn):
    idx, lock, errors = [0], threading.Lock(), []

    def worker():
        while True:
            with lock:
                i = idx[0]; idx[0] += 1
            if i >= n:
                return
            try:
                fn(i)
            except Exception as e:  # noqa
                errors.append(repr(e))

    ts = [threading.Thread(target=worker) for _ in range(THREADS)]
    [t.start() for t in ts]; [t.join() for t in ts]
    return errors


st, reg, _ = call("POST", "/auth/register", {"company": {"name": f"Load {SUFFIX}", "default_currency": "USD"},
    "owner": {"name": "Owner", "username": "owner", "email": f"o{SUFFIX}@example.com", "password": PW}})
T = call("POST", "/auth/login", {"company_id": reg["company_id"], "username": "owner", "password": PW})[1]["access_token"]
unit = call("GET", "/units", token=T)[1][0]["id"]
loc = call("GET", "/locations", token=T)[1][0]["id"]
cats = [call("POST", "/categories", {"name": f"Category {i}"}, token=T)[1]["id"] for i in range(20)]

print(f"Creating {N_PRODUCTS} products ...", flush=True)
variant_ids = [None] * N_PRODUCTS
t0 = time.time()


def make_product(i):
    st, p, _ = call("POST", "/products", {"name": f"Product {i:05d} {'milk' if i % 7 == 0 else 'item'}", "category_id": cats[i % 20],
        "variant": {"name": "std", "sku": f"SKU-{SUFFIX}-{i:05d}", "barcode": f"{SUFFIX}{i:07d}", "unit_id": unit, "selling_price": f"{1 + i % 50}.00", "cost_price": f"{0.5 + (i % 50) / 2}"}}, token=T)
    v = call("GET", f"/products/{p['id']}/variants", token=T)[1]
    variant_ids[i] = (v if isinstance(v, list) else v["items"])[0]["id"]
    call("POST", "/inventory/adjustments", {"location_id": loc, "variant_id": variant_ids[i], "movement_type": "adjustment_in", "quantity_delta": "1000", "idempotency_key": str(uuid.uuid4())}, token=T)


errs = parallel(N_PRODUCTS, make_product)
print(f"  done in {time.time() - t0:.1f}s ({N_PRODUCTS / (time.time() - t0):.0f} products/s), errors: {len(errs)}")

rg = call("POST", "/cash-registers", {"location_id": loc, "code": "R1", "name": "Till"}, token=T)[1]
sess = call("POST", f"/cash-registers/{rg['id']}/sessions", {"opening_float": "100"}, token=T)[1]
custs = [call("POST", "/customers", {"name": f"Customer {i}"}, token=T)[1]["id"] for i in range(200)]

print(f"Creating {N_SALES} completed sales ...", flush=True)
t0 = time.time()
lat = []


def make_sale(i):
    items, seen = [], set()
    for k in range(3):
        vid = variant_ids[(i * 3 + k * 37) % N_PRODUCTS]
        if vid in seen:
            continue
        seen.add(vid)
        items.append({"variant_id": vid, "quantity": "1", "unit_price": f"{1 + ((i * 3 + k * 37) % N_PRODUCTS) % 50}.00"})
    total = sum(float(x["unit_price"]) for x in items)
    st, s, ms1 = call("POST", "/sales", {"location_id": loc, "customer_id": custs[i % 200] if i % 3 == 0 else None, "items": items}, token=T)
    st, r, ms2 = call("POST", f"/sales/{s['id']}/checkout", {"expected_version": s["version"], "idempotency_key": str(uuid.uuid4()), "register_id": rg["id"],
        "payments": [{"payment_method": "cash", "amount": f"{total:.2f}", "tendered_amount": f"{total:.2f}"}]}, token=T)
    lat.append(ms1 + ms2)
    if st != 200:
        raise RuntimeError(f"checkout {st} {r}")


errs = parallel(N_SALES, make_sale)
dt = time.time() - t0
print(f"  done in {dt:.1f}s ({N_SALES / dt:.0f} sales/s), errors: {len(errs)} {errs[:2]}")
if lat:
    lat.sort()
    print(f"  sale create+checkout latency under {THREADS}-way load: p50 {lat[len(lat)//2]:.0f} ms, p95 {lat[int(len(lat)*.95)]:.0f} ms, max {lat[-1]:.0f} ms")

SETTLE = int(__import__("os").environ.get("LOAD_SETTLE_SECONDS", "20"))
if SETTLE:
    print(f"\nWaiting {SETTLE}s for the server's periodic ANALYZE (run the server with ANALYZE_INTERVAL_SECONDS=10 for this test) ...", flush=True)
    time.sleep(SETTLE)
print("\nEndpoint timings (median of 5, ms):")
probes = [
    ("GET /products?search=milk&limit=50", "/products?search=milk&limit=50"),
    ("GET /products?limit=100&offset=1900", "/products?limit=100&offset=1900"),
    ("GET /inventory/stock (first page)", f"/inventory/stock?location_id={loc}&limit=100"),
    ("GET /sales?limit=50", "/sales?limit=50"),
    ("GET /customers?search=Customer 1", "/customers?search=Customer%201"),
    ("GET /reports/dashboard", "/reports/dashboard"),
    ("GET /reports/sales/summary", "/reports/sales/summary"),
    ("GET /reports/sales/by-product", "/reports/sales/by-product"),
    ("GET /reports/sales/by-category", "/reports/sales/by-category"),
    ("GET /reports/sales/gross-profit", "/reports/sales/gross-profit"),
    ("GET /reports/inventory/valuation", "/reports/inventory/valuation"),
    ("GET /reports/inventory/stock", "/reports/inventory/stock"),
    ("GET /reports/cash/sessions", "/reports/cash/sessions"),
    ("GET /sync/pull/variants (1000/page)", "/sync/pull/variants?limit=1000"),
    ("GET /sync/pull/stock (1000/page)", "/sync/pull/stock?limit=1000"),
    ("GET /sync/pull/customers", "/sync/pull/customers?limit=1000"),
]
slow = []
for label, path in probes:
    times = []
    for _ in range(5):
        st, js, ms = call("GET", path, token=T)
        times.append(ms)
        if st != 200:
            break
    med = statistics.median(times)
    flag = "" if st == 200 and med < 500 else ("   <-- SLOW" if st == 200 else f"   <-- HTTP {st}")
    if flag:
        slow.append((label, med, st))
    print(f"  {label:<44} {med:7.0f}{flag}")

# full initial sync of the catalogue, as a new tablet would do
t0 = time.time(); pages = 0; cursor = ""; rows = 0
while True:
    st, js, _ = call("GET", f"/sync/pull/variants?limit=1000" + (f"&cursor={cursor}" if cursor else ""), token=T)
    pages += 1; rows += len(js["items"]); cursor = js["next_cursor"]
    if not js["has_more"]:
        break
print(f"\nFull catalogue sync: {rows} variants in {pages} pages, {time.time() - t0:.2f}s")
print("\n" + ("SLOW ENDPOINTS: " + ", ".join(f"{a} ({b:.0f} ms)" for a, b, _ in slow) if slow else "ALL ENDPOINTS UNDER 500 ms"))
