# Merkle blocklist guide

Bulk-import a large blocklist into `compliance` as a single 32-byte Merkle
root, then prove membership per address with `is_blocked_with_proof`.

Source of truth: `contracts/compliance/src/lib.rs`
(`set_blocklist_root`, `get_blocklist_root`, `is_blocked_with_proof`),
tests in `contracts/compliance/tests/pause_reason_and_merkle_test.rs`,
reference generator in `scripts/build-blocklist-merkle.py`, and design notes
in `docs/compliance-sanctions-design.md` (§ Addendum).

The root supplements — never replaces — individual `block_address` /
`block_address_until` flags. `is_blocked_with_proof` checks the individual
flag first and only walks the proof when it is absent. `is_allowed` does not
take a proof and is unchanged by the root; settlement flows that must honour
the imported list call `is_blocked_with_proof` with an off-chain-supplied
proof.

## 1. Hashing scheme (must match byte-for-byte)

On-chain (`lib.rs`):

```rust
// Leaf:
let mut node: BytesN<32> = env.crypto().sha256(&address.to_xdr(&env)).into();
// Each level:
let (a, b) = if node.to_array() <= sibling.to_array() {
    (node, sibling)
} else {
    (sibling, node)
};
let mut buf = Bytes::from_array(&env, &a.to_array());
buf.extend_from_array(&b.to_array());
node = env.crypto().sha256(&buf).into();
```

That is:

- Leaf: `sha256(address.to_xdr())` (canonical Soroban XDR, not the raw
  strkey bytes).
- Internal node: `sha256(min(left, right) || max(left, right))`, bytewise
  comparison of the 32-byte arrays. Proofs are therefore an unordered list
  of siblings — no index bits are needed.

Reference generator: `scripts/build-blocklist-merkle.py` prints deterministic
test vectors (valid / tampered / wrong-order) mirrored in the test module:

```bash
python3 scripts/build-blocklist-merkle.py
```

```text
# ordering: positional (NOT sorted)
root = 0x599578092399354870dd428c87937093ed1f72a972f2172af4768d6d3eeaf947
target = 0x0303030303030303030303030303030303030303
proof = [
    0x64761807a19e002a488ea37ba66102ff75bf4ea4d0207e99a75acac05e5e52a6,
    0xf5f924456935788f53b84b5572f23e9ef207f04099652c0d4def01d1dbabcab4,
]
valid      = False
tampered   = False
wrong_order= False
```

The script uses domain-separated positional hashing
(`H(0x00 || addr)`, `H(0x01 || left || right)`) to exercise ordering edge
cases in tests. For a production tree that verifies on-chain, use the sorted
`sha256(min || max)` scheme above with `address.to_xdr()` leaves. The Python
helper in §2 mirrors the on-chain scheme exactly.

## 2. Build the tree off-chain

```python
import hashlib

def leaf_hash(address_xdr: bytes) -> bytes:
    return hashlib.sha256(address_xdr).digest()

def node_hash(left: bytes, right: bytes) -> bytes:
    a, b = (left, right) if left <= right else (right, left)
    return hashlib.sha256(a + b).digest()

def build_tree(leaf_xdrs: list[bytes]) -> list[list[bytes]]:
    """Levels[0] = leaves. Odd levels duplicate the last node."""
    level = [leaf_hash(x) for x in leaf_xdrs]
    if len(level) % 2 == 1:
        level = level + [level[-1]]
    tree = [level]
    while len(level) > 1:
        level = [node_hash(level[i], level[i + 1])
                 for i in range(0, len(level), 2)]
        if len(level) > 1 and len(level) % 2 == 1:
            level = level + [level[-1]]
        tree.append(level)
    return tree

def merkle_proof(tree: list[list[bytes]], index: int) -> list[bytes]:
    proof, idx = [], index
    for level in tree[:-1]:
        proof.append(level[idx ^ 1])
        idx //= 2
    return proof
```

Pad odd levels by duplicating the last node so every parent has two
children — this matches the test helper (`node(env, left, right)`) in
`pause_reason_and_merkle_test.rs`.

## 3. Publish the root with `set_blocklist_root`

Admin-only, permitted while paused (same emergency policy as
`block_address`). Replaces any previous root atomically; proofs against the
old root stop verifying immediately. Emits `("blocklist_root_set",) → root`.

```sh
stellar contract invoke \
  --id $COMPLIANCE_CONTRACT \
  --source $ADMIN \
  --network $NETWORK \
  -- set_blocklist_root \
  --admin $ADMIN \
  --root $ROOT_HEX
```

Confirm with `get_blocklist_root` (returns `Option<BytesN<32>>`, `None` when
no root is stored).

Rotation is just another `set_blocklist_root` call. There is no dedicated
clear entrypoint; to disable Merkle checks, rotate to a root no proof can
satisfy or stop supplying proofs (missing root falls back to `is_blocked`).

## 4. Build proofs

For leaf `i`, collect one sibling per level, leaf-to-root
(`merkle_proof(tree, i)` above). Proof length equals tree height; an empty
proof is only meaningful for a single-leaf tree (leaf == root) or when the
address is individually blocked (short-circuit, §5).

Because pairs are sorted at hash time, sibling order within each pair does
not affect verification — but a wrong sibling hash, a truncated proof, or a
proof for a different address fails (root mismatch).

## 5. Check with `is_blocked_with_proof`

Permissionless read: `is_blocked_with_proof(address, proof) -> bool`.
Evaluation order:

1. If `is_blocked(address)` is true, return `true` immediately (proof
   ignored). This covers `block_address` / `block_address_until` flags.
2. If no `BlocklistRoot` is stored, return `false`.
3. Walk the proof from `sha256(address.to_xdr())` with sorted-pair hashing;
   return `computed == stored_root`.

```sh
stellar contract invoke \
  --id $COMPLIANCE_CONTRACT \
  --network $NETWORK \
  -- is_blocked_with_proof \
  --address $TARGET_ADDRESS \
  --proof '["<sibling_1>", "<sibling_2>"]'
```

| Situation | Result |
|---|---|
| No root stored | `false` (unless individually blocked) |
| Correct proof for listed address | `true` |
| Wrong sibling / truncated / wrong address | `false` |
| Empty proof, multi-leaf tree | `false` (unless individually blocked) |
| Individually blocked (any proof, incl. empty) | `true` |
| Root rotated since proof was built | `false` (regenerate from new list) |

Blocked-by-Merkle addresses do not appear in `get_block_reason`,
`get_block_placer`, or `export_snapshot` — use individual blocks when
on-chain accountability metadata is required.

## 6. Worked example (4 leaves)

Using stand-ins for `address.to_xdr()` bytes:

```python
addrs  = [b"address-a-xdr", b"address-b-xdr", b"address-c-xdr", b"address-d-xdr"]
tree   = build_tree(addrs)          # levels: 4 leaves -> 2 nodes -> root
root   = tree[-1][0]
proof_a = merkle_proof(tree, 0)     # [leaf(b), node(c,d)]
```

```text
ab   = 19fc0cfad6044442a815f6de81e6c91bff77d608c9a1cc6a8cbde675cc643413
cd   = a2e4bda1b1b0661f536e27bde4b1690291d2835b6859a59311a60a288418f5a5
root = 3f215094fffaf845cb64a7f6b600033718090bb5abf333f03011b8e18f6ea93a
```

End-to-end (mirrors `merkle_proofs_valid_and_merkle` tests):

```sh
# 1. Publish (admin signs; works even while paused).
stellar contract invoke --id $COMPLIANCE_CONTRACT --source $ADMIN --network $NETWORK \
  -- set_blocklist_root --admin $ADMIN --root 3f215094fffaf845cb64a7f6b600033718090bb5abf333f03011b8e18f6ea93a

# 2. Listed address + correct proof -> true.
stellar contract invoke --id $COMPLIANCE_CONTRACT --network $NETWORK \
  -- is_blocked_with_proof --address $ADDR_A --proof '["<lb>", "<cd>"]'
# -> true

# 3. Same address, tampered sibling -> false.
# 4. Unlisted address + a's proof -> false.
# 5. Listed address + empty proof -> false.
# 6. After block_address($OUTSIDER): is_blocked_with_proof($OUTSIDER, []) -> true.
# 7. After set_blocklist_root to a new root: old proofs -> false.
```

The contract test module covers each row: `merkle_proofs_valid_and_invalid`
(root-absent, valid, wrong-address, empty, direct-block short-circuit,
rotation), `merkle_proofs_valid_for_all_leaves`,
`merkle_proofs_reject_tampered_siblings`, and
`merkle_proofs_reject_wrong_order`.

## 7. Operational notes

- **Proof delivery:** transactions are stateless — indexers must store
  `(root, address -> proof)` after each rotation and attach the current
  proof at call time. Check `get_blocklist_root` before a batch; a mismatch
  means proofs must be regenerated.
- **Freshness:** treat proofs as ephemeral. Rotation invalidates every
  cached proof instantly.
- **No removal trail:** dropping an address from the off-chain list leaves
  no on-chain event (unlike `clear_address`). Keep the published list
  versioned off-chain for audits.
- **Odd-tree padding:** duplicating the last node is standard padding, not
  a vulnerability — document the list size alongside the root so verifiers
  can reproduce it.
