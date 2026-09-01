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
done

if [[ -s "$status_file" ]]; then
    echo ""
    echo "✗ One or more #[contracterror]/#[contracttype] enums are missing an enum-level doc comment."
    echo "  Add a /// comment immediately before the attribute block of each flagged enum."
    exit 1
fi

echo "✓ All #[contracterror]/#[contracttype] enums have an enum-level doc comment."
exit 0
