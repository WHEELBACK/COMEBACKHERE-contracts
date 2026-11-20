#!/usr/bin/env bash
set -euo pipefail

python3 - <<'PY'
from pathlib import Path
import re
import sys

roots = [Path("crates"), Path("contracts")]

# Match both plain `pub enum FooError { ... }` declarations and macro-based
# declarations such as `declare_settlement_error_enum! { ... }` or
# `declare_error_enum!(FooError { ... })`. The macro invocations are used by
# modularized error enums (e.g. the oracle-adapter settlement error enum) so
# the uniqueness check must cover them as well.
enum_re = re.compile(
    r"(?:pub\s+enum\s+(?P<plain>[A-Za-z0-9_]*Error)\s*\{"
    r"|(?P<macro>[A-Za-z0-9_]*declare[A-Za-z0-9_]*error[A-Za-z0-9_]*!)\s*[\(\{]\s*"
    r"(?:(?P<macro_name>[A-Za-z0-9_]*Error)\s*)?\{?)"
    r"(?P<body>.*?)\n\s*\}",
    re.S,
)
variant_re = re.compile(r"^\s*([A-Za-z][A-Za-z0-9_]*)\s*=\s*([0-9]+)\s*,", re.M)

failed = False
for root in roots:
    for path in sorted(root.rglob("src/**/*.rs")):
        text = path.read_text()
        for enum_match in enum_re.finditer(text):
            enum_name = (
                enum_match.group("plain")
                or enum_match.group("macro_name")
                or enum_match.group("macro")
                or "<anonymous>"
            )
            seen = {}
            for variant, code in variant_re.findall(enum_match.group("body")):
                if code in seen:
                    print(
                        f"{path}: duplicate {enum_name} discriminant {code}: "
                        f"{seen[code]} and {variant}",
                        file=sys.stderr,
                    )
                    failed = True
                else:
                    seen[code] = variant

if failed:
    sys.exit(1)
PY