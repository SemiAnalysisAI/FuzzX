# w325: foldPHIArgLoadIntoPHI leaks !invariant.group from FirstLI to merged load

## Summary

`InstCombinerImpl::foldPHIArgLoadIntoPHI` sinks identical-shape loads through a
PHI. It calls `NewLI->copyMetadata(*FirstLI)` (wholesale copy from the FIRST
load), then for each subsequent load `combineMetadataForCSE(NewLI, LI, true)`.

`combineMetadataForCSE` -> `combineMetadata` handles `MD_invariant_group`
**outside the K-metadata loop**, via:

```
if (auto *JMD = J->getMetadata(LLVMContext::MD_invariant_group))
  if (isa<LoadInst>(K) || isa<StoreInst>(K))
    K->setMetadata(LLVMContext::MD_invariant_group, JMD);
```

(`llvm/lib/Transforms/Utils/Local.cpp:3065-3067`)

So this overwrites K's invariant.group **only if J has one**. When J does not
have invariant.group, K retains the value copied from FirstLI. The merged load
now claims `!invariant.group` even though one of its merged sources had no such
guarantee.

This violates the invariant.group semantics: GVN/loadcoalescing/etc may consider
this merged load equivalent to other loads in the same `!invariant.group` (after
launder/strip), but the load can legitimately read a non-invariant value from
the `bb2` predecessor.

## Source

- `llvm/lib/Transforms/InstCombine/InstCombinePHI.cpp:752-755`
  - `NewLI = new LoadInst(...)`
  - `NewLI->copyMetadata(*FirstLI);`            <-- blind copy from FirstLI
  - `combineMetadataForCSE(NewLI, LI, true);`   <-- per-other-LI merge
- `llvm/lib/Transforms/Utils/Local.cpp:3059-3067` (invariant.group merge)
- Acknowledged FIXME at `Local.cpp:3063`: cannot represent merged invariant.group;
  but the fix in combineMetadata only ADDS J's, never CLEARS K's.

## Reproducer

```llvm
target datalayout = "e-m:e-p:64:64-i64:64-f80:128-n8:16:32:64-S128"
target triple = "x86_64-unknown-linux-gnu"

define i32 @test(i1 %c, ptr %p1, ptr %p2) {
entry:
  br i1 %c, label %bb1, label %bb2
bb1:
  %a = load i32, ptr %p1, align 4, !invariant.group !0
  br label %end
bb2:
  %b = load i32, ptr %p2, align 4
  br label %end
end:
  %p = phi i32 [ %a, %bb1 ], [ %b, %bb2 ]
  ret i32 %p
}
!0 = !{!"vt"}
```

## Diff: `opt -passes=instcombine -S`

Before:
```
bb1:  %a = load i32, ptr %p1, align 4, !invariant.group !0
bb2:  %b = load i32, ptr %p2, align 4                       ; NO invariant.group
end:  %p = phi i32 [ %a, %bb1 ], [ %b, %bb2 ]
```

After (BUG -- merged load incorrectly carries !invariant.group):
```
end:
  %p.in = phi ptr [ %p1, %bb1 ], [ %p2, %bb2 ]
  %p = load i32, ptr %p.in, align 4, !invariant.group !0   ; LEAKED from bb1
```

Expected: the merged load should NOT carry `!invariant.group`, because not every
predecessor's load did.

## Why this is a miscompile (not just a missed-opt)

`!invariant.group` is a SEMANTIC promise (not a hint). LLVM IR Reference:
> The existence of the invariant.group metadata on the instruction tells the
> optimizer that every load and store to the same pointer operand can be
> assumed to load or store the same value.

Down-stream code (GVN-style invariant load CSE; the strip/launder/.barrier
optimizations) is permitted to replace this merged load with another invariant
load on the same logical pointer object. But for the `bb2` predecessor the
loaded value has no such guarantee -- if the underlying object stored a
different value at the `p2` location, the optimization-substituted load would
give a different result than the dynamically reachable original.

## Risk / scope

Conservative path; usually no consumer of `!invariant.group` is triggered unless
C++-like vtable code is present. Still a latent miscompile that hides until a
later pass aggressively uses the metadata.

## Fix sketch

In `combineMetadata` (and/or the wrapping `combineMetadataForCSE`), when
`DoesKMove` and `!JMD`, clear K's invariant.group:

```cpp
auto *JMD = J->getMetadata(LLVMContext::MD_invariant_group);
if (isa<LoadInst>(K) || isa<StoreInst>(K)) {
  if (JMD) K->setMetadata(LLVMContext::MD_invariant_group, JMD);
  else if (DoesKMove) K->setMetadata(LLVMContext::MD_invariant_group, nullptr);
}
```

Alternative: clear in `foldPHIArgLoadIntoPHI` before the loop (forbid
invariant.group on the sunk load entirely, since we cannot prove it on every
path).
