#!/usr/bin/env python3
"""Replace Milestone 4 inventory stubs with contracts for their Rust handlers."""

from pathlib import Path

import yaml

ROOT = Path(__file__).resolve().parents[1]
SPEC_PATH = ROOT / "docs/openapi.yaml"
REF = lambda name: {"$ref": f"#/components/schemas/{name}"}

# key -> (summary, response schema, success status, optional request schema, queries)
CONTRACTS = {
    ("GET", "/api/v1/products"): ("List company products", {"type": "array", "items": REF("Product")}, "200", None, []),
    ("POST", "/api/v1/products"): ("Create a product and its first variant", REF("Product"), "201", "CreateProductRequest", []),
    ("GET", "/api/v1/products/{id}"): ("Get a product and its variants", REF("Product"), "200", None, []),
    ("PUT", "/api/v1/products/{id}"): ("Update a product and its default variant", REF("Product"), "200", "UpdateProductRequest", []),
    ("DELETE", "/api/v1/products/{id}"): ("Deactivate a product", None, "204", None, []),
    ("GET", "/api/v1/products/{product_id}/variants"): ("List a product's variants", {"type": "array", "items": REF("ProductVariant")}, "200", None, []),
    ("POST", "/api/v1/products/{product_id}/variants"): ("Create a variant for a product", REF("ProductVariant"), "201", "ProductVariantRequest", []),
    ("PUT", "/api/v1/products/{product_id}/variants/{variant_id}"): ("Update a product variant using its expected version", REF("ProductVariant"), "200", "UpdateProductVariantRequest", []),
    ("DELETE", "/api/v1/products/{product_id}/variants/{variant_id}"): ("Deactivate a product variant", None, "204", None, []),
    ("GET", "/api/v1/inventory/stock"): ("List current stock balances", {"type": "array", "items": REF("StockItem")}, "200", None, ["stock"]),
    ("GET", "/api/v1/inventory/movements"): ("List paginated stock movements", {"type": "array", "items": REF("StockMovement")}, "200", None, ["movement"]),
    ("POST", "/api/v1/inventory/adjustments"): ("Post an idempotent stock adjustment", REF("StockAdjustmentResponse"), "201", "StockAdjustmentRequest", []),
    ("GET", "/api/v1/purchases"): ("List purchase orders", {"type": "array", "items": REF("PurchaseOrder")}, "200", None, ["purchase"]),
    ("POST", "/api/v1/purchases"): ("Create a draft purchase order", REF("PurchaseOrder"), "201", "CreatePurchaseOrderRequest", []),
    ("GET", "/api/v1/purchases/{id}"): ("Get a purchase order", REF("PurchaseOrder"), "200", None, []),
    ("PUT", "/api/v1/purchases/{id}"): ("Update an editable purchase order", REF("PurchaseOrder"), "200", "UpdatePurchaseOrderRequest", []),
    ("POST", "/api/v1/purchases/{id}/submit"): ("Submit a purchase order", None, "204", None, []),
    ("POST", "/api/v1/purchases/{id}/cancel"): ("Cancel an editable purchase order", None, "204", None, []),
    ("GET", "/api/v1/purchases/{id}/receipts"): ("List goods receipts for a purchase order", {"type": "array", "items": REF("GoodsReceipt")}, "200", None, []),
    ("POST", "/api/v1/purchases/{id}/receipts"): ("Receive goods against a purchase order", REF("GoodsReceipt"), "201", "ReceiveGoodsRequest", []),
    ("GET", "/api/v1/sales"): ("List sales", {"type": "array", "items": REF("Sale")}, "200", None, ["sale"]),
    ("POST", "/api/v1/sales"): ("Create a draft sale", REF("Sale"), "201", "CreateSaleRequest", []),
    ("GET", "/api/v1/sales/{id}"): ("Get a sale", REF("Sale"), "200", None, []),
    ("PUT", "/api/v1/sales/{id}"): ("Update an editable sale", REF("Sale"), "200", "UpdateSaleRequest", []),
    ("POST", "/api/v1/sales/{id}/checkout"): ("Check out a sale using an idempotency key", REF("Sale"), "200", "CheckoutRequest", []),
    ("GET", "/api/v1/sales/{id}/receipt"): ("Retrieve the sale receipt", REF("Sale"), "200", None, []),
    ("POST", "/api/v1/sales/{id}/cancel"): ("Cancel a draft sale", None, "204", None, []),
    ("GET", "/api/v1/sales/{id}/returns"): ("List returns for a sale", {"type": "array", "items": REF("SaleReturn")}, "200", None, []),
    ("POST", "/api/v1/sales/{id}/returns"): ("Record a return against a sale", REF("SaleReturn"), "201", "CreateSaleReturnRequest", []),
    ("POST", "/api/v1/sales/returns/{return_id}/refunds"): ("Refund a recorded sale return", REF("SaleRefund"), "201", "CreateRefundRequest", []),
    ("GET", "/api/v1/sales/{id}/refunds"): ("List refunds for a sale", {"type": "array", "items": REF("SaleRefund")}, "200", None, []),
    ("GET", "/api/v1/sales/{id}/void"): ("Get the sale void record, or null when none exists", {"anyOf": [REF("SaleVoid"), {"type": "null"}]}, "200", None, []),
    ("POST", "/api/v1/sales/{id}/void"): ("Void a sale and record any refunds", REF("SaleVoid"), "201", "VoidSaleRequest", []),
    ("GET", "/api/v1/customers"): ("List company customers", {"type": "array", "items": REF("Customer")}, "200", None, []),
    ("POST", "/api/v1/customers"): ("Create a customer", REF("Customer"), "201", "CustomerCreate", []),
    ("PUT", "/api/v1/customers/{id}"): ("Update a customer profile", REF("Customer"), "200", "CustomerUpdate", []),
    ("PATCH", "/api/v1/customers/{id}/credit-policy"): ("Update a customer's credit policy", REF("Customer"), "200", "CustomerCreditPolicyUpdate", []),
    ("GET", "/api/v1/suppliers"): ("List company suppliers", {"type": "array", "items": REF("Supplier")}, "200", None, []),
    ("POST", "/api/v1/suppliers"): ("Create a supplier", REF("Supplier"), "201", "SupplierCreate", []),
    ("PUT", "/api/v1/suppliers/{id}"): ("Update a supplier", REF("Supplier"), "200", "SupplierUpdate", []),
    ("GET", "/api/v1/cash-registers"): ("List cash registers", {"type": "array", "items": REF("CashRegister")}, "200", None, ["register"]),
    ("POST", "/api/v1/cash-registers"): ("Create a cash register", REF("CashRegister"), "201", "CreateCashRegisterRequest", []),
    ("POST", "/api/v1/cash-registers/{register_id}/sessions"): ("Open a cash session", REF("CashRegisterSession"), "201", "OpenCashSessionRequest", []),
    ("GET", "/api/v1/cash-registers/{register_id}/sessions/open"): ("Get the open session for a register", REF("CashRegisterSession"), "200", None, []),
    ("GET", "/api/v1/cash-sessions/{session_id}"): ("Get a cash session", REF("CashRegisterSession"), "200", None, []),
    ("GET", "/api/v1/cash-sessions/{session_id}/summary"): ("Get cash session totals and expected cash", REF("CashSessionSummary"), "200", None, []),
    ("GET", "/api/v1/cash-sessions/{session_id}/movements"): ("List manual movements for a cash session", {"type": "array", "items": REF("CashMovement")}, "200", None, []),
    ("POST", "/api/v1/cash-sessions/{session_id}/movements"): ("Record an idempotent manual cash movement", REF("CashMovement"), "201", "CashMovementRequest", []),
    ("POST", "/api/v1/cash-sessions/{session_id}/close"): ("Close a cash session with a counted balance", REF("CashRegisterSession"), "200", "CloseCashSessionRequest", []),
    ("GET", "/api/v1/inventory/stock"): ("List current stock balances", {"type": "array", "items": REF("StockItem")}, "200", None, ["stock"]),
    ("GET", "/health"): ("Check API and database availability", REF("HealthResponse"), "200", None, []),
}

SUMMARY_DETAILS = {
    "/api/v1/products": "Returns every company product; this endpoint has no query filters or pagination.",
    "/api/v1/customers": "Returns every company customer; this endpoint has no query filters or pagination.",
    "/api/v1/suppliers": "Returns every company supplier; this endpoint has no query filters or pagination.",
    "/api/v1/inventory/stock": "Optional location_id and variant_id filters are applied; this endpoint is not paginated.",
    "/api/v1/inventory/movements": "Optional location_id and variant_id filters; limit defaults to 50 and is clamped to 1–100; offset defaults to 0 and is clamped to 0 or greater.",
    "/api/v1/sales": "Optional status (draft, completed, cancelled) and location_id filters; limit defaults to 50 and is clamped to 1–100; offset defaults to 0 and is clamped to 0 or greater.",
    "/api/v1/purchases": "Optional supplier_id and status filters; status values are draft, ordered, partially_received, received, cancelled; limit defaults to 50 and is clamped to 1–100; offset defaults to 0 and is clamped to 0 or greater.",
    "/api/v1/cash-registers": "Optional location_id filter; this endpoint is not paginated.",
    "/health": "This public liveness/readiness endpoint queries the database; database failure responds 503.",
}
METHOD_DETAILS = {
    ("POST", "/api/v1/products"): "Creates a product and its first variant.",
    ("POST", "/api/v1/customers"): "Creates a customer in the authenticated company.",
    ("POST", "/api/v1/suppliers"): "Creates a supplier in the authenticated company.",
    ("POST", "/api/v1/purchases"): "Creates a draft purchase order.",
    ("POST", "/api/v1/sales"): "Creates a draft sale.",
    ("POST", "/api/v1/cash-registers"): "Creates a cash register.",
    ("POST", "/api/v1/sales/{id}/returns"): "A new return responds 201; an idempotent replay responds 200.",
    ("POST", "/api/v1/sales/returns/{return_id}/refunds"): "A new refund responds 201; an idempotent replay responds 200.",
    ("POST", "/api/v1/sales/{id}/void"): "A new void responds 201; an idempotent replay responds 200.",
    ("POST", "/api/v1/purchases/{id}/receipts"): "A new goods receipt responds 201; an idempotent replay responds 200.",
    ("POST", "/api/v1/sales/{id}/checkout"): "The handler returns 200 for both initial completion and idempotent replay.",
}


def example_for(schema, schemas, key=""):
    if "$ref" in schema:
        return example_for(schemas[schema["$ref"].rsplit("/", 1)[1]], schemas, key)
    if "anyOf" in schema:
        return example_for(next(s for s in schema["anyOf"] if s.get("type") != "null"), schemas, key)
    if "oneOf" in schema:
        return example_for(schema["oneOf"][0], schemas, key)
    typ = schema.get("type")
    if isinstance(typ, list):
        typ = next(t for t in typ if t != "null")
    if typ == "object":
        return {k: example_for(v, schemas, k) for k, v in schema.get("properties", {}).items()}
    if typ == "array":
        return [example_for(schema.get("items", {}), schemas)]
    if typ == "string":
        fmt = schema.get("format")
        if schema.get("enum"):
            return schema["enum"][0]
        known = {
            "uuid": "00000000-0000-0000-0000-000000000001",
            "date": "2026-11-12",
            "date-time": "2026-11-12T10:30:00Z",
            "email": "person@example.test",
        }
        if fmt in known:
            return known[fmt]
        samples = {
            "name": "Example product", "description": "Example description",
            "code": "TILL-01", "sku": "SKU-001", "barcode": "1234567890123",
            "username": "sample-user", "currency_code": "USD", "movement_type": "adjustment_in",
            "item_condition": "resellable", "payment_method": "card", "status": "draft",
            "reason": "Customer returned item", "reference": "REF-1001", "notes": "Example note",
            "unit_code": "ea", "unit_name": "Each", "unit_symbol": "ea",
            "tax_number": "TAX-001", "phone": "+263771234567", "address": "1 Example Road",
            "city": "Harare", "country_code": "ZW", "timezone": "Africa/Harare", "locale": "en-ZW",
            "payment_terms_days": "",
        }
        return samples.get(key, "Example")
    if typ == "integer":
        return 1
    if typ == "number":
        return 12.5
    if typ == "boolean":
        return True
    return None


def query_parameters(kind):
    def q(name, schema, description):
        example = {"location_id": 1, "variant_id": 1, "limit": 50, "offset": 0, "supplier_id": 1, "status": "draft"}.get(name)
        result = {"in": "query", "name": name, "required": False, "description": description, "schema": schema}
        if example is not None:
            result["example"] = example
        return result
    integer = {"type": "integer", "format": "int64"}
    page = [q("limit", {"type": "integer", "default": 50}, "Optional signed integer; the service defaults to 50 and clamps to 1–100."), q("offset", {"type": "integer", "default": 0}, "Optional signed integer; the service defaults to 0 and clamps negative values to 0.")]
    if kind == "stock":
        return [q("location_id", integer, "Filter by inventory location."), q("variant_id", integer, "Filter by product variant.")]
    if kind == "movement":
        return [q("location_id", integer, "Filter by inventory location."), q("variant_id", integer, "Filter by product variant."), *page]
    if kind == "sale":
        return [q("status", {"type": "string", "enum": ["draft", "completed", "cancelled"]}, "Filter by sale status."), q("location_id", integer, "Filter by location."), *page]
    if kind == "purchase":
        return [q("supplier_id", integer, "Filter by supplier."), q("status", {"type": "string", "enum": ["draft", "ordered", "partially_received", "received", "cancelled"]}, "Filter by purchase order status."), *page]
    if kind == "register":
        return [q("location_id", integer, "Filter by location.")]
    return []


def main():
    spec = yaml.safe_load(SPEC_PATH.read_text())
    schemas = spec["components"]["schemas"]
    schemas["StockAdjustmentRequest"]["properties"]["movement_type"]["enum"] = ["adjustment_in", "adjustment_out", "count_adjustment"]
    schemas["CashMovementRequest"]["properties"]["movement_type"]["enum"] = ["cash_in", "cash_out", "paid_out", "cash_drop"]
    schemas["ReturnItemRequest"]["properties"]["item_condition"]["enum"] = ["resellable", "damaged", "unsellable"]
    schemas["SalePaymentRequest"]["properties"]["payment_method"]["enum"] = ["cash", "card", "mobile_money", "other"]
    schemas["CreateRefundRequest"]["properties"]["payment_method"]["enum"] = ["cash", "card", "mobile_money", "other"]
    schemas["VoidRefundRequest"]["properties"]["payment_method"]["enum"] = ["cash", "card", "mobile_money", "other"]
    found = 0
    for (method, path), (summary, response_schema, status, request_name, query_kind) in CONTRACTS.items():
        operation = spec["paths"][path][method.lower()]
        found += 1
        operation["summary"] = summary
        detail = METHOD_DETAILS.get((method, path), SUMMARY_DETAILS.get(path))
        if method == "GET" and path == "/api/v1/purchases/{id}/receipts":
            detail = "Lists receipts belonging to this company-scoped purchase order."
        elif method == "GET" and path == "/api/v1/sales/{id}/returns":
            detail = "Lists returns belonging to this company-scoped sale."
        elif method == "GET" and path == "/api/v1/sales/{id}/void":
            detail = "Returns the void record, or JSON null when the sale has not been voided."
        if path == "/health":
            operation["description"] = (
                "Public health endpoint. Executes a lightweight database query. "
                "Returns 200 when the API can reach the database and an empty 503 response otherwise."
            )
        else:
            operation["description"] = ("Company-scoped. " + detail) if detail else "Company-scoped operation."
        operation.pop("requestBody", None)
        operation.pop("parameters", None)
        params = []
        for name in re.findall(r"\{([^}]+)\}", path):
            params.append({"in": "path", "name": name, "required": True, "schema": {"type": "integer", "format": "int64", "minimum": 1}, "example": 1})
        params.extend(query_parameters(query_kind))
        if params:
            operation["parameters"] = params
        responses = {}
        if response_schema is None:
            responses[status] = {"description": "Successful response with no body."}
        else:
            body_schema = response_schema if isinstance(response_schema, dict) else REF(response_schema)
            body_example = example_for(body_schema, schemas)
            responses[status] = {"description": "Successful response.", "content": {"application/json": {"schema": body_schema, "example": body_example}}}
        if (
            (path.endswith("/receipts") and method == "POST")
            or (path.endswith("/returns") and method == "POST")
            or (path.endswith("/refunds") and method == "POST")
            or (path.endswith("/void") and method == "POST")
        ):
            responses["200"] = responses.get("200", {"description": "Idempotent replay returns the previously posted resource."})
            responses["201"] = responses.get("201", {"description": "Resource created."})
            # Keep examples/schema on both replay and first-post responses.
            if response_schema is not None:
                response = {"description": "Idempotent replay returns the previously posted resource." if "200" in responses else "Resource created.", "content": {"application/json": {"schema": response_schema, "example": example_for(response_schema, schemas)}}}
                responses["200"] = response
                responses["201"] = {**response, "description": "Resource created."}
        errors = ["401", "403"]
        if path == "/health":
            errors = []
            responses["503"] = {"description": "Database health query failed; response body is empty."}
        else:
            if path.startswith("/api/v1/products"):
                errors.extend(["500"] if method == "GET" and path == "/api/v1/products" else ["404", "500"] if method == "GET" else ["400", "404", "409", "500"])
                if method == "POST" or method == "PUT": errors.extend(["400", "404", "409", "500"])
            elif path.startswith("/api/v1/inventory/"):
                errors.extend(["500"] if method == "GET" else ["400", "404", "409", "500"])
            elif path.startswith("/api/v1/purchases"):
                errors.extend(["400", "500"] if method == "GET" and path == "/api/v1/purchases" else ["404", "500"] if method == "GET" else ["404", "409", "500"] if path.endswith(("/submit", "/cancel")) else ["400", "404", "409", "500"])
            elif path.startswith("/api/v1/sales"):
                errors.extend(["400", "500"] if method == "GET" and path == "/api/v1/sales" else ["404", "500"] if method == "GET" else ["404", "409", "500"] if path.endswith("/cancel") else ["400", "403", "404", "409", "500"])
                if path.endswith(("/returns", "/refunds", "/void")) and method == "POST": errors.append("422")
            elif path.startswith("/api/v1/cash-"):
                errors.extend(["500"] if method == "GET" and path == "/api/v1/cash-registers" else ["404", "500"] if method == "GET" else ["400", "404", "409", "500"])
            elif path.startswith("/api/v1/customers") or path.startswith("/api/v1/suppliers"):
                errors.extend(["500"] if method == "GET" and path in {"/api/v1/customers", "/api/v1/suppliers"} else ["400", "404", "409", "500"])
                if path.endswith("/credit-policy"): errors.append("403")
        for code in errors:
            responses.setdefault(code, {"$ref": "#/components/responses/SecurityError" if code in {"401", "403"} else "#/components/responses/ApiError"})
        operation["responses"] = responses
        if path == "/health":
            responses["200"]["content"]["application/json"]["example"] = {
                "status": "ok",
                "database": "connected",
            }
        if request_name:
            request_schema = REF(request_name)
            operation["requestBody"] = {
                "required": True,
                "content": {"application/json": {"schema": request_schema, "example": example_for(request_schema, schemas)}},
            }
    if found != 50:
        raise SystemExit(f"Expected to complete 50 placeholders, found {found}")
    SPEC_PATH.write_text(yaml.safe_dump(spec, sort_keys=False, allow_unicode=True, width=110))
    print(f"Typed {found} placeholder operations")


if __name__ == "__main__":
    import re
    main()
