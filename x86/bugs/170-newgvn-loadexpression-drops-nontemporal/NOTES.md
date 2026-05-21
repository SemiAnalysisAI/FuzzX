# NewGVN `LoadExpression` CSE drops `!nontemporal` metadata

## File and root cause

`llvm/lib/Transforms/Scalar/NewGVN.cpp` — `LoadExpression::equals` (line 927)
and `equalsLoadStoreHelper` (line 920).

```c++
template <typename T>
static bool equalsLoadStoreHelper(const T &LHS, const Expression &RHS) {
  if (!isa<LoadExpression>(RHS) && !isa<StoreExpression>(RHS))
    return false;
  return LHS.MemoryExpression::equals(RHS);
}

bool LoadExpression::equals(const Expression &Other) const {
  return equalsLoadStoreHelper(*this, Other);
}
```

`MemoryExpression::equals` (`GVNExpression.h:289`) only checks
`BasicExpression::equals` plus `MemoryLeader` pointer equality. None of:

* `!nontemporal` metadata
* alignment differences relevant to NT codegen
* (since the `isSimple()` guard at line 1558 prevents atomic/volatile loads
  from being symbolically evaluated at all, syncscope/ordering are excluded
  upstream — but `!nontemporal` is not)

are part of the equality test. As a result, two loads from the same address
with the same `MemorySSA` defining access are unified into the same
`CongruenceClass`, and `eliminateInstructions` deletes the redundant one and
RAUWs its uses to the leader. `combineMetadataForCSE` then intersects metadata
(`Local.cpp:3030`), so `!nontemporal` survives only if BOTH the kept and the
eliminated load carry it.

## Reproducer

`x86/candidates/w99-load-nt.ll`:

```llvm
target triple = "x86_64-unknown-linux-gnu"

define <4 x i32> @test(ptr %p) {
entry:
  %a = load <4 x i32>, ptr %p, align 16
  %b = load <4 x i32>, ptr %p, align 16, !nontemporal !0
  %r = add <4 x i32> %a, %b
  ret <4 x i32> %r
}

!0 = !{i32 1}
```

### `opt -passes=newgvn` diff

Before:
```llvm
  %a = load <4 x i32>, ptr %p, align 16
  %b = load <4 x i32>, ptr %p, align 16, !nontemporal !0
  %r = add <4 x i32> %a, %b
```

After:
```llvm
  %a = load <4 x i32>, ptr %p, align 16
  %r = add <4 x i32> %a, %a
```

The `!nontemporal` load `%b` is eliminated and the surviving regular load `%a`
has NO `!nontemporal` metadata. The original program asked the codegen to use
non-temporal load for `%b` (e.g. `MOVNTDQA` / `vmovntdqa` on x86 with SSE4.1);
after NewGVN that hint is gone.

For comparison: regular `-passes=gvn` produces a similar replacement but the
same metadata-merge logic applies. The bug here is unique to NewGVN's
unified-class treatment of loads with same `(MemoryLeader, pointer, type)` but
different metadata.

## Why this is a regression

* Loss of programmer-visible cache-bypass intent.
* On x86 with SSE4.1+, this changes codegen from `MOVNTDQA` to `MOVDQA`
  (cacheable). The original `%b` was a streaming load; after NewGVN it is a
  regular load.
* On targets like AMDGPU where `!nontemporal` loads use different cache
  policy bits (`glc`/`slc`/`dlc`), losing the hint changes whether the
  generated load goes through the L1 cache, with measurable performance
  cliff (and on systems that rely on those bits for coherence with
  device-coherent memory regions, this can be a correctness issue).

## Fix sketch

* Extend `LoadExpression::equals` to also compare
  `hasMetadata(MD_nontemporal)`, so loads with mismatched NT hint fall into
  separate congruence classes. (Stricter than necessary if we only care about
  the hint, but a simple fix.)
* Alternatively, in the load-elimination path, when the leader does NOT have
  `!nontemporal` but the eliminated load did, propagate `!nontemporal` to the
  leader (union-style, not intersect-style merge).

Note: the same root cause produces the StoreExpression variant filed in
`w99-newgvn-storeexpression-drops-nontemporal.md`.
