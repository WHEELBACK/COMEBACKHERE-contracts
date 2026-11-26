#!/usr/bin/env bash
#
# #446: Enum-level doc-comment lint
#
# Checks that every `#[contracterror]` and `#[contracttype]` *enum* in the
# codebase has at least one `///` doc comment immediately before (or within)
# its attribute+declaration block.
#
# In standard Rust style, enum-level doc comments appear before the first
# attribute:
#
#   /// Doc comment here
#   #[contracttype]
#   #[derive(Clone)]
#   pub enum Foo { ... }
#
# The script accepts `///` lines anywhere in the contiguous block that spans
# from the first `///` or `#[contracterror]`/`#[contracttype]` line through
# to the `pub enum` line — this covers both "doc before attribute" and the
# less-common "doc between attributes" patterns.
#
# WHAT IS CHECKED
#   - pub enum declarations preceded by #[contracterror] or #[contracttype].
#   - At least one `///` line must appear in the lines leading up to
#     `pub enum` within a contiguous doc/attribute block.
#
# WHAT IS NOT CHECKED
#   - Structs (pub struct), even if #[contracttype]-annotated.
#   - Private enums (no `pub` keyword).
#   - Test files (paths containing `/tests/` or ending in `_test.rs`).
#
# #627: error enums declared through `declare_contract_error!` (crates/error-macros)
# are checked too. The macro supplies `#[contracterror]` itself, so the source
# contains the invocation instead; the invocation is treated as an equivalent
# block starter, and the doc comment is expected inside the braces, directly
# above the `pub enum` line the macro forwards it to.
#
# #955/#964/#973: the modularized error crates (event-emitter-errors,
# storage-layout-errors, access-control-errors) declare their enums through
# `declare_contract_error!`. Those invocations are already picked up by the
# `find contracts crates` scan below; in addition, every variant inside such an
# invocation must carry its own `///` doc comment so the enums stay documented
# for clients.
#
# USAGE
#   ./scripts/check-enum-doc-comments.sh
#
# EXIT CODES
#   0 - all matching enums have an enum-level doc comment
#   1 - one or more enums are missing an enum-level doc comment

set -euo pipefail

echo "=== Enum-level doc-comment check ==="

status_file="$(mktemp)"
trap 'rm -f "$status_file"' EXIT

# Macro invocations that stand in for `#[contracterror] pub enum <Name> { ... }`.
ERROR_ENUM_MACROS='declare_contract_error'

# Error crates whose `declare_contract_error!` variants must each be documented.
ERROR_CRATE_DIRS='crates/compliance-errors crates/invoice-errors crates/workflow-errors'

find contracts crates -name '*.rs' -type f | sort | while read -r file; do
    # Skip test files
    if echo "$file" | grep -qE '/tests/|_test\.rs$'; then
        continue
    fi

    # Read the whole file line by line.
    #
    # We track a sliding "candidate block" which starts whenever we see a `///`
    # doc-comment line or a `#[contracterror]`/`#[contracttype]` line.  The block
    # accumulates consecutive `///`, `#[...]`, `#[derive(...)]`, `#[repr(...)]`
    # and blank lines.  When we hit `pub enum`, we check whether the block
    # contained at least one `#[contracterror]` or `#[contracttype]` AND at
    # least one `///`.  Anything else resets the block.

    in_block=0    # 1 = we are accumulating a candidate block
    has_attr=0    # 1 = block contains #[contracterror] or #[contracttype]
    has_doc=0     # 1 = block contains at least one /// line
    block_start=0

    lineno=0
    while IFS= read -r line; do
        lineno=$((lineno + 1))
        trimmed="${line#"${line%%[![:space:]]*}"}"

        # Doc-comment line ///
        if [[ "$trimmed" =~ ^/// ]]; then
            if [[ $in_block -eq 0 ]]; then
                in_block=1
                has_attr=0
                has_doc=0
                block_start=$lineno
            fi
            has_doc=1
            continue
        fi

        # #[contracterror] or #[contracttype]
        if [[ "$trimmed" =~ ^#\[(contracterror|contracttype)\] ]]; then
            if [[ $in_block -eq 0 ]]; then
                in_block=1
                has_attr=0
                has_doc=0
                block_start=$lineno
            fi
            has_attr=1
            continue
        fi

        # #627: a `declare_contract_error!` invocation is the macro form of a
        # `#[contracterror]` attribute block, so it opens the same candidate
        # block. Only the macros named in $ERROR_ENUM_MACROS are treated this
        # way — an arbitrary macro could expand to anything.
        if [[ "$trimmed" =~ ^([A-Za-z0-9_]+)! ]]; then
            macro_name="${BASH_REMATCH[1]}"
            case "|$ERROR_ENUM_MACROS|" in
                *"|$macro_name|"*)
                    if [[ $in_block -eq 0 ]]; then
                        in_block=1
                        has_attr=0
                        has_doc=0
                        block_start=$lineno
                    fi
                    has_attr=1
                    continue
                    ;;
            esac
        fi

        if [[ $in_block -eq 1 ]]; then
            # Other attribute lines (#[derive(...)], #[repr(...)], etc.) — stay in block
            if [[ "$trimmed" =~ ^#\[ ]]; then
                continue
            fi

            # pub enum line — this is what we are waiting for
            if [[ "$trimmed" =~ ^pub[[:space:]]+enum[[:space:]]+([A-Za-z0-9_]+) ]]; then
                enum_name="${BASH_REMATCH[1]}"
                if [[ $has_attr -eq 1 && $has_doc -eq 0 ]]; then
                    echo "ERROR: $file:$block_start: pub enum $enum_name is missing an enum-level /// doc comment"
                    echo "1" >> "$status_file"
                fi
                # Reset for next block
                in_block=0
                has_attr=0
                has_doc=0
                continue
            fi

            # Blank lines are allowed inside an attribute block
            if [[ -z "$trimmed" ]]; then
                continue
            fi

            # Anything else resets the block (e.g. pub struct, a function, a comment)
            in_block=0
            has_attr=0
            has_doc=0
        fi
    done < "$file"

    # #955/#964/#973: variant-level doc-comment regression check for the
    # modularized error crates. Every variant declared inside a
    # `declare_contract_error!` invocation must be preceded by a `///` line.
    for crate_dir in $ERROR_CRATE_DIRS; do
        case "$file" in
            "$crate_dir"/*) ;;
            *) continue ;;
        esac

        in_macro=0
        depth=0
        pending_doc=0
        vlineno=0
        while IFS= read -r line; do
            vlineno=$((vlineno + 1))
            vtrimmed="${line#"${line%%[![:space:]]*}"}"

            if [[ $in_macro -eq 0 ]]; then
                if [[ "$vtrimmed" =~ ^declare_contract_error! ]]; then
                    in_macro=1
                    depth=0
                    pending_doc=0
                fi
                continue
            fi

            # Track brace depth so we know when the invocation ends.
            opens="${vtrimmed//[^\{]/}"
            closes="${vtrimmed//[^\}]/}"
            depth=$((depth + ${#opens} - ${#closes}))

            if [[ "$vtrimmed" =~ ^/// ]]; then
                pending_doc=1
                continue
            fi

            # A variant line: `Name = N,` or `Name,` (optionally with a value).
            if [[ "$vtrimmed" =~ ^([A-Za-z_][A-Za-z0-9_]*)[[:space:]]*(=.*)?,?$ ]]; then
                variant="${BASH_REMATCH[1]}"
                if [[ $pending_doc -eq 0 ]]; then
                    echo "ERROR: $file:$vlineno: variant $variant in declare_contract_error! is missing a /// doc comment"
                    echo "1" >> "$status_file"
                fi
                pending_doc=0
            elif [[ -n "$vtrimmed" && "$vtrimmed" != "{" && "$vtrimmed" != "}" ]]; then
                # Any other non-blank line (e.g. `pub enum Foo {`) resets the doc.
                pending_doc=0
            fi

            if [[ $depth -le 0 ]]; then
                in_macro=0
            fi
        done < "$file"
    done
done

if [[ -s "$status_file" ]]; then
    echo ""
    echo "✗ One or more #[contracterror]/#[contracttype] enums are missing a doc comment."
    echo "  Add a /// comment immediately before the attribute block of each flagged enum,"
    echo "  and a /// comment above every variant in the modularized error crates."
    exit 1
fi

echo "✓ All #[contracterror]/#[contracttype] enums have an enum-level doc comment."
echo "✓ All variants in the modularized error crates are documented."
exit 0
