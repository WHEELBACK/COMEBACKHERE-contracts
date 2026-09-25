#!/usr/bin/env bash
set -euo pipefail

python3 - <<'PY'
from pathlib import Path
import re
import sys

roots = [Path("crates"), Path("contracts")]
enum_re = re.compile(r"pub\s+enum\s+([A-Za-z0-9_]*Error)\s*\{(?P<body>.*?)\n\}", re.S)
variant_re = re.compile(r"^\s*([A-Za-z][A-Za-z0-9_]*)\s*=\s*([0-9]+)\s*,", re.M)

failed = False
for root in roots:
    for path in sorted(root.rglob("src/**/*.rs")):
        text = path.read_text()
        for enum_match in enum_re.finditer(text):
            enum_name = enum_match.group(1)
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
