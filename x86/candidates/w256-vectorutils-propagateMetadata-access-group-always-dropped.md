# w256: `llvm::propagateMetadata` always drops `!llvm.access.group` (called from every SLPVectorizer load/store/binop combine)

## Pass
`-passes=slp-vectorizer` (default x86 -O2 pipeline). The buggy helper lives in
`llvm/lib/Analysis/VectorUtils.cpp` but its only effect in -O2 is via SLP — LV
uses a different path (it appends to the existing access group via
`uniteAccessGroups`).

## Summary

`llvm::propagateMetadata(NewInst, VL)` is called by SLPVectorizer all over the
place to attach intersected metadata to a freshly created vector load / store /
binop / shuffle / cast / etc. (See e.g. SLPVectorizer.cpp:22698, 23030, 23032,
23163, 23238, 23294, 23319, 23423, 23492, 23552.)

The helper seeds `MD` from `VL[0]`'s metadata, then walks the rest of `VL` and
intersects each kind into `MD`. For most kinds the intersect operand is the
running `MD` itself:

```cpp
case LLVMContext::MD_tbaa:
  MD = MDNode::getMostGenericTBAA(MD, IMD);   // MD on the left ✓
  break;
case LLVMContext::MD_noalias:
case LLVMContext::MD_nontemporal:
case LLVMContext::MD_invariant_load:
  MD = MDNode::intersect(MD, IMD);            // MD on the left ✓
  break;
```

The `MD_access_group` case is different:

```cpp
case LLVMContext::MD_access_group:
  MD = intersectAccessGroups(Inst, IJ);       // Inst (the new vector inst!)
  break;
```

`Inst` is the **brand-new** vector instruction; it has no metadata yet (the
single `Inst->setMetadata(Kind, MD)` happens only once, **after** the inner
loop, at line 1114). So `Inst->getMetadata(MD_access_group)` returns null on
every iteration, and `intersectAccessGroups` returns null on its first
invocation:

```cpp
MDNode *llvm::intersectAccessGroups(const Instruction *Inst1,
                                    const Instruction *Inst2) {
  bool MayAccessMem1 = Inst1->mayReadOrWriteMemory();
  bool MayAccessMem2 = Inst2->mayReadOrWriteMemory();
  if (!MayAccessMem1 && !MayAccessMem2) return nullptr;
  if (!MayAccessMem1) return Inst2->getMetadata(MD_access_group);
  if (!MayAccessMem2) return Inst1->getMetadata(MD_access_group);
  MDNode *MD1 = Inst1->getMetadata(MD_access_group);
  MDNode *MD2 = Inst2->getMetadata(MD_access_group);
  if (!MD1 || !MD2) return nullptr;   // <-- MD1 is always null when Inst1 is the new vec inst
  ...
}
```

For a load/store, `MayAccessMem1` is true (new vector load/store DOES access
memory), so we fall through to the `MD1 = Inst1->getMetadata(...)` line, which
is null on the new instruction → returns nullptr → `MD` becomes null → the
outer `for (... ; MD && J != E; ...)` exits → `Inst->setMetadata(MD_access_group, nullptr)`
is a no-op → access group is dropped.

This means **every** vector load/store that SLP emits loses its
`!llvm.access.group`, even when every scalar in the bundle shares the same
group. That defeats subsequent loop-vectorize / parallel-loop annotations and
LICM/CSE/scheduling decisions that rely on the parallel-loop semantics
(`isParallelAccess`).

## Source (LLVM 23.0.0git, `llvm/lib/Analysis/VectorUtils.cpp`)

```cpp
// 1083  for (int J = 1, E = VL.size(); MD && J != E; ++J) {
// 1084    const Instruction *IJ = cast<Instruction>(VL[J]);
// 1085    MDNode *IMD = IJ->getMetadata(Kind);
// ...
// 1106    case LLVMContext::MD_access_group:
// 1107      MD = intersectAccessGroups(Inst, IJ);   // BUG: should be (MD-as-instruction, IJ) or use a different intersection helper
// 1108      break;
// ...
// 1114  Inst->setMetadata(Kind, MD);
```

Sister cases (TBAA, alias_scope, fpmath, noalias, nontemporal, invariant_load,
mmra) all chain through `MD`. The access-group case is the only one that
re-reads from `Inst`, which is the empty vector instruction.

## Reproducer

`t_ag.ll`:
```llvm
target datalayout = "e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-i128:128-f80:128-n8:16:32:64-S128"
target triple = "x86_64-unknown-linux-gnu"

define void @f(ptr %p, ptr %q) {
  %p0 = getelementptr float, ptr %p, i32 0
  %p1 = getelementptr float, ptr %p, i32 1
  %q0 = getelementptr float, ptr %q, i32 0
  %q1 = getelementptr float, ptr %q, i32 1
  %a = load float, ptr %q0, !llvm.access.group !1
  %b = load float, ptr %q1, !llvm.access.group !1
  store float %a, ptr %p0, !llvm.access.group !2
  store float %b, ptr %p1, !llvm.access.group !2
  ret void
}

!1 = distinct !{}
!2 = distinct !{}
```

Command:
```
opt -passes=slp-vectorizer -S t_ag.ll
```

Output (relevant excerpt):
```llvm
define void @f(ptr %p, ptr %q) {
  %p0 = getelementptr float, ptr %p, i32 0
  %q0 = getelementptr float, ptr %q, i32 0
  %1 = load <2 x float>, ptr %q0, align 4         ; <-- !llvm.access.group !0 dropped
  store <2 x float> %1, ptr %p0, align 4          ; <-- !llvm.access.group !1 dropped
  ret void
}
```

Both scalar loads and both scalar stores carried the same access group; the
correct intersection is `!1` (and `!2` respectively). The vectorized form has
neither.

A control test confirms this is not generic SLP behaviour:

```
opt -passes=slp-vectorizer -S /tmp/single-load.ll
; %a = load float, ptr %q, !llvm.access.group !0   -- preserved
```

A single load is left untouched and keeps the metadata. The bug only triggers
when `propagateMetadata` is invoked, i.e. whenever ≥ 2 scalars are merged into
a vector instruction.

## Why this matters

`!llvm.access.group` is the IR mechanism by which the front-end (or LV /
loop-distribute) declares that a memory access belongs to a parallel-loop
access group. Downstream consumers:

- `Loop::isAnnotatedParallel()` / `MemoryDepChecker::isInSameAccessGroup`
- `LoopAccessAnalysis::analyzeLoop` (chooses runtime checks vs. asserting
  no-deps)
- LICM / GVN cross-iteration reasoning

Dropping the group means a parallel-loop body that was vectorized by SLP loses
the parallel-loop guarantee, and the surrounding loop is downgraded to "may
have inter-iteration deps" on the next pass. This is a real
missed-optimization regression — and in some patterns (where another pass
re-runs LV after SLP) it changes whether a loop vectorizes at all.

## Suggested fix

```cpp
case LLVMContext::MD_access_group:
-  MD = intersectAccessGroups(Inst, IJ);
+  // Intersect against the running MD, not against Inst (which is the
+  // freshly created vector inst and has no metadata yet).
+  if (MD && IMD)
+    MD = intersectAccessGroups(MD, IMD); // overload that takes MDNode*, or
+                                         // factor the body to take MDNodes
+  else
+    MD = nullptr;
  break;
```

`intersectAccessGroups` currently takes two `Instruction *` because it special-cases
`mayReadOrWriteMemory()`. The MDNode-only flavour is what `propagateMetadata`
needs here — the `mayAccessMem` short-circuit makes no sense when both
operands describe the same new vector instruction.
