# w111 -- SimplifyCFG `mergeConditionalStores` spreads `!invariant.group` from one store to the merged store, with the **other** store's value

## Component / Location

`llvm/lib/Transforms/Utils/SimplifyCFG.cpp` --
`mergeConditionalStoreToAddress` (~line 4408-4410), reachable through
the **default -O2 `simplifycfg<>` pipeline** (`mergeConditionalStores`
is on whenever `MergeCondStores=true`, the default; none of the
`<hoist|sink-common-insts>` options are required).

The relevant sequence is:

```cpp
StoreInst *SI = cast<StoreInst>(QB.CreateStore(QPHI, Address));
combineMetadataForCSE(QStore, PStore, true);   // line 4409
SI->copyMetadata(*QStore);                     // line 4410
```

`combineMetadataForCSE` calls `combineMetadata` in
`llvm/lib/Transforms/Utils/Local.cpp:2934`. For `MD_invariant_group`
there is the in-switch `break` ("Preserve !invariant.group in K"), but
*after* the switch, the helper unconditionally copies J's metadata onto
K when J has the tag (line 3065-3067):

```cpp
if (auto *JMD = J->getMetadata(LLVMContext::MD_invariant_group))
  if (isa<LoadInst>(K) || isa<StoreInst>(K))
    K->setMetadata(LLVMContext::MD_invariant_group, JMD);
```

So if only **PStore** carries `!invariant.group`, the helper writes
PStore's tag onto QStore, then `SI->copyMetadata(*QStore)` re-copies it
onto the merged store `SI`. The merged store -- whose value is
`select(QCond, QStoreVal, PStoreVal)` -- now carries `!invariant.group`
on the QCond-true path, storing QStore's value with a tag that QStore
never had.

Per LangRef
(`llvm/docs/LangRef.rst:md_invariant.group`):

> The existence of the `invariant.group` metadata on the instruction
> tells the optimizer that every `load` and `store` to the same pointer
> operand can be assumed to load or store the same value [...]

If a different earlier store in the same group writes value V1 and the
merged store now writes V2 (= QStore's value) with the same group, the
program becomes self-contradictory under the `invariant.group` model:
downstream invariant-group-aware loads are entitled to assume V1, but
the merged store has written V2.

## Repro

`/tmp/t_mcs_invgroup_miscompile.ll`:

```llvm
target triple = "x86_64-unknown-linux-gnu"
target datalayout = "e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-i128:128-f80:128-n8:16:32:64-S128"

define i32 @miscompile(ptr %p, i1 %c1, i1 %c2) {
entry:
  ; Establish invariant.group !0 with value 100.
  store i32 100, ptr %p, !invariant.group !0
  br label %if.start

if.start:
  br i1 %c1, label %if.then, label %if.end

if.then:
  ; PStore -- in the same invariant group, also stores 100 (consistent).
  store i32 100, ptr %p, align 4, !invariant.group !0
  br label %if.end

if.end:
  br i1 %c2, label %if.then2, label %if.end2

if.then2:
  ; QStore -- NOT in the invariant group. Source intent: this is a
  ; deliberate out-of-group write of a different value.
  store i32 200, ptr %p, align 4
  br label %if.end2

if.end2:
  ; This load is invariant-group-aware.
  %v = load i32, ptr %p, !invariant.group !0
  ret i32 %v
}

!0 = !{}
```

## Invocation

```
opt -passes='simplifycfg<bonus-inst-threshold=1;no-forward-switch-cond;switch-range-to-icmp;no-switch-to-arithmetic;no-switch-to-lookup;keep-loops;no-hoist-common-insts;no-hoist-loads-stores-with-cond-faulting;no-sink-common-insts;speculate-blocks;simplify-cond-branch;no-speculate-unpredictables>' -S repro.ll
```

## Output

```llvm
define i32 @miscompile(ptr %p, i1 %c1, i1 %c2) {
entry:
  store i32 100, ptr %p, align 4, !invariant.group !0
  %spec.select = select i1 %c2, i32 200, i32 100
  %0 = or i1 %c1, %c2
  br i1 %0, label %1, label %2

1:
  store i32 %spec.select, ptr %p, align 4, !invariant.group !0   ; <-- !invariant.group spuriously added!
  br label %2

2:
  %v = load i32, ptr %p, align 4, !invariant.group !0
  ret i32 %v
}
```

The merged conditional store now carries `!invariant.group !0` (inherited
from PStore via `combineMetadata` lines 3065-3067), but on the
`%c2=true` path it stores **200** -- a value that the original program
never wrote with `!invariant.group !0`. The original `if.then2` store of
200 was deliberately *outside* the group, marking this address as having
just left the invariant regime; after the merge, that intent is lost.

Concretely, the IR is now ill-formed against its own
`!invariant.group` contract: the entry store says "the value at `%p`
within group `!0` is 100" and the merged store says "the value at `%p`
within group `!0` is 200" on the same dynamic path (c1=*, c2=true). An
invariant-group-aware optimizer is free to constant-fold the final load
to either 100 or 200 (or anything), at its discretion -- a latent
miscompile waiting on the next pass that exploits the contract.

This mirrors w61 (atomic drop in the same call site) and the broader
"write through CreateStore + copyMetadata" family in SimplifyCFG, but
the failure here is the *opposite* direction: the merge adds metadata to
the new store that **one of the two source stores never had**, falsely
strengthening the asserted invariant.

## Recommended fix

Either (a) refuse to merge if exactly one of `PStore`/`QStore` carries
`MD_invariant_group`, or (b) explicitly clear `MD_invariant_group` on
the merged store (`SI`) when the two inputs disagree, e.g. before the
`SI->copyMetadata(*QStore)` line. Option (a) is preferable because the
invariant-group contract is value-precise: if only PStore was in the
group, we cannot prove that QStore's value belongs to it.

A narrower fix is to remove the unconditional propagation block in
`combineMetadata` (`Local.cpp:3065-3067`) and instead require *both* J
and K to carry `MD_invariant_group` (with equal MDNodes) before
preserving it -- analogous to how `MD_nontemporal` and `MD_nosanitize`
are already handled.
