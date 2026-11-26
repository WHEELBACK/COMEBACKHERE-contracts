#!/usr/bin/env bash
set -euo pipefail

# check-mod-files.sh — verify that every `mod name;` declaration in the
# workspace has a corresponding `name.rs`, `name/mod.rs`, or `#[path = "..."]` file.
#
# Exits with code 0 if all module files are present, 1 if any are missing.

failures=0
checks=0

while IFS= read -r rs_file; do
    dir="$(dirname "$rs_file")"
    pending_path=""

    while IFS= read -r line; do
        if [[ "$line" =~ ^[[:space:]]*#\[path[[:space:]]*=[[:space:]]*\"([^\"]+)\"\] ]]; then
            pending_path="${BASH_REMATCH[1]}"
            continue
        fi

        if [[ "$line" =~ ^[[:space:]]*(pub[[:space:]]+)?mod[[:space:]]+([a-z_][a-z0-9_]*)\; ]]; then
            mod_name="${BASH_REMATCH[2]}"
            checks=$((checks + 1))

            if [[ -n "$pending_path" ]]; then
                target="$dir/$pending_path"
                if [ -f "$target" ]; then
                    echo "PASS: $rs_file — mod ${mod_name}; (via #[path = \"${pending_path}\"])"
                else
                    echo "FAIL: $rs_file — mod ${mod_name}; (path ${target} does not exist)"
                    failures=$((failures + 1))
                fi
                pending_path=""
            else
                mod_file="$dir/${mod_name}.rs"
                mod_dir_file="$dir/${mod_name}/mod.rs"

                if [ -f "$mod_file" ] || [ -f "$mod_dir_file" ]; then
                    echo "PASS: $rs_file — mod ${mod_name};"
                else
                    echo "FAIL: $rs_file — mod ${mod_name}; (neither ${mod_file} nor ${mod_dir_file} exists)"
                    failures=$((failures + 1))
                fi
            fi
        elif [[ ! "$line" =~ ^[[:space:]]*(#\[|//|$) ]]; then
            pending_path=""
        fi
    done < "$rs_file"
done < <(find . -name '*.rs' \
    -not -path './target/*' \
    -not -path './contracts/dispute-resolver/*' \
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
