#!/usr/bin/env python3
"""Reference generator for blocklist Merkle proof test vectors.

Ordering convention (must match `is_blocked_with_proof` in the compliance
contract): sibling hashes are POSITIONAL, not sorted. At each level the
proof element is combined as:

    parent = H(current || sibling)   if the current node is a LEFT child
    parent = H(sibling || current)   if the current node is a RIGHT child

Leaf encoding: H(0x00 || address_bytes) where address_bytes is the raw
20-byte account id. Internal nodes: H(0x01 || left || right).

This script prints deterministic vectors (valid, tampered, wrong-order)
that are mirrored in the contract's test module. Run with:

    python3 scripts/build-blocklist-merkle.py
"""

import hashlib


def h(data: bytes) -> bytes:
    return hashlib.sha256(data).digest()


def leaf(addr: bytes) -> bytes:
    return h(b"\x00" + addr)


def node(left: bytes, right: bytes) -> bytes:
    return h(b"\x01" + left + right)


def build_tree(addrs):
    """Build a positional Merkle tree; returns (levels, root).

    levels[0] is the leaf level. Odd levels are padded by duplicating the
    last node so every parent has two children.
    """
    level = [leaf(a) for a in addrs]
    levels = [level]
    while len(level) > 1:
        if len(level) % 2 == 1:
            level = level + [level[-1]]
        level = [node(level[i], level[i + 1]) for i in range(0, len(level), 2)]
        levels.append(level)
    return levels, levels[-1][0]


def proof_for(levels, index):
    """Return positional siblings from leaf to root for `index`."""
    proof = []
    idx = index
    for level in levels[:-1]:
        sibling = idx ^ 1
        if sibling >= len(level):
            sibling = idx
        proof.append(level[sibling])
        idx //= 2
    return proof


def verify(addr: bytes, proof, root: bytes) -> bool:
    """Positional verification matching `is_blocked_with_proof`."""
    cur = leaf(addr)
    idx = 0
    for sibling in proof:
        if idx % 2 == 0:
            cur = node(cur, sibling)
        else:
            cur = node(sibling, cur)
        idx //= 2
    return cur == root


def hexb(b: bytes) -> str:
    return "0x" + b.hex()


def main():
    addrs = [bytes([i]) * 20 for i in range(1, 5)]
    levels, root = build_tree(addrs)
    target = 2
    proof = proof_for(levels, target)

    print("# ordering: positional (NOT sorted)")
    print("root =", hexb(root))
    print("target =", hexb(addrs[target]))
    print("proof = [")
    for p in proof:
        print("   ", hexb(p) + ",")
    print("]")

    print("valid      =", verify(addrs[target], proof, root))

    tampered = list(proof)
    tampered[0] = bytes([0xFF]) * 32
    print("tampered   =", verify(addrs[target], tampered, root))

    wrong_order = list(reversed(proof))
    print("wrong_order=", verify(addrs[target], wrong_order, root))


if __name__ == "__main__":
    main()
