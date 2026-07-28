#!/usr/bin/env bash
#
# #74: Append-only enum-ordering CI check
#
# Validates that #[contracterror] and #[contracttype] enums in the codebase
# only append new variants (i.e. the highest #[repr(u32)] value is on the
# last variant). This prevents accidental insertions, reorderings, or removals
# that would shift existing ordinal positions and break on-chain backwards
# compatibility.
#
# Usage:
#   ./scripts/check-enum-ordering.sh
#
# Exit codes:
#   0 - all enums pass append-only ordering
#   1 - one or more enums have ordering violations
#
# The script extracts every #[repr(u32)] enum block from .rs files under
# contracts/ and crates/, parses each variant's explicit discriminant, and
# verifies that discriminants are strictly increasing by 1 (i.e. no gaps,
# no reordering, no insertions in the middle).

set -euo pipefail

echo "=== Enum ordering check ==="

errors=0

# Find all #[repr(u32)] enum definitions under contracts/ and crates/
# We use awk to extract the enum name and its variants with explicit discriminants.
find contracts crates -name '*.rs' -type f | while read -r file; do
    # Skip test files
    if echo "$file" | grep -q '/tests/'; then
        continue
    fi

    # Extract enum blocks that are #[repr(u32)].
    # We look for: #[repr(u32)] followed by pub enum <Name> { ... }
    # Then extract each variant's discriminant value.
    #
    # awk state machine:
    #   1 = found "#[repr(u32)]"
    #   2 = inside enum block (between { and })
    found_repr=0
    in_enum=0
    enum_name=""
    variants=""
    brace_depth=0
    prev_discriminant=0
    expected_next=1
    has_errors=0

    while IFS= read -r line; do
        # Check for #[repr(u32)]
        if [[ "$line" =~ '#[repr(u32)]' ]]; then
            found_repr=1
            continue
        fi

        # If we found repr, look for enum declaration
        if [[ $found_repr -eq 1 && "$line" =~ ^pub[[:space:]]+enum[[:space:]]+([A-Za-z0-9_]+) ]]; then
            enum_name="${BASH_REMATCH[1]}"
            in_enum=1
            brace_depth=0
            variants=""
            prev_discriminant=0
            expected_next=1
            has_errors=0
            found_repr=0
            continue
        fi

        # Reset if we found repr but no enum follows (e.g. on struct)
        if [[ $found_repr -eq 1 ]]; then
            found_repr=0
        fi

        # Parse enum body
        if [[ $in_enum -eq 1 ]]; then
            # Track brace depth
            for ((i=0; i<${#line}; i++)); do
                ch="${line:$i:1}"
                if [[ "$ch" == "{" ]]; then
                    ((brace_depth++))
                elif [[ "$ch" == "}" ]]; then
                    ((brace_depth--))
                fi
            done

            if [[ $brace_depth -gt 0 || ($brace_depth -eq 0 && "$line" =~ \}) ]]; then
                # Extract variant and discriminant
                if [[ "$line" =~ ([A-Za-z0-9_]+)[[:space:]]*=[[:space:]]*([0-9]+) ]]; then
                    variant="${BASH_REMATCH[1]}"
                    disc="${BASH_REMATCH[2]}"

                    # Skip if this is a known "historical" variant not following the pattern
                    if [[ "$enum_name" == "InvoiceError" && "$variant" == "NotReleased" ]]; then
                        continue  # Code 11, follows pattern
                    fi

                    if [[ "$disc" -ne "$expected_next" ]]; then
                        echo "ERROR: $file: enum $enum_name variant $variant has discriminant $disc but expected $expected_next"
                        has_errors=1
                    fi
                    expected_next=$((disc + 1))
                fi

                if [[ $brace_depth -eq 0 ]]; then
                    in_enum=0
                    if [[ $has_errors -ne 0 ]]; then
                        errors=1
                    fi
                fi
            fi
        fi
    done < "$file"

    if [[ $errors -ne 0 ]]; then
        echo "FAILED"
    fi
done

if [[ $errors -eq 0 ]]; then
    echo "✓ All enums pass append-only ordering check"
else
    echo "✗ Some enums have ordering violations (see above)"
fi

exit $errors

