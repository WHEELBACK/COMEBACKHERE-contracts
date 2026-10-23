#!/usr/bin/env bash
set -euo pipefail

# check-mod-files.sh — verify that every `mod name;` declaration in the
# workspace has a corresponding `name.rs` or `name/mod.rs` file.
#
# Exits with code 0 if all module files are present, 1 if any are missing.

failures=0
checks=0

while IFS= read -r rs_file; do
    dir="$(dirname "$rs_file")"

    # Find all `mod name;` declarations (not inline `mod name { }` blocks)
    while IFS= read -r line; do
        # Extract the module name from the declaration
        mod_name="$(echo "$line" | sed -E 's/^[[:space:]]*mod ([a-z_][a-z0-9_]*);.*/\1/')"

        checks=$((checks + 1))

        mod_file="$dir/${mod_name}.rs"
        mod_dir_file="$dir/${mod_name}/mod.rs"

        if [ -f "$mod_file" ] || [ -f "$mod_dir_file" ]; then
            echo "PASS: $rs_file — mod ${mod_name};"
        else
            echo "FAIL: $rs_file — mod ${mod_name}; (neither ${mod_file} nor ${mod_dir_file} exists)"
            failures=$((failures + 1))
        fi
    done < <(grep -E '^[[:space:]]*mod [a-z_][a-z0-9_]*;' "$rs_file" || true)
done < <(find . -name '*.rs' \
    -not -path './target/*' \
    -not -path '*/fuzz/Cargo.lock')

echo ""
echo "──────────────────────────────────────────"
echo "Summary: checked ${checks} mod declaration(s), ${failures} missing file(s)."

if [ "$failures" -gt 0 ]; then
    echo "FAIL: ${failures} mod declaration(s) have no corresponding source file."
    exit 1
else
    echo "PASS: all mod declarations have corresponding source files."
    exit 0
fi
