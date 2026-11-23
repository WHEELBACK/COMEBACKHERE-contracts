#!/usr/bin/env bash
set -euo pipefail

python3 - <<'PY'
from pathlib import Path
import re
import sys

roots = [Path("crates"), Path("contracts")]
enum_re = re.compile(r"pub\s+enum\s+([A-Za-z0-9_]*Error)\s*\{(?P<body>.*?)\n\}", re.S)
variant_re = re.compile(r"^\s*([A-Za-z][A-Za-z0-9_]*)\s*=\s*([0-9]+)\s*,", re.M)

# Error crates introduced by the modularization work (#955, #964, #973).
# Each entry maps the crate directory to the error enum it must define.
required_error_crates = {
    Path("crates/event-emitter-errors"): "EventEmitterError",
    Path("crates/storage-layout-errors"): "StorageLayoutError",
    Path("crates/access-control-errors"): "AccessControlError",
}

failed = False

# Track discriminants per enum name across all scanned files so that
# collisions between the new modularized crates are detected too.
global_seen = {}

scanned_files = []
for root in roots:
    if not root.exists():
        continue
    for path in sorted(root.rglob("src/**/*.rs")):
        scanned_files.append(path)
        text = path.read_text()
        for enum_match in enum_re.finditer(text):
            enum_name = enum_match.group(1)
            seen = global_seen.setdefault(enum_name, {})
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

# Assert each required error crate exists, is scanned, and defines its enum.
for crate_dir, enum_name in required_error_crates.items():
    if not crate_dir.exists():
        print(
            f"missing required error crate: {crate_dir} (expected enum {enum_name})",
            file=sys.stderr,
        )
        failed = True
        continue

    crate_files = [p for p in scanned_files if crate_dir in p.parents]
    if not crate_files:
        print(
            f"error crate {crate_dir} was not scanned for error enums",
            file=sys.stderr,
        )
        failed = True
        continue

    if enum_name not in global_seen:
        print(
            f"error crate {crate_dir} does not define expected enum {enum_name}",
            file=sys.stderr,
        )
        failed = True

# Fail loudly if a new error crate is added without being registered above.
for root in roots:
    if not root.exists():
        continue
    for crate_dir in sorted(p for p in root.iterdir() if p.is_dir()):
        if crate_dir.name.endswith("-errors") and crate_dir not in required_error_crates:
            print(
                f"unregistered error crate: {crate_dir}; add it to "
                f"required_error_crates in scripts/check-error-code-uniqueness.sh",
                file=sys.stderr,
            )
            failed = True

if failed:
    sys.exit(1)
PY