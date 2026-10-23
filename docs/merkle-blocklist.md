# Merkle blocklist guide

This document explains how to bulk-import a large blocklist into the compliance
contract using a Merkle root, how to generate proofs for individual addresses,
and how `is_blocked_with_proof` evaluates those proofs on-chain.

The Merkle-root mechanism supplements — it never replaces — the individual
`block_address` / `block_address_until` entries already stored in the compliance
contract. `is_blocked_with_proof` checks the individual flag first; it only walks
the Merkle proof when the direct flag is absent. This means a direct block applied
after the Merkle root is set will always win, and you do not need to regenerate
the whole tree to block a single new address.

---

## 1. Why a Merkle root?

Individually blocking thousands of addresses via `block_address` is expensive
on Soroban: each call occupies a separate transaction and each blocked address
occupies persistent storage with its own TTL fees. A Merkle root encodes the
same information as a single 32-byte value stored in instance storage. Proof
verification is O(log n) in the number of blocked addresses — typically 20–30
hash operations for a list of one million addresses — within a single contract
call.

The trade-off: blocked-by-Merkle addresses do not appear in `get_block_reason` or
`get_block_placer` and are invisible to `export_snapshot`. Use individual blocks
for addresses that need on-chain accountability metadata; use the Merkle root for
large, externally-maintained sanctions lists.

---

## 2. Leaf and node hashing scheme

The compliance contract's `is_blocked_with_proof` uses this exact scheme — any
off-chain generator must match it byte-for-byte or proofs will always fail.

**Leaf hash** — `sha256(address.to_xdr(env))`:
An address is converted to its canonical Soroban XDR encoding, then SHA-256
hashed. This is how `Address::to_xdr` works in `soroban-sdk`; the encoding
includes a type discriminant and the raw 32-byte public key.

**Internal node hash** — `sha256(min(left, right) || max(left, right))`:
The two children are sorted lexicographically (byte-by-byte comparison of their
32-byte arrays) before concatenation. Sorting makes the tree order-independent:
you can build the tree from any ordered list of addresses and still produce the
same root.

```
node = sha256(sorted_concat(left_child, right_child))
where sorted_concat(a, b) = a ++ b  if a <= b
                           = b ++ a  if b < a
```

---

## 3. Generating the Merkle tree off-chain

No bundled script exists for Merkle tree generation; the snippet below is the
reference Python implementation that mirrors the on-chain hashing exactly.

```python
import hashlib
import struct

# soroban-sdk Address XDR encoding for a Stellar account (ed25519 public key):
# 4-byte discriminant for Account (0x00000000)
# 4-byte discriminant for PublicKeyTypeEd25519 (0x00000000)
# 32 bytes raw public key
# Total: 40 bytes
#
# This matches what Address::to_xdr(&env) produces for a G-address.
def address_to_xdr_bytes(public_key_bytes: bytes) -> bytes:
    account_discriminant = struct.pack(">I", 0)       # ScAddressType::Account
    key_type_discriminant = struct.pack(">I", 0)      # PublicKeyType::PublicKeyTypeEd25519
    return account_discriminant + key_type_discriminant + public_key_bytes

def leaf_hash(public_key_bytes: bytes) -> bytes:
    return hashlib.sha256(address_to_xdr_bytes(public_key_bytes)).digest()

def node_hash(left: bytes, right: bytes) -> bytes:
    a, b = (left, right) if left <= right else (right, left)
    return hashlib.sha256(a + b).digest()

def build_merkle_tree(leaves: list[bytes]) -> list[list[bytes]]:
    """Returns the full tree as a list of levels, index 0 = leaf level."""
    if not leaves:
        raise ValueError("cannot build tree from empty leaf list")
    level = [leaf_hash(pk) for pk in leaves]
    # Pad to even length by duplicating the last leaf if needed.
    if len(level) % 2 == 1:
        level.append(level[-1])
    tree = [level]
    while len(level) > 1:
        next_level = []
        for i in range(0, len(level), 2):
            next_level.append(node_hash(level[i], level[i + 1]))
        if len(next_level) > 1 and len(next_level) % 2 == 1:
            next_level.append(next_level[-1])
        tree.append(next_level)
        level = next_level
    return tree

def merkle_root(leaves: list[bytes]) -> bytes:
    return build_merkle_tree(leaves)[-1][0]

def merkle_proof(leaves: list[bytes], target_index: int) -> list[bytes]:
    """Returns the sibling hashes needed to verify leaves[target_index]."""
    tree = build_merkle_tree(leaves)
    proof = []
    idx = target_index
    for level in tree[:-1]:  # every level except the root
        sibling_idx = idx ^ 1  # XOR with 1 toggles between even/odd pair
        if sibling_idx < len(level):
            proof.append(level[sibling_idx])
        idx //= 2
    return proof
```

### Worked example

Suppose you have three addresses to bulk-block, represented by their raw 32-byte
ed25519 public keys:

```python
# Illustrative key bytes (in practice, decode from strkey/base58)
alice_key = bytes(range(32))           # 0x000102...1f
bob_key   = bytes([255 - i for i in range(32)])  # 0xff fe fd...
carol_key = bytes([i * 3 % 256 for i in range(32)])

leaves = [alice_key, bob_key, carol_key]

root  = merkle_root(leaves)
proof = merkle_proof(leaves, 0)  # proof for alice_key

print("Root (hex):", root.hex())
print("Proof steps:", len(proof))
```

`build_merkle_tree` with 3 leaves pads to 4 (duplicates carol), building:

```
Level 0 (leaves): [H(alice), H(bob), H(carol), H(carol)]
Level 1:          [node(H(alice),H(bob)), node(H(carol),H(carol))]
Level 2 (root):   [node(level1[0], level1[1])]
```

Alice's proof = `[H(bob), node(H(carol),H(carol))]` — 2 siblings, one per level
above the leaf.

---

## 4. Calling `set_blocklist_root`

`set_blocklist_root` is admin-only and takes a single 32-byte hash. It replaces
any previously stored root atomically — there is no history of old roots on-chain.

```bash
stellar contract invoke \
  --id $COMPLIANCE_CONTRACT \
  --source $ADMIN \
  --network $NETWORK \
  -- set_blocklist_root \
  --admin $ADMIN \
  --root "$(python3 -c "import sys; sys.stdout.buffer.write(bytes.fromhex('$ROOT_HEX'))" | base64)"
```

Or in TypeScript via the generated client:

```typescript
import { Contract, xdr, hash } from "@stellar/stellar-sdk";

// rootHex is the 40-character hex string produced by merkle_root().hex()
const rootBytes = Buffer.from(rootHex, "hex");
const rootXdr   = xdr.ScVal.scvBytes(rootBytes);

await complianceClient.set_blocklist_root({
  admin:  adminKeypair.publicKey(),
  root:   rootBytes,
});
```

**Notes:**
- The call is permitted while the contract is paused (no `require_not_paused`
  guard), matching the same emergency-policy intent as `block_address`.
- Rotating the root — publishing a new list — is just another `set_blocklist_root`
  call with the new root. The old root is silently overwritten.
- Clearing the blocklist Merkle root: call `set_blocklist_root` with a 32-byte
  value that no proof can satisfy (e.g., all zeros), or store a root of an empty
  tree. There is no dedicated "clear" entrypoint.

---

## 5. Calling `is_blocked_with_proof`

`is_blocked_with_proof` is permissionless (no auth required). It accepts the
address to test and an ordered list of sibling hashes that form the Merkle proof.

```bash
stellar contract invoke \
  --id $COMPLIANCE_CONTRACT \
  --source $ANY_ACCOUNT \
  --network $NETWORK \
  -- is_blocked_with_proof \
  --address $TARGET_ADDRESS \
  --proof '["<sibling_1_base64>","<sibling_2_base64>"]'
```

In TypeScript:

```typescript
const proof: Buffer[] = merkleProof.map(h => Buffer.from(h, "hex"));

const isBlocked: boolean = await complianceClient.is_blocked_with_proof({
  address: targetAddress,
  proof:   proof,
});
```

The contract evaluates the call in this order:

1. If `DataKey::Blocked(address)` or a non-expired `DataKey::BlockedUntil(address)`
   flag is set, return `true` immediately — the proof is not needed.
2. If no `BlocklistRoot` is stored in instance storage, return `false`.
3. Compute `node = sha256(address.to_xdr(env))`.
4. For each sibling in `proof`, sort `(node, sibling)` and hash the concatenation
   into the new `node`.
5. Return `node == stored_root`.

An empty proof `[]` is valid if and only if the address is the sole leaf and
is also the root — practically, a single-address tree.

### Error cases

| Situation | Result |
|---|---|
| No `BlocklistRoot` stored | `false` |
| Wrong proof (any step) | `false` (hash mismatch at root) |
| Direct `Blocked` flag set | `true` (short-circuits before tree walk) |
| Empty proof, single-address tree | `true` if `sha256(address.to_xdr()) == root` |

---

## 6. Off-chain proof delivery

Because Soroban transactions are stateless, off-chain callers must supply the
proof at call time. A typical integration:

1. **Index step** — after each `set_blocklist_root` call, rebuild the full tree
   off-chain and store `(root_hex, address → proof_hex_list)` in a key-value
   store or a database.
2. **Query step** — when a payment is about to be submitted, look up the current
   root, retrieve the address's proof, and include it in the
   `is_blocked_with_proof` invocation.
3. **Root rotation** — when the admin sets a new root, invalidate the cached
   proofs and rebuild from the new address list.

Off-chain systems should cross-check the root stored in instance storage
(`get_blocklist_root` if exposed, or read storage directly via RPC) against
the root they used to generate proofs before submitting a batch of
`is_blocked_with_proof` calls. A mismatch means the root has been rotated and
all proofs need regeneration.

---

## 7. Security considerations

- **Root integrity** — the Merkle root is as trustworthy as the admin key that
  set it. If the admin key is compromised, an attacker can set a root that
  excludes known-bad addresses. Protect the admin key accordingly.
- **Proof freshness** — a proof generated against an old root is invalid after
  root rotation. Proofs should be treated as ephemeral and regenerated on each
  root change.
- **No deletion audit trail** — unlike individual `block_address` calls (which
  emit events and store `BlockedBy`), removing an address from the Merkle set
  leaves no on-chain record. Use individual blocks for regulatory accountability.
- **Duplicate leaves** — the padding scheme (duplicating the last leaf to even
  the tree) means a tree with 2n−1 addresses will evaluate `is_blocked_with_proof`
  as `true` for the last address even with the proof for its duplicate. This is
  expected behavior and consistent with standard Merkle tree padding.
