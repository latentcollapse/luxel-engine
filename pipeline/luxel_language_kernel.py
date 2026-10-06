"""Safe parse-only frontend kernel for the frozen Luxel 0.6 core contract.

This module deliberately stops before type checking and semantic IR emission.
It never compiles or executes Python.  Python's parser is used only as a
concrete-syntax decoder for the closed Luxel source subset.
"""

from __future__ import annotations

import ast
import hashlib
import io
import keyword
import math
import re
import tokenize
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Iterable


SOURCE_SCHEMA = "luxel.frontend.parsed-module/0.6"
MAX_EXACT_INT = 2**53 - 1
ASCII_BINDING = re.compile(r"[A-Za-z_][A-Za-z0-9_]*\Z")
SEED_PATTERN = re.compile(r"seed256:[0-9a-f]{64}\Z")
DIGEST_PATTERN = re.compile(r"sha256:[0-9a-f]{64}\Z")
STANDARD_NAMESPACES = frozenset(
    {"luxel.core", "luxel.geometry", "luxel.units", "luxel.asset", "luxel.world"}
)


class FrontendError(Exception):
    """Stable source diagnostic raised before semantic compilation."""

    def __init__(
        self,
        code: str,
        detail: str,
        node: ast.AST | None = None,
        *,
        line: int | None = None,
        column: int | None = None,
    ) -> None:
        self.code = code
        self.detail = detail
        self.line = line if line is not None else getattr(node, "lineno", None)
        self.column = column if column is not None else getattr(node, "col_offset", None)
        location = "" if self.line is None else f" at {self.line}:{(self.column or 0) + 1}"
        super().__init__(f"{code}{location}: {detail}")


@dataclass(frozen=True)
class RegistrySymbolPin:
    namespace: str
    symbol: str
    module_id: str
    semantic_digest: str
    kind: str = "value"
    positional_identity_slots: int = 0

    def __post_init__(self) -> None:
        _binding(self.symbol, "registry symbol")
        if self.namespace not in STANDARD_NAMESPACES:
            raise ValueError(f"unregistered Luxel namespace: {self.namespace!r}")
        if not DIGEST_PATTERN.fullmatch(self.semantic_digest):
            raise ValueError("registry symbol semantic_digest must be sha256:<64 lowercase hex>")
        if self.kind not in {"constructor", "type", "unit", "value"}:
            raise ValueError(f"unsupported registry symbol kind: {self.kind!r}")
        if self.positional_identity_slots not in (0, 1):
            raise ValueError("language 1.0 permits at most one positional identity slot")
        if self.kind != "constructor" and self.positional_identity_slots:
            raise ValueError("only constructors may declare a positional identity slot")


@dataclass(frozen=True)
class BuildManifest:
    package_id: str
    root_module_id: str
    root_source_digest: str
    package_seed: str
    target: str
    product_profile_id: str
    product_profile_digest: str
    registry_digest: str
    imports: tuple[RegistrySymbolPin, ...]
    request_module_seed: str | None = None
    max_source_bytes: int = 1_000_000
    max_ast_nodes: int = 100_000
    max_collection_items: int = 100_000

    def __post_init__(self) -> None:
        for field_name in ("root_source_digest", "product_profile_digest", "registry_digest"):
            if not DIGEST_PATTERN.fullmatch(getattr(self, field_name)):
                raise ValueError(f"{field_name} must be sha256:<64 lowercase hex>")
        if not SEED_PATTERN.fullmatch(self.package_seed):
            raise ValueError("package_seed must use seed256:<64 lowercase hex>")
        if self.request_module_seed is not None and not SEED_PATTERN.fullmatch(self.request_module_seed):
            raise ValueError("request_module_seed must use seed256:<64 lowercase hex>")
        if self.max_source_bytes <= 0 or self.max_ast_nodes <= 0 or self.max_collection_items <= 0:
            raise ValueError("frontend resource ceilings must be positive")
        keys = [(pin.namespace, pin.symbol) for pin in self.imports]
        if len(keys) != len(set(keys)):
            raise ValueError("manifest registry imports must be unique")


@dataclass(frozen=True)
class SourceSpan:
    line: int
    column: int
    end_line: int
    end_column: int


@dataclass(frozen=True)
class ParsedImport:
    namespace: str
    symbol: str
    module_id: str
    semantic_digest: str
    span: SourceSpan


@dataclass(frozen=True)
class ParsedDeclaration:
    name: str
    expression: dict[str, Any]
    span: SourceSpan


@dataclass(frozen=True)
class SeedResolution:
    kind: str
    value: str | None
    package_seed: str | None = None
    module_id: str | None = None


@dataclass(frozen=True)
class ParsedModule:
    schema: str
    package_id: str
    module_id: str
    source_digest: str
    target: str
    product_profile_id: str
    product_profile_digest: str
    registry_digest: str
    imports: tuple[ParsedImport, ...]
    module_seed: SeedResolution
    declarations: tuple[ParsedDeclaration, ...]
    expression_statements: tuple[dict[str, Any], ...]


def source_digest(source: str) -> str:
    return "sha256:" + hashlib.sha256(source.encode("utf-8")).hexdigest()


def _binding(value: str, context: str) -> str:
    if not ASCII_BINDING.fullmatch(value) or keyword.iskeyword(value):
        raise ValueError(f"{context} must be an ASCII non-keyword binding: {value!r}")
    return value


def _span(node: ast.AST) -> SourceSpan:
    return SourceSpan(
        line=getattr(node, "lineno", 1),
        column=getattr(node, "col_offset", 0),
        end_line=getattr(node, "end_lineno", getattr(node, "lineno", 1)),
        end_column=getattr(node, "end_col_offset", getattr(node, "col_offset", 0)),
    )


def _raw_lexical_checks(source: str) -> None:
    for line_number, line in enumerate(source.splitlines(), 1):
        indentation = line[: len(line) - len(line.lstrip(" \t"))]
        if "\t" in indentation:
            raise FrontendError("tab_indentation", "tabs are forbidden in indentation", line=line_number, column=0)
    try:
        tokens = tokenize.generate_tokens(io.StringIO(source).readline)
        for token in tokens:
            if token.type == tokenize.NAME and not token.string.isascii():
                raise FrontendError(
                    "non_ascii_source_name",
                    f"source names must be ASCII: {token.string!r}",
                    line=token.start[0],
                    column=token.start[1],
                )
    except (tokenize.TokenError, IndentationError) as error:
        raise FrontendError("invalid_syntax", str(error)) from error


class _ExpressionParser:
    def __init__(
        self,
        bindings: set[str],
        imported: dict[str, RegistrySymbolPin],
        max_collection_items: int,
    ) -> None:
        self.bindings = bindings
        self.imported = imported
        self.max_collection_items = max_collection_items

    def parse(self, node: ast.AST) -> dict[str, Any]:
        method = getattr(self, f"parse_{type(node).__name__}", None)
        if method is None:
            raise FrontendError("unsupported_syntax", f"{type(node).__name__} is outside the Luxel subset", node)
        return method(node)

    def parse_Constant(self, node: ast.Constant) -> dict[str, Any]:
        value = node.value
        if value is None or isinstance(value, (str, bool)):
            return {"kind": "literal", "value": value}
        if isinstance(value, int):
            if abs(value) > MAX_EXACT_INT:
                raise FrontendError("integer_out_of_range", "integer exceeds the interoperable 53-bit range", node)
            return {"kind": "literal", "value": value}
        if isinstance(value, float):
            if not math.isfinite(value):
                raise FrontendError("non_finite_number", "NaN and infinity are forbidden", node)
            return {"kind": "literal", "value": value}
        raise FrontendError("unsupported_literal", f"literal type {type(value).__name__} is forbidden", node)

    def parse_Name(self, node: ast.Name) -> dict[str, Any]:
        if node.id == "module_seed":
            raise FrontendError("reserved_seed_reference", "module_seed is metadata, not a value binding", node)
        if node.id not in self.bindings:
            raise FrontendError("unbound_name", f"{node.id!r} is not an imported or prior immutable binding", node)
        return {"kind": "reference", "name": node.id}

    def parse_Attribute(self, node: ast.Attribute) -> dict[str, Any]:
        if not ASCII_BINDING.fullmatch(node.attr) or keyword.iskeyword(node.attr):
            raise FrontendError("invalid_attribute", f"invalid registry attribute {node.attr!r}", node)
        return {"kind": "attribute", "value": self.parse(node.value), "name": node.attr}

    def parse_List(self, node: ast.List) -> dict[str, Any]:
        if len(node.elts) > self.max_collection_items:
            raise FrontendError("collection_limit", "list exceeds the frontend item ceiling", node)
        return {"kind": "list", "items": [self.parse(item) for item in node.elts]}

    def parse_Tuple(self, node: ast.Tuple) -> dict[str, Any]:
        raise FrontendError("tuple_literal_forbidden", "use a list literal for Luxel sequences", node)

    def parse_Dict(self, node: ast.Dict) -> dict[str, Any]:
        if len(node.keys) > self.max_collection_items:
            raise FrontendError("collection_limit", "record exceeds the frontend item ceiling", node)
        seen: set[str] = set()
        fields = []
        for key_node, value_node in zip(node.keys, node.values, strict=True):
            if key_node is None:
                raise FrontendError("record_unpacking", "record unpacking is forbidden", node)
            if not isinstance(key_node, ast.Constant) or not isinstance(key_node.value, str):
                raise FrontendError("record_key_type", "record keys must be string literals", key_node)
            key = key_node.value
            if key in seen:
                raise FrontendError("duplicate_record_key", f"duplicate decoded record key {key!r}", key_node)
            seen.add(key)
            fields.append({"key": key, "value": self.parse(value_node)})
        return {"kind": "record", "fields": fields}

    def parse_UnaryOp(self, node: ast.UnaryOp) -> dict[str, Any]:
        if not isinstance(node.op, ast.USub):
            code = "unsupported_boolean_operator" if isinstance(node.op, ast.Not) else "unsupported_operator"
            raise FrontendError(code, "only unary numeric negation is supported", node)
        operand = self.parse(node.operand)
        if operand["kind"] == "literal" and isinstance(operand["value"], int):
            value = -operand["value"]
            if abs(value) > MAX_EXACT_INT:
                raise FrontendError("integer_out_of_range", "integer exceeds the interoperable 53-bit range", node)
            return {"kind": "literal", "value": value}
        return {"kind": "unary", "operator": "negate", "operand": operand}

    def parse_BinOp(self, node: ast.BinOp) -> dict[str, Any]:
        operators = {ast.Add: "add", ast.Sub: "subtract", ast.Mult: "multiply", ast.Div: "divide"}
        operator = operators.get(type(node.op))
        if operator is None:
            code = "unsupported_boolean_operator" if isinstance(node.op, ast.BitXor) else "unsupported_operator"
            raise FrontendError(code, f"{type(node.op).__name__} is outside the Luxel arithmetic subset", node)
        return {"kind": "binary", "operator": operator, "left": self.parse(node.left), "right": self.parse(node.right)}

    def parse_BoolOp(self, node: ast.BoolOp) -> dict[str, Any]:
        raise FrontendError("unsupported_boolean_operator", "use registered mask constructors", node)

    def parse_Compare(self, node: ast.Compare) -> dict[str, Any]:
        if len(node.ops) != 1 or len(node.comparators) != 1:
            raise FrontendError("chained_comparison", "comparison chains are forbidden", node)
        names = {
            ast.Lt: "lt", ast.LtE: "le", ast.Eq: "eq",
            ast.NotEq: "ne", ast.GtE: "ge", ast.Gt: "gt",
        }
        operator = names.get(type(node.ops[0]))
        if operator is None:
            raise FrontendError("unsupported_comparison", "comparison operator is outside the Luxel subset", node)
        return {"kind": "comparison", "operator": operator, "left": self.parse(node.left), "right": self.parse(node.comparators[0])}

    def parse_Call(self, node: ast.Call) -> dict[str, Any]:
        if not isinstance(node.func, ast.Name):
            raise FrontendError("unregistered_call", "constructors must be directly imported registry names", node.func)
        pin = self.imported.get(node.func.id)
        if pin is None or pin.kind != "constructor":
            raise FrontendError("unregistered_call", f"{node.func.id!r} is not an imported constructor", node.func)
        if len(node.args) > pin.positional_identity_slots:
            raise FrontendError("unexpected_positional_operand", f"{node.func.id!r} operands are keyword-only", node)
        positional = [self.parse(argument) for argument in node.args]
        keywords = []
        seen: set[str] = set()
        for argument in node.keywords:
            if argument.arg is None:
                raise FrontendError("keyword_unpacking", "**kwargs unpacking is forbidden", argument.value)
            if argument.arg in seen:
                raise FrontendError("duplicate_argument", f"duplicate argument {argument.arg!r}", argument.value)
            seen.add(argument.arg)
            keywords.append({"name": argument.arg, "value": self.parse(argument.value)})
        return {
            "kind": "call",
            "constructor": node.func.id,
            "registry_identity": {
                "module_id": pin.module_id,
                "symbol": pin.symbol,
                "semantic_digest": pin.semantic_digest,
            },
            "positional_identity": positional,
            "arguments": keywords,
        }


def _resolve_seed(source_seed: str | None, manifest: BuildManifest) -> SeedResolution:
    request_seed = manifest.request_module_seed
    if source_seed is not None and request_seed is not None and source_seed != request_seed:
        raise FrontendError("seed_conflict", "source and build-request module seeds differ")
    if source_seed is not None:
        return SeedResolution(kind="explicit_source", value=source_seed)
    if request_seed is not None:
        return SeedResolution(kind="build_request_module_seed", value=request_seed)
    return SeedResolution(
        kind="derive_from_package",
        value=None,
        package_seed=manifest.package_seed,
        module_id=manifest.root_module_id,
    )


def _parse_seed_assignment(statement: ast.Assign) -> str:
    if len(statement.targets) != 1 or not isinstance(statement.targets[0], ast.Name):
        raise FrontendError("invalid_seed_assignment", "module_seed requires one name target", statement)
    call = statement.value
    if not isinstance(call, ast.Call) or not isinstance(call.func, ast.Name) or call.func.id != "seed":
        raise FrontendError("invalid_seed_assignment", "module_seed must call seed(value=...)", statement.value)
    if call.args or len(call.keywords) != 1 or call.keywords[0].arg != "value":
        raise FrontendError("invalid_seed_assignment", "module_seed requires exactly seed(value=...)", call)
    value_node = call.keywords[0].value
    if not isinstance(value_node, ast.Constant) or not isinstance(value_node.value, str):
        raise FrontendError("invalid_seed_value", "seed value must be a literal string", value_node)
    if not SEED_PATTERN.fullmatch(value_node.value):
        raise FrontendError("invalid_seed_value", "seed must use seed256:<64 lowercase hex>", value_node)
    return value_node.value


def parse_module(source: str, manifest: BuildManifest) -> ParsedModule:
    """Parse and validate one authored Luxel module without executing it."""
    encoded = source.encode("utf-8")
    if len(encoded) > manifest.max_source_bytes:
        raise FrontendError("source_limit", "source exceeds the manifest byte ceiling")
    actual_digest = source_digest(source)
    if actual_digest != manifest.root_source_digest:
        raise FrontendError("source_digest_mismatch", "source bytes do not match the build-request manifest")
    _raw_lexical_checks(source)
    try:
        tree = ast.parse(source, mode="exec", type_comments=False)
    except SyntaxError as error:
        raise FrontendError(
            "invalid_syntax", error.msg, line=error.lineno, column=(error.offset or 1) - 1
        ) from error
    if sum(1 for _ in ast.walk(tree)) > manifest.max_ast_nodes:
        raise FrontendError("ast_limit", "source exceeds the manifest AST-node ceiling")

    # Metadata placement is a module-structure rule, so diagnose it before an
    # earlier ordinary declaration's expression can mask the corrective error.
    seed_statements = [
        (index, statement)
        for index, statement in enumerate(tree.body)
        if isinstance(statement, ast.Assign)
        and any(isinstance(target, ast.Name) and target.id == "module_seed" for target in statement.targets)
    ]
    if len(seed_statements) > 1:
        raise FrontendError("duplicate_module_seed", "module_seed may appear at most once", seed_statements[1][1])
    if seed_statements:
        seed_index, seed_statement = seed_statements[0]
        if any(not isinstance(statement, ast.ImportFrom) for statement in tree.body[:seed_index]):
            raise FrontendError("late_module_seed", "module_seed must precede every declaration", seed_statement)

    authorized = {(pin.namespace, pin.symbol): pin for pin in manifest.imports}
    imported: dict[str, RegistrySymbolPin] = {}
    parsed_imports: list[ParsedImport] = []
    bindings: set[str] = set()
    declarations: list[ParsedDeclaration] = []
    expression_statements: list[dict[str, Any]] = []
    source_seed: str | None = None
    phase = "imports"

    for statement in tree.body:
        if isinstance(statement, ast.ImportFrom):
            if phase != "imports":
                raise FrontendError("late_import", "imports must precede module_seed and declarations", statement)
            if statement.level or statement.module not in STANDARD_NAMESPACES:
                raise FrontendError("unauthorized_import", "only standard compiler-owned Luxel namespaces are legal", statement)
            for alias in statement.names:
                if alias.name == "*" or alias.asname is not None:
                    raise FrontendError("import_alias_forbidden", "wildcard imports and aliases are forbidden", statement)
                if alias.name == "module_seed":
                    raise FrontendError("reserved_seed_binding", "module_seed cannot be imported", statement)
                pin = authorized.get((statement.module, alias.name))
                if pin is None:
                    raise FrontendError("undeclared_registry_export", f"{statement.module}.{alias.name} is not authorized", statement)
                previous = imported.get(alias.name)
                if previous is not None and previous != pin:
                    raise FrontendError("import_collision", f"{alias.name!r} resolves to different registry symbols", statement)
                if previous is None:
                    imported[alias.name] = pin
                    bindings.add(alias.name)
                    parsed_imports.append(
                        ParsedImport(statement.module, alias.name, pin.module_id, pin.semantic_digest, _span(statement))
                    )
            continue

        if isinstance(statement, ast.Assign) and any(
            isinstance(target, ast.Name) and target.id == "module_seed" for target in statement.targets
        ):
            if phase == "declarations":
                raise FrontendError("late_module_seed", "module_seed must precede every declaration", statement)
            if source_seed is not None:
                raise FrontendError("duplicate_module_seed", "module_seed may appear at most once", statement)
            source_seed = _parse_seed_assignment(statement)
            phase = "seed"
            continue

        phase = "declarations"
        parser = _ExpressionParser(bindings, imported, manifest.max_collection_items)
        if isinstance(statement, ast.Assign):
            if len(statement.targets) != 1 or not isinstance(statement.targets[0], ast.Name):
                raise FrontendError("invalid_assignment", "declarations require one name target", statement)
            name = statement.targets[0].id
            if name == "module_seed":
                raise FrontendError("invalid_seed_assignment", "invalid module_seed metadata assignment", statement)
            try:
                _binding(name, "declaration")
            except ValueError as error:
                raise FrontendError("invalid_binding", str(error), statement.targets[0]) from error
            if name in bindings:
                raise FrontendError("binding_redefinition", f"{name!r} is already imported or declared", statement.targets[0])
            expression = parser.parse(statement.value)
            declarations.append(ParsedDeclaration(name=name, expression=expression, span=_span(statement)))
            bindings.add(name)
        elif isinstance(statement, ast.Expr):
            if not isinstance(statement.value, ast.Call):
                raise FrontendError("invalid_expression_statement", "only registered declaration calls may stand alone", statement)
            expression_statements.append(parser.parse(statement.value))
        else:
            raise FrontendError("unsupported_statement", f"{type(statement).__name__} is outside the Luxel subset", statement)

    return ParsedModule(
        schema=SOURCE_SCHEMA,
        package_id=manifest.package_id,
        module_id=manifest.root_module_id,
        source_digest=actual_digest,
        target=manifest.target,
        product_profile_id=manifest.product_profile_id,
        product_profile_digest=manifest.product_profile_digest,
        registry_digest=manifest.registry_digest,
        imports=tuple(parsed_imports),
        module_seed=_resolve_seed(source_seed, manifest),
        declarations=tuple(declarations),
        expression_statements=tuple(expression_statements),
    )


def parse_module_file(path: str | Path, manifest: BuildManifest) -> ParsedModule:
    """Read one canonical ``.luxel`` source file and pass it to :func:`parse_module`."""
    source_path = Path(path)
    if source_path.suffix != ".luxel":
        raise FrontendError("wrong_source_extension", "canonical Luxel source must use the .luxel extension")
    if not source_path.is_file():
        raise FrontendError("missing_source", f"source file does not exist: {source_path}")
    if source_path.stat().st_size > manifest.max_source_bytes:
        raise FrontendError("source_limit", "source exceeds the manifest byte ceiling")
    try:
        source = source_path.read_text(encoding="utf-8")
    except UnicodeDecodeError as error:
        raise FrontendError("source_encoding", "Luxel source must be UTF-8") from error
    return parse_module(source, manifest)


def constructor_pins(pins: Iterable[RegistrySymbolPin]) -> tuple[RegistrySymbolPin, ...]:
    """Small helper for callers assembling immutable manifest import tuples."""
    return tuple(pins)
