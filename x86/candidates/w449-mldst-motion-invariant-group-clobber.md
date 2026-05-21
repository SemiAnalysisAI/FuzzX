# `MergedLoadStoreMotion::sinkStoresAndGEPs`: arbitrary `!invariant.group` clobber via `combineMetadataForCSE`

**Pass surface:** `mldst-motion`.
**Source:**
- `llvm/lib/Transforms/Scalar/MergedLoadStoreMotion.cpp` line 260: `combineMetadataForCSE(S0, S1, true);`
- `llvm/lib/Transforms/Utils/Local.cpp` lines 3059-3067 (invariant.group unconditional override):
```cpp
if (auto *JMD = J->getMetadata(LLVMContext::MD_invariant_group))
  if (isa<LoadInst>(K) || isa<StoreInst>(K))
    K->setMetadata(LLVMContext::MD_invariant_group, JMD);
```
**Triple:** `x86_64-unknown-linux-gnu`
**Tool:** `/home/orenamd@semianalysis.com/FuzzX/amdgpu/build/llvm-fuzzer/bin/opt -S -passes=mldst-motion`.

## Root cause

`MergedLoadStoreMotion` sinks two structurally-identical stores `S0` and `S1` from two predecessor blocks of a diamond into the diamond's tail. It calls `combineMetadataForCSE(S0, S1, /*DoesKMove=*/true)` to merge S1's metadata into S0, then clones S0 into the join block.

`combineMetadata`'s epilogue at Local.cpp line 3065 unconditionally overwrites S0's `!invariant.group` with S1's if S1 has it — even though the two stores were known-equivalent only at the value level (`S0` and `S1` were sunk because they store the same value to the same address). The two stores belonging to two *different* invariant.group identifiers (groupA on branch t, groupB on branch f) is sound program input — a front-end could emit it to model "after `launder.invariant.group`, the address is in a different group identity".

After mldst-motion, the surviving join-block store has tag groupB (or groupA, depending on which path was visited first). Whichever it is, ONE of the two predecessor invariant-group invariants is silently broken: an invariant-group-aware load on the surviving path that was previously equivalent to the surviving store now isn't.

## Reproducer

```llvm
target datalayout = "e-m:e-i64:64-f80:128-n8:16:32:64-S128"
target triple = "x86_64-unknown-linux-gnu"

define void @sink(i1 %c, ptr %p) {
entry:
  br i1 %c, label %t, label %f

t:
  store i32 1, ptr %p, align 4, !invariant.group !0
  br label %join

f:
  store i32 1, ptr %p, align 4, !invariant.group !1
  br label %join

join:
  ret void
}

!0 = !{!"groupA"}
!1 = !{!"groupB"}
```

```
$ opt -S -passes=mldst-motion repro.ll
```

After:
```
join:
  store i32 1, ptr %p, align 4, !invariant.group !0
  ret void
!0 = !{!"groupB"}
```

The sunk store now claims to be in groupB unconditionally — on the original `%t`-edge runtime path, the store was in groupA. Any downstream invariant-group-keyed load in `%t`'s domination (e.g., reached via a `launder.invariant.group` from `%entry`) that previously matched a groupA store now sees a groupB store and the invariant linkage is broken.

## Why this matters

`!invariant.group` is the mechanism C++ devirtualizers and LLVM-IR-level optimizers use to model vtable pointer identity across `launder.invariant.group` barriers. mldst-motion is a relatively early -O2 pass; collapsing two differently-tagged vptr-stores into one tagged store can cause subsequent GVN-invariant-group folding to incorrectly fold (or fail to fold) loads against the post-mldst surviving store.

## Pipeline

The IR survives `-O2`:
```
$ opt -S -O2 repro.ll  | grep -A1 'invariant.group'
```
shows the surviving single store carries one of the two original tags arbitrarily.

## Notes

- The mldst-motion-specific `canSinkStoresAndGEPs` check (MergedLoadStoreMotion.cpp:231) does NOT verify equality of invariant.group tags — only address equality. So the diamond is admitted, then the metadata merger silently picks J's tag.
- Same root-cause as w445 but a different pass and a different downstream impact (devirtualization correctness vs. EarlyCSE invariant.group equivalence).
- Fix: `MergedLoadStoreMotion::sinkStoresAndGEPs` could either (a) refuse to sink stores whose invariant.group tags differ, or (b) drop both invariant.group tags after merging, or (c) Local.cpp 3065-3067 could compare KMD vs JMD and drop on mismatch.
