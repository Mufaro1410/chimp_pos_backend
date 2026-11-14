#!/usr/bin/env python3
"""Add concise operation inventory entries for routes currently absent from OpenAPI.

Run once after reviewing the route additions. Detailed model schemas are maintained
in docs/openapi.yaml; generated inventory operations point to those schemas where
the Rust handler has a stable model and explicitly mark response shape otherwise.
"""

from pathlib import Path

import yaml

ROOT = Path(__file__).resolve().parents[1]
SPEC_PATH = ROOT / "docs/openapi.yaml"

PERMISSIONS = {
    "GET /api/v1/cash-registers": "cash_register.view",
    "POST /api/v1/cash-registers": "cash_register.manage",
    "POST /api/v1/cash-registers/{register_id}/sessions": "cash_register.open",
    "GET /api/v1/cash-registers/{register_id}/sessions/open": "cash_register.view",
    "GET /api/v1/cash-sessions/{session_id}": "cash_register.view",
    "GET /api/v1/cash-sessions/{session_id}/close": "cash_register.close",
    "POST /api/v1/cash-sessions/{session_id}/close": "cash_register.close",
    "GET /api/v1/cash-sessions/{session_id}/movements": "cash_register.view",
    "POST /api/v1/cash-sessions/{session_id}/movements": "cash_register.movement",
    "GET /api/v1/cash-sessions/{session_id}/summary": "cash_register.view",
    "GET /api/v1/customers": "customers.view",
    "POST /api/v1/customers": "customers.profile.manage",
    "PUT /api/v1/customers/{id}": "customers.profile.manage",
    "PATCH /api/v1/customers/{id}/credit-policy": "customers.credit",
    "GET /api/v1/suppliers": "suppliers.view",
    "POST /api/v1/suppliers": "suppliers.manage",
    "PUT /api/v1/suppliers/{id}": "suppliers.manage",
    "GET /api/v1/inventory/stock": "inventory.view",
    "GET /api/v1/inventory/movements": "inventory.view",
    "POST /api/v1/inventory/adjustments": "inventory.adjust",
    "GET /api/v1/products": "products.view",
    "POST /api/v1/products": "products.create",
    "GET /api/v1/products/{id}": "products.view",
    "PUT /api/v1/products/{id}": "products.update",
    "DELETE /api/v1/products/{id}": "products.archive",
    "GET /api/v1/products/{product_id}/variants": "products.view",
    "POST /api/v1/products/{product_id}/variants": "products.create",
    "PUT /api/v1/products/{product_id}/variants/{variant_id}": "products.update",
    "DELETE /api/v1/products/{product_id}/variants/{variant_id}": "products.archive",
    "GET /api/v1/purchases": "purchases.view",
    "POST /api/v1/purchases": "purchases.create",
    "GET /api/v1/purchases/{id}": "purchases.view",
    "PUT /api/v1/purchases/{id}": "purchases.update",
    "POST /api/v1/purchases/{id}/cancel": "purchases.cancel",
    "GET /api/v1/purchases/{id}/receipts": "purchases.view",
    "POST /api/v1/purchases/{id}/receipts": "purchases.receive",
    "POST /api/v1/purchases/{id}/submit": "purchases.update",
    "GET /api/v1/sales": "sales.view",
    "POST /api/v1/sales": "sales.create",
    "GET /api/v1/sales/{id}": "sales.view",
    "PUT /api/v1/sales/{id}": "sales.update",
    "POST /api/v1/sales/{id}/cancel": "sales.cancel",
    "POST /api/v1/sales/{id}/checkout": "sales.checkout",
    "GET /api/v1/sales/{id}/receipt": "sales.view",
    "GET /api/v1/sales/{id}/refunds": "sales.view",
    "GET /api/v1/sales/{id}/returns": "sales.view",
    "POST /api/v1/sales/{id}/returns": "sales.return",
    "GET /api/v1/sales/{id}/void": "sales.view",
    "POST /api/v1/sales/{id}/void": "sales.void",
    "POST /api/v1/sales/returns/{return_id}/refunds": "sales.refund",
    "GET /health": None,
}

SUMMARIES = {
    "/api/v1/products": "List or create products",
    "/api/v1/products/{id}": "Retrieve, update or deactivate a product",
    "/api/v1/products/{product_id}/variants": "List or create product variants",
    "/api/v1/products/{product_id}/variants/{variant_id}": "Update or deactivate a product variant",
    "/api/v1/inventory/stock": "List current stock balances",
    "/api/v1/inventory/movements": "List stock movements",
    "/api/v1/inventory/adjustments": "Post an idempotent stock adjustment",
    "/api/v1/purchases": "List or create purchase orders",
    "/api/v1/purchases/{id}": "Retrieve or update a purchase order",
    "/api/v1/purchases/{id}/submit": "Submit a purchase order",
    "/api/v1/purchases/{id}/cancel": "Cancel a purchase order",
    "/api/v1/purchases/{id}/receipts": "List receipts or receive goods",
    "/api/v1/sales": "List or create sales",
    "/api/v1/sales/{id}": "Retrieve or update a sale",
    "/api/v1/sales/{id}/checkout": "Complete checkout for a sale",
    "/api/v1/sales/{id}/receipt": "Retrieve a sale receipt",
    "/api/v1/sales/{id}/cancel": "Cancel a draft sale",
    "/api/v1/sales/{id}/returns": "List or create sale returns",
    "/api/v1/sales/{id}/refunds": "List refunds for a sale",
    "/api/v1/sales/{id}/void": "Retrieve or post a sale void",
    "/api/v1/sales/returns/{return_id}/refunds": "Post a refund for a return",
    "/api/v1/customers": "List or create customers",
    "/api/v1/customers/{id}": "Update a customer profile",
    "/api/v1/customers/{id}/credit-policy": "Update customer credit policy",
    "/api/v1/suppliers": "List or create suppliers",
    "/api/v1/suppliers/{id}": "Update a supplier",
    "/api/v1/cash-registers": "List or create cash registers",
    "/api/v1/cash-registers/{register_id}/sessions": "Open a cash register session",
    "/api/v1/cash-registers/{register_id}/sessions/open": "Retrieve the open cash session",
    "/api/v1/cash-sessions/{session_id}": "Retrieve a cash session",
    "/api/v1/cash-sessions/{session_id}/summary": "Retrieve a cash session summary",
    "/api/v1/cash-sessions/{session_id}/movements": "List or post cash movements",
    "/api/v1/cash-sessions/{session_id}/close": "Close a cash session",
    "/health": "Check API process health",
}


def main() -> None:
    raw = SPEC_PATH.read_text()
    spec = yaml.safe_load(raw)
    method_names = {
        "get": "GET", "post": "POST", "put": "PUT", "patch": "PATCH", "delete": "DELETE"
    }
    missing: dict[str, dict] = {}
    for path, item in spec["paths"].items():
        for method, operation in item.items():
            if method not in method_names:
                continue
            key = f"{method_names[method]} {path}"
            PERMISSIONS.setdefault(key, None)
    # Route paths and HTTP methods are extracted by the validator; entries below
    # cover routes absent from the current document.
    from validate_openapi import route_operations
    registered = route_operations((ROOT / "src/routes/mod.rs").read_text())
    documented = {
        (method_names[m], p)
        for p, item in spec["paths"].items()
        for m in item
        if m in method_names
    }
    for method, path in sorted(registered - documented):
        op: dict = {
            "summary": SUMMARIES.get(path, f"{method.title()} {path}"),
            "description": "Company-scoped endpoint. This route inventory entry still needs a typed request/response schema linked to the corresponding Rust model.",
            "responses": {
                "default": {
                    "description": "Handler-specific status and response schema require typed contract review."
                }
            },
        }
        permission = PERMISSIONS.get(f"{method} {path}")
        if permission:
            op["security"] = [{"bearerAuth": []}]
            op["x-required-permission"] = permission
        elif (method, path) != ("GET", "/health"):
            raise SystemExit(f"permission metadata missing for {method} {path}")
        if "{" in path:
            parameter_name = path.split("{")[1].split("}")[0]
            op["parameters"] = [{
                "in": "path",
                "name": parameter_name,
                "required": False,
                "schema": {"type": "integer", "format": "int64"},
            }]
        if method in {"POST", "PUT", "PATCH"}:
            op["requestBody"] = {
                "required": True,
                "content": {"application/json": {"schema": {"type": "object", "additionalProperties": True}}},
            }
        missing.setdefault(path, {})[method.lower()] = op
    if not missing:
        print("No missing route operations.")
        return
    spec["paths"].update(missing)
    # Insert only new path blocks before components, leaving existing paths intact.
    block = yaml.safe_dump(missing, sort_keys=False, allow_unicode=True, width=110)
    block = "\n".join(f"  {line}" if line else line for line in block.splitlines()) + "\n"
    marker = "components:\n"
    if marker not in raw:
        raise SystemExit("components section not found")
    raw = raw.replace(marker, block + marker, 1)
    SPEC_PATH.write_text(raw)
    print(f"Added {sum(len(v) for v in missing.values())} route operation entries.")


if __name__ == "__main__":
    main()
