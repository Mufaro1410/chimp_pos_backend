#!/usr/bin/env python3
"""Generate OpenAPI component schemas for public Rust API model structs.

This is intentionally limited to the model structs used by the existing
handlers. Runtime/business validation is documented separately and is not
inferred from Rust field types alone.
"""

from __future__ import annotations

import re
from pathlib import Path

import yaml

ROOT = Path(__file__).resolve().parents[1]
SPEC_PATH = ROOT / "docs/openapi.yaml"
MODEL_FILES = [
    "cash_register.rs", "inventory.rs", "product.rs", "purchasing.rs",
    "sales.rs", "sales_returns.rs", "trading_parties.rs",
]


def schema_type(rust_type: str):
    t = rust_type.strip()
    t = t.replace("chrono::DateTime", "DateTime").replace("chrono::Utc", "Utc")
    if t.startswith("Option<") and t.endswith(">"):
        inner = schema_type(t[7:-1])
        if "$ref" in inner:
            return {"anyOf": [inner, {"type": "null"}]}
        return {**inner, "type": [inner["type"], "null"]}
    if t.startswith("Vec<") and t.endswith(">"):
        return {"type": "array", "items": schema_type(t[4:-1])}
    primitives = {
        "String": {"type": "string"},
        "bool": {"type": "boolean"},
        "i32": {"type": "integer", "format": "int32"},
        "i64": {"type": "integer", "format": "int64"},
        "u64": {"type": "integer", "format": "int64", "minimum": 0},
        "Decimal": {"type": "number", "format": "decimal"},
        "Uuid": {"type": "string", "format": "uuid"},
        "NaiveDate": {"type": "string", "format": "date"},
        "DateTime<Utc>": {"type": "string", "format": "date-time"},
    }
    if t in primitives:
        return primitives[t]
    return {"$ref": f"#/components/schemas/{t.split('::')[-1]}"}


def structs(source: str):
    out = {}
    pattern = re.compile(r"#\[derive\(([^\]]+)\)\](.*?)pub struct (\w+)\s*\{", re.S)
    for match in pattern.finditer(source):
        derives, prefix, name = match.groups()
        body_start = match.end()
        depth, i = 1, body_start
        while i < len(source) and depth:
            if source[i] == "{": depth += 1
            elif source[i] == "}": depth -= 1
            i += 1
        body = source[body_start:i-1]
        if "Serialize" not in derives and "Deserialize" not in derives:
            continue
        properties, required = {}, []
        defaulted = False
        for line in body.splitlines():
            line = line.strip()
            if line.startswith("#[serde(default"):
                defaulted = True
                continue
            field = re.match(r"pub (\w+)\s*:\s*([^,]+),", line)
            if not field:
                continue
            field_name, rust_type = field.groups()
            rust_type = rust_type.strip()
            properties[field_name] = schema_type(rust_type)
            if not rust_type.startswith("Option<") and not defaulted:
                required.append(field_name)
            defaulted = False
        # SQLx skipped fields are not serialized (or accepted as input).
        for field_name in re.findall(r"#\[sqlx\(skip\)\]\s*pub (\w+)", body):
            properties.pop(field_name, None)
            if field_name in required: required.remove(field_name)
        if properties:
            schema = {"type": "object", "properties": properties}
            if required:
                schema["required"] = required
            # Preserve serde's permissive unknown-field handling unless the
            # struct explicitly opts into deny_unknown_fields.
            if "deny_unknown_fields" in prefix:
                schema["additionalProperties"] = False
            out[name] = schema
    return out


def main():
    spec = yaml.safe_load(SPEC_PATH.read_text())
    schemas = spec["components"]["schemas"]
    generated = {}
    for filename in MODEL_FILES:
        generated.update(structs((ROOT / "src/models" / filename).read_text()))
    generated["HealthResponse"] = {
        "type": "object",
        "required": ["status", "database"],
        "properties": {"status": {"type": "string"}, "database": {"type": "string"}},
    }
    enum_fields = {
        ("StockAdjustmentRequest", "movement_type"): ["adjustment_in", "adjustment_out", "count_adjustment"],
        ("CashMovementRequest", "movement_type"): ["cash_in", "cash_out", "paid_out", "cash_drop"],
        ("ReturnItemRequest", "item_condition"): ["resellable", "damaged", "unsellable"],
        ("SalePaymentRequest", "payment_method"): ["cash", "card", "mobile_money", "other"],
        ("CreateRefundRequest", "payment_method"): ["cash", "card", "mobile_money", "other"],
        ("VoidRefundRequest", "payment_method"): ["cash", "card", "mobile_money", "other"],
    }
    for (model, field), values in enum_fields.items():
        generated[model]["properties"][field]["enum"] = values
    # API model components only. Keep existing administration/report/financial
    # components and their intentional naming unchanged.
    for name, schema in generated.items():
        schemas[name] = schema
    # Serialize only after generation; JSON-compatible YAML is valid OpenAPI.
    SPEC_PATH.write_text(yaml.safe_dump(spec, sort_keys=False, allow_unicode=True, width=110))
    print(f"Synchronized {len(generated)} Rust model schemas")


if __name__ == "__main__":
    main()
