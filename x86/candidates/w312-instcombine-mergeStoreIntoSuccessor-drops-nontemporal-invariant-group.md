# w312: InstCombine `mergeStoreIntoSuccessor` drops `!nontemporal`, `!invariant.group`, `!access_group`, `!mem_parallel_loop_access`, `!noalias_addrspace` on the merged store

## File / function

`llvm/lib/Transforms/InstCombine/InstCombineLoadStoreAlloca.cpp`,
`InstCombinerImpl::mergeStoreIntoSuccessor` (line 1631).

Lines 1735-1745:

```cpp
StoreInst *NewSI =
    new StoreInst(MergedVal, SI.getOperand(1), SI.isVolatile(), SI.getAlign(),
                  SI.getOrdering(), SI.getSyncScopeID());
InsertNewInstBefore(NewSI, BBI);
NewSI->setDebugLoc(MergedLoc);
NewSI->mergeDIAssignID({&SI, OtherStore});

// If the two stores had AA tags, merge them.
AAMDNodes AATags = SI.getAAMetadata();
if (AATags)
  NewSI->setAAMetadata(AATags.merge(OtherStore->getAAMetadata()));
```

## Root cause

The new `StoreInst` is constructed with no metadata. The function then
explicitly transfers only:

- debug location (line 1739)
- `!DIAssignID` (line 1740, via `mergeDIAssignID`)
- AA metadata (`!tbaa`, `!tbaa_struct`, `!alias_scope`, `!noalias`) at line 1745

All other store-applicable metadata kinds present on **both** source stores
are silently dropped:

- `!nontemporal`
- `!invariant.group`
- `!access_group`
- `!mem_parallel_loop_access`
- `!noalias_addrspace`
- `!prof`, `!fpmath`

Compare with `combineMetadata` (`Utils/Local.cpp:2934`) which has
intersect/union policies for each of these metadata kinds and is the
canonical helper for "I merged two equivalent instructions, set merged
metadata."

## Reproducer

```llvm
target datalayout = "e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-i128:128-f80:128-n8:16:32:64-S128"
target triple = "x86_64-unknown-linux-gnu"

define void @f(ptr %p, i1 %c) {
entry:
  br i1 %c, label %if, label %else
if:
  store i32 1, ptr %p, align 4, !nontemporal !0, !invariant.group !1
  br label %end
else:
  store i32 2, ptr %p, align 4, !nontemporal !0, !invariant.group !1
  br label %end
end:
  ret void
}

!0 = !{i32 1}
!1 = !{}
```

### `opt -passes=instcombine -S` produces

```llvm
define void @f(ptr %p, i1 %c) {
entry:
  br i1 %c, label %if, label %else

if:
  br label %end

else:
  br label %end

end:
  %storemerge = phi i32 [ 2, %else ], [ 1, %if ]
  store i32 %storemerge, ptr %p, align 4                ; both !nontemporal AND
                                                          ; !invariant.group GONE
  ret void
}
```

`!nontemporal` and `!invariant.group` (which were present on *both*
source stores, so by intersect-policy MUST be preserved) are both dropped.
Reproduced against
`/home/orenamd@semianalysis.com/FuzzX/amdgpu/build/llvm-fuzzer/bin/opt`.

## Why it matters

- `!invariant.group` loss is a real miscompile vector. The frontend (e.g.
  the C++ vptr machinery) emits `!invariant.group` on both arms of a
  conditional construct so that downstream loads can be safely
  forwarded. After merging in InstCombine, the merged store loses the
  marker. A subsequent GVN/loadCSE that sees a peer
  `load !invariant.group` from the same group address may now correctly
  treat the merged store as a barrier (it's not in any group) - missed
  opt - OR, if the markers reappear later via cloning, the inconsistency
  causes the same vptr-substitution miscompile documented in w106 /
  bug #177.

- `!nontemporal` loss converts the streaming-store hint from the frontend
  into regular store codegen, a measurable cache regression for the
  pattern `if (cond) nontemporal_store(p, 1); else nontemporal_store(p, 2);`.

- `!access_group` / `!mem_parallel_loop_access` loss removes the merged
  store from the parallel-loop accesses set; LoopVectorize / LICM may
  fail to vectorize the enclosing loop because the merged store is no
  longer in any access group.

## Fix shape

Replace the ad-hoc per-MD copies in lines 1739-1745 with a call to
`combineMetadataForCSE(NewSI, &SI, /*DoesKMove=*/true);` followed by
a second `combineMetadataForCSE(NewSI, OtherStore, /*DoesKMove=*/true);`,
or refactor to use `combineMetadata` directly on the union of both
source stores. (`mergeDIAssignID` and `setDebugLoc` would stay as
they are; the AA merge becomes part of the general helper.)

## Confidence

High (verified by reproducer). Net-new function not covered by any
existing w-prefixed candidate or filed bug under
`/home/orenamd@semianalysis.com/FuzzX/x86/bugs/`.
