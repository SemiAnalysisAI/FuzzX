# w111 -- SimplifyCFG `mergeConditionalStores` silently drops `!nontemporal` from one of the paired stores

## Component / Location

`llvm/lib/Transforms/Utils/SimplifyCFG.cpp` --
`mergeConditionalStoreToAddress` (~line 4260), inside the default
`mergeConditionalStores` path (line 4434) reachable in the **default
-O2 `simplifycfg<>` pipeline**:
`bonus-inst-threshold=1;no-forward-switch-cond;switch-range-to-icmp;
no-switch-to-arithmetic;no-switch-to-lookup;keep-loops;
no-hoist-common-insts;no-hoist-loads-stores-with-cond-faulting;
no-sink-common-insts;speculate-blocks;simplify-cond-branch;
no-speculate-unpredictables`.

The merge first calls `combineMetadataForCSE(QStore, PStore, /*KMove=*/true)`
(line 4409), then immediately overrides `SI->copyMetadata(*QStore)` (line
4410). Inside `combineMetadata` (`llvm/lib/Transforms/Utils/Local.cpp:3030`):

```cpp
case LLVMContext::MD_nontemporal:
  // Preserve !nontemporal if it is present on both instructions.
  if (!AAOnly)
    K->setMetadata(Kind, JMD);
  break;
```

When the two stores disagree (only `QStore` carries `!nontemporal`), the
metadata on `QStore` is unconditionally cleared because `JMD` (PStore's)
is null. `SI->copyMetadata(*QStore)` then copies the cleared state to the
merged store. The nontemporal hint is silently dropped for the path that
originally carried it.

This is the same family as w105 (hoistCommonCodeFromSuccessors), w120/121
(sink/hoist common volatile loads/stores), w76 (memcpyopt), w75 (DSE) --
but those are gated behind options that are **off** in the default O2
pipeline, whereas `mergeConditionalStores` is on by default (see also
w61, which covers the analogous `atomic` drop in the same call site).

## Repro

`/tmp/repro_mcs_nontemp_one.ll`:

```llvm
target triple = "x86_64-unknown-linux-gnu"
target datalayout = "e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-i128:128-f80:128-n8:16:32:64-S128"

define void @merge_nontemp_q(ptr %p, i1 %c1, i1 %c2) {
entry:
  br i1 %c1, label %if.then, label %if.end

if.then:
  store i32 100, ptr %p, align 4
  br label %if.end

if.end:
  br i1 %c2, label %if.then2, label %if.end2

if.then2:
  store i32 200, ptr %p, align 4, !nontemporal !0
  br label %if.end2

if.end2:
  ret void
}

!0 = !{i32 1}
```

## Invocation

```
opt -passes='simplifycfg<bonus-inst-threshold=1;no-forward-switch-cond;switch-range-to-icmp;no-switch-to-arithmetic;no-switch-to-lookup;keep-loops;no-hoist-common-insts;no-hoist-loads-stores-with-cond-faulting;no-sink-common-insts;speculate-blocks;simplify-cond-branch;no-speculate-unpredictables>' -S repro.ll
```

## Output

```llvm
define void @merge_nontemp_q(ptr %p, i1 %c1, i1 %c2) {
entry:
  %spec.select = select i1 %c2, i32 200, i32 100
  %0 = or i1 %c1, %c2
  br i1 %0, label %1, label %2

1:
  store i32 %spec.select, ptr %p, align 4        ; <-- !nontemporal dropped!
  br label %2

2:
  ret void
}
```

For the `c2=true, c1=false` path the original program executed a single
`store !nontemporal`, asking the hardware to bypass the cache. After
merge the merged store no longer carries `!nontemporal`, so codegen lowers
a plain `movl`/`mov` rather than the streaming variant
(`movntdq`/`movntps`). The cache footprint on that path is now strictly
worse than before, and the program's explicit cache-bypass hint is
unrecoverable downstream.

While `!nontemporal` is a hint (no UB if dropped), this is an asymmetric
silent loss that triggers in the **default O2** SimplifyCFG invocation,
and is identical in spirit to w61's atomicity drop in the same call site.

## Recommended fix

Either (a) refuse to merge if exactly one of `PStore`/`QStore` carries
`MD_nontemporal`, or (b) make `mergeConditionalStoreToAddress` materialise
a `!nontemporal` on the merged store whenever **either** input carried
it. (Option (b) is the more conservative choice: the new store is gated
on the union of the two conditions, so promoting the hint upward never
introduces nontemporal writes on a path that previously had none -- the
merged store only executes when at least one of the originals would
have.)
