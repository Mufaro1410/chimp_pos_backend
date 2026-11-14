#!/usr/bin/env python3
"""Validate Axum route coverage and key OpenAPI/Rust contract metadata.

Requires PyYAML (`python3 -m pip install PyYAML`). The checker compares registered
routes, authentication layers, permission middleware mappings, local OpenAPI
references, and DTO fields/types/requiredness for the configured Rust model files.
It does not prove that every operation references the exact handler response DTO
or that all documented status codes and error bodies match runtime behaviour.
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

import yaml

ROOT = Path(__file__).resolve().parents[1]
ROUTES = ROOT / "src/routes/mod.rs"
SPEC = ROOT / "docs/openapi.yaml"
PERMISSIONS = ROOT / "src/utils/permission_middleware.rs"
HTTP_METHODS = {"get", "post", "put", "patch", "delete", "options", "head"}
BODYLESS = {
    ("DELETE", "/api/v1/products/{id}"),
    ("DELETE", "/api/v1/products/{product_id}/variants/{variant_id}"),
    ("POST", "/api/v1/purchases/{id}/submit"),
    ("POST", "/api/v1/purchases/{id}/cancel"),
    ("POST", "/api/v1/sales/{id}/cancel"),
}


class UniqueKeyLoader(yaml.SafeLoader):
    pass


def construct_mapping(loader, node, deep=False):
    mapping = {}
    for key_node, value_node in node.value:
        key = loader.construct_object(key_node, deep=deep)
        if key in mapping:
            raise ValueError(f"duplicate YAML key: {key!r}")
        mapping[key] = loader.construct_object(value_node, deep=deep)
    return mapping


UniqueKeyLoader.add_constructor(yaml.resolver.BaseResolver.DEFAULT_MAPPING_TAG, construct_mapping)


def route_operations(source: str) -> set[tuple[str, str]]:
    operations: set[tuple[str, str]] = set()
    for match in re.finditer(r"\.route\s*\(\s*\"([^\"]+)\"\s*,", source):
        start = match.end()
        depth, quoted, escaped = 1, False, False
        i = start
        while i < len(source) and depth:
            char = source[i]
            if quoted:
                if escaped:
                    escaped = False
                elif char == "\\":
                    escaped = True
                elif char == '"':
                    quoted = False
            elif char == '"':
                quoted = True
            elif char == "(":
                depth += 1
            elif char == ")":
                depth -= 1
            i += 1
        path = match.group(1)
        expression = source[start : i - 1]
        for method in re.findall(r"\b(get|post|put|patch|delete|options|head)\s*\(", expression):
            operations.add((method.upper(), path))
    return operations


def matching_brace(source: str, opening: int) -> int:
    depth, quoted, escaped = 0, False, False
    for i in range(opening, len(source)):
        char = source[i]
        if quoted:
            if escaped:
                escaped = False
            elif char == "\\":
                escaped = True
            elif char == '"':
                quoted = False
        elif char == '"':
            quoted = True
        elif char == "{":
            depth += 1
        elif char == "}":
            depth -= 1
            if depth == 0:
                return i
    raise ValueError("unbalanced braces in Rust source")


def top_level_arms(body: str) -> list[str]:
    arms, start = [], 0
    braces = parens = brackets = 0
    quoted = escaped = False
    for i, char in enumerate(body):
        if quoted:
            if escaped:
                escaped = False
            elif char == "\\":
                escaped = True
            elif char == '"':
                quoted = False
            continue
        if char == '"':
            quoted = True
        elif char == "{": braces += 1
        elif char == "}": braces -= 1
        elif char == "(" and body.startswith("(&Method::", i) and braces == parens == brackets == 0 and i > start:
            if body[start:i].strip():
                arms.append(body[start:i].strip())
            start = i
            parens += 1
        elif char == "(": parens += 1
        elif char == ")": parens -= 1
        elif char == "[": brackets += 1
        elif char == "]": brackets -= 1
        elif char == "," and braces == parens == brackets == 0:
            arms.append(body[start:i].strip())
            start = i + 1
    if body[start:].strip():
        arms.append(body[start:].strip())
    return arms


def rust_condition(condition: str, path: str) -> bool:
    condition = " ".join(condition.split())
    condition = re.sub(r"p\.split\('/'\)\.count\(\)", f"len({path!r}.split('/'))", condition)
    condition = re.sub(r'p\.starts_with\("([^"]*)"\)', lambda m: f"{path!r}.startswith({m.group(1)!r})", condition)
    condition = re.sub(r'p\.ends_with\("([^"]*)"\)', lambda m: f"{path!r}.endswith({m.group(1)!r})", condition)
    condition = re.sub(r"!(?=\s*['\"])", "not ", condition)
    condition = condition.replace("&&", " and ").replace("||", " or ")
    if re.search(r"[^\w\s()'\".=<>/,{}-]", condition):
        raise ValueError(f"unsupported permission guard: {condition}")
    return bool(eval(condition, {"__builtins__": {}}, {"len": len}))


def permission_body(body: str, path: str) -> str | None:
    direct = re.fullmatch(r'\s*Some\("([^"]+)"\)\s*', body)
    if direct:
        return direct.group(1)
    # Permission middleware has one nested conditional form for purchase verbs.
    tests = list(re.finditer(r'if\s+p\.(ends_with|starts_with)\("([^"]+)"\)\s*\{\s*Some\("([^"]+)"\)\s*\}', body, re.S))
    for test in tests:
        predicate, suffix, permission = test.groups()
        if (path.endswith(suffix) if predicate == "ends_with" else path.startswith(suffix)):
            return permission
    # An unconditional Some in the final else is the default branch.
    all_matches = list(re.finditer(r'Some\("([^"]+)"\)', body))
    if all_matches and not re.search(r"\belse\s*\{\s*None", body):
        return all_matches[-1].group(1)
    return None


def middleware_permissions(source: str, operations: set[tuple[str, str]]) -> dict[tuple[str, str], str | None]:
    start = source.index("fn required_permission(")
    match_start = source.index("match (method, path) {", start)
    opening = source.index("{", match_start)
    body = source[opening + 1 : matching_brace(source, opening)]
    arms = top_level_arms(body)
    result = {}
    for method, path in operations:
        permission = None
        for arm in arms:
            split = arm.find("=>")
            if split < 0:
                continue
            pattern = re.sub(r"(?m)//.*$", "", arm[:split]).strip()
            rhs = arm[split + 2 :].strip()
            head = re.match(r"\(&Method::(\w+),\s*(?:\"([^\"]+)\"|(p))\)\s*(?:if\s*(.*))?$", pattern, re.S)
            if not head or head.group(1) != method:
                continue
            literal, variable, guard = head.group(2), head.group(3), head.group(4)
            if literal is not None and literal != path:
                continue
            if variable and guard and not rust_condition(guard, path):
                continue
            permission = permission_body(rhs, path)
            if permission is not None:
                break
        result[(method, path)] = permission
    return result


def authentication_configuration(route_source: str) -> tuple[set[tuple[str, str]], set[tuple[str, str]]]:
    protected, auth_only = set(), set()
    starts = list(re.finditer(r"\blet\s+(\w+)\s*=\s*Router::new\(\)", route_source))
    for i, item in enumerate(starts):
        end = starts[i + 1].start() if i + 1 < len(starts) else route_source.index("\n    Router::new()", item.end())
        block = route_source[item.start():end]
        if "auth_middleware" in block:
            routes = route_operations(block)
            protected.update(routes)
            if "permission_middleware" not in block:
                auth_only.update(routes)
    return protected, auth_only


def local_refs(value, path=""):
    if isinstance(value, dict):
        ref = value.get("$ref")
        if ref and ref.startswith("#/"):
            yield path, ref
        for key, child in value.items():
            yield from local_refs(child, f"{path}/{key}")
    elif isinstance(value, list):
        for index, child in enumerate(value):
            yield from local_refs(child, f"{path}/{index}")


def resolve_pointer(document, pointer):
    current = document
    for token in pointer[2:].split("/"):
        token = token.replace("~1", "/").replace("~0", "~")
        current = current[token]
    return current


def main() -> int:
    spec = yaml.load(SPEC.read_text(), Loader=UniqueKeyLoader)
    paths = spec.get("paths", {})
    documented = {
        (method.upper(), path)
        for path, item in paths.items()
        for method in item
        if method.lower() in HTTP_METHODS
    }
    registered = route_operations(ROUTES.read_text())
    rust_permissions = middleware_permissions(PERMISSIONS.read_text(), registered)
    protected, auth_only = authentication_configuration(ROUTES.read_text())
    failures: list[str] = []
    for method, path in sorted(registered - documented):
        failures.append(f"undocumented route: {method} {path}")
    for method, path in sorted(documented - registered):
        failures.append(f"documented but unregistered: {method} {path}")
    for method, path in sorted(registered & documented):
        operation = paths[path][method.lower()]
        if (method, path) in protected:
            if not operation.get("security"):
                failures.append(f"missing authentication requirement: {method} {path}")
        else:
            if operation.get("security"):
                failures.append(f"public route must not declare bearer auth: {method} {path}")
        actual_permission = rust_permissions.get((method, path))
        documented_permission = operation.get("x-required-permission")
        if actual_permission is None:
            if documented_permission:
                failures.append(f"unexpected permission metadata (middleware has no permission): {method} {path}")
            if (method, path) not in auth_only and (method, path) in protected:
                failures.append(f"protected route has no permission mapping: {method} {path}")
        elif documented_permission != actual_permission:
            failures.append(f"permission mismatch: {method} {path}: OpenAPI={documented_permission!r}, Rust={actual_permission!r}")

        request_body = operation.get("requestBody")
        if method in {"POST", "PUT", "PATCH"} and (method, path) not in BODYLESS and not request_body:
            failures.append(f"missing request body definition: {method} {path}")
        if request_body and not any("schema" in media for media in request_body.get("content", {}).values()):
            failures.append(f"request body has no schema: {method} {path}")
        for code, response in operation.get("responses", {}).items():
            if code.startswith("2") and code != "204" and "$ref" not in response:
                if not any("schema" in media for media in response.get("content", {}).values()):
                    failures.append(f"success response {code} has no schema: {method} {path}")

    for location, ref in local_refs(spec):
        try:
            resolve_pointer(spec, ref)
        except (KeyError, IndexError, TypeError):
            failures.append(f"invalid OpenAPI reference {ref!r} at {location}")

    try:
        from sync_openapi_rust_models import MODEL_FILES, structs

        for filename in MODEL_FILES:
            models = structs((ROOT / "src/models" / filename).read_text())
            for name, expected_schema in models.items():
                schema = spec["components"]["schemas"].get(name)
                if not schema:
                    failures.append(f"missing schema for Rust model {name}")
                    continue
                expected = expected_schema.get("properties", {})
                actual = schema.get("properties", {})
                if set(expected) != set(actual):
                    failures.append(f"Rust/OpenAPI fields differ for {name}: missing={sorted(set(expected)-set(actual))}, extra={sorted(set(actual)-set(expected))}")
                for field, expected_type in expected.items():
                    actual_type = actual.get(field)
                    if actual_type != expected_type:
                        # Service-validated enum values and documentary schema
                        # constraints extend the DTO type but may not change it.
                        actual_base = {k: v for k, v in (actual_type or {}).items() if k not in {"enum", "description", "example", "examples"}}
                        if actual_base != expected_type:
                            failures.append(f"Rust/OpenAPI type differs for {name}.{field}: expected={expected_type!r}, actual={actual_type!r}")
                expected_required = set(expected_schema.get("required", []))
                actual_required = set(schema.get("required", []))
                if expected_required != actual_required:
                    failures.append(f"Rust/OpenAPI required fields differ for {name}: expected={sorted(expected_required)}, actual={sorted(actual_required)}")
    except (ImportError, KeyError, OSError) as error:
        failures.append(f"could not compare schemas with Rust models: {error}")
    if failures:
        print("OpenAPI validation failed:")
        print("\n".join(f"- {failure}" for failure in failures))
        return 1
    print(f"OpenAPI contract valid for {len(registered)} routes: route/auth/permission/schema/reference checks passed.")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (OSError, yaml.YAMLError, ValueError) as error:
        print(f"OpenAPI validation failed: {error}", file=sys.stderr)
        raise SystemExit(1)
