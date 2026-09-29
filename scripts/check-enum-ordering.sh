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
#
# #627: error enums declared through `declare_contract_error!` (crates/error-macros)
# are checked too. The macro supplies #[repr(u32)] itself, so the source contains
# the macro invocation rather than the attribute, and the invocation is treated as
# an equivalent block opener here. Without this, moving an error enum behind the
# macro would have silently removed it from this check's coverage — the exact
# failure mode this script exists to prevent.

set -euo pipefail

echo "=== Enum ordering check ==="

errors=0
status_file="$(mktemp)"
trap 'rm -f "$status_file"' EXIT

# Macro invocations that stand in for `#[repr(u32)] pub enum <Name> { ... }`.
ERROR_ENUM_MACROS='declare_contract_error'

# Find all #[repr(u32)] enum definitions under contracts/ and crates/
# We use awk to extract the enum name and its variants with explicit discriminants.
# NOTE: this loop runs in a subshell (piped from `find`), so per-file failures are
# recorded to status_file rather than the `errors` variable, which would not
# survive back to the parent shell.
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
    #   1 = an enum block opener was seen and the `pub enum` line has not yet
    #       arrived ("pending"). Armed by either `#[repr(u32)]` or one of the
    #       $ERROR_ENUM_MACROS invocations.
    #   2 = inside enum block (between { and })
    #
    # The `pub enum` line is allowed to be indented: inside a
    # `declare_contract_error! { ... }` invocation it is nested one level, but the
    # variants inside it are still laid out exactly as in a hand-written enum.
    found_repr=0
    in_enum=0
    enum_name=""
    variants=""
    brace_depth=0
    prev_discriminant=0
    expected_next=1
    has_errors=0

    while IFS= read -r line; do
        trimmed="${line#"${line%%[![:space:]]*}"}"

        # #627: a `declare_contract_error!` invocation carries the same
        # #[contracterror] + #[repr(u32)] contract as a hand-written attribute
        # block, so it arms the same block detection. Only the macros named in
        # $ERROR_ENUM_MACROS are treated this way — an arbitrary macro could
        # expand to anything.
        if [[ "$trimmed" =~ ^([A-Za-z0-9_]+)! ]]; then
            macro_name="${BASH_REMATCH[1]}"
            case "|$ERROR_ENUM_MACROS|" in
                *"|$macro_name|"*)
                    found_repr=1
                    continue
                    ;;
            esac
        fi

        # Check for #[repr(u32)]
        if [[ "$line" =~ '#[repr(u32)]' ]]; then
            found_repr=1
            continue
        fi

        # If we found an opener, look for the enum declaration. The `pub enum`
        # line is NOT skipped with `continue` below: its opening brace has to be
        # counted, otherwise the first variant is read at depth 0 and — because
        # the body is only parsed once the depth is non-zero — every variant of
        # every enum would be silently ignored.
        if [[ $found_repr -eq 1 && "$trimmed" =~ ^pub[[:space:]]+enum[[:space:]]+([A-Za-z0-9_]+) ]]; then
            enum_name="${BASH_REMATCH[1]}"
            in_enum=1
            brace_depth=0
            variants=""
            prev_discriminant=0
            expected_next=1
            has_errors=0
            found_repr=0
        elif [[ $found_repr -eq 1 ]]; then
            # Doc comments between the opener and the `pub enum` line are expected
            # in the macro form (the macro forwards them to the generated enum),
            # so they must not disarm the pending block. Anything else — a struct
            # carrying a repr, say — does.
            if [[ "$trimmed" =~ ^/// ]]; then
                continue
            fi
            found_repr=0
        fi

        # Parse enum body
        if [[ $in_enum -eq 1 ]]; then
            # Track brace depth
            for ((i=0; i<${#line}; i++)); do
                ch="${line:$i:1}"
                if [[ "$ch" == "{" ]]; then
                    brace_depth=$((brace_depth + 1))
                elif [[ "$ch" == "}" ]]; then
                    brace_depth=$((brace_depth - 1))
                fi
            done

            if [[ $brace_depth -gt 0 || ($brace_depth -eq 0 && "$line" =~ \}) ]]; then
                # Extract variant and discriminant
                if [[ "$line" =~ ([A-Za-z0-9_]+)[[:space:]]*=[[:space:]]*([0-9]+) ]]; then
                    variant="${BASH_REMATCH[1]}"
                    disc="${BASH_REMATCH[2]}"

                    if [[ "$disc" -ne "$expected_next" ]]; then
                        echo "ERROR: $file: enum $enum_name variant $variant has discriminant $disc but expected $expected_next"
                        has_errors=1
                    fi
                    expected_next=$((disc + 1))
                fi

                if [[ $brace_depth -le 0 ]]; then
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
        echo "1" >> "$status_file"
    fi
done

if [[ -s "$status_file" ]]; then
    errors=1
fi

if [[ $errors -eq 0 ]]; then
    echo "✓ All enums pass append-only ordering check"
else
    echo "✗ Some enums have ordering violations (see above)"
fi

exit $errors

