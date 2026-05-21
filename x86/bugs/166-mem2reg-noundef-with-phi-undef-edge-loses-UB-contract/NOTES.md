# w101 mem2reg: `!noundef` load loses immediate-UB contract when promoted into a PHI that has an `undef` incoming on the uninitialized path

## Location

- `llvm/lib/Transforms/Utils/PromoteMemoryToRegister.cpp:1182-1199`
  (multi-block `RenamePass` rewrite path)
- `convertMetadataToAssumes` at line 500.

## Bug

When mem2reg promotes an alloca whose dominator analysis says a load is
not always preceded by a store (one or more incoming edges leave the
slot uninitialized), it creates a PHI whose value-incoming on the
uninitialized predecessor is `undef`, then RAUW's the load with the
PHI.

The relevant rewrite snippet (`PromoteMemoryToRegister.cpp:1182-1199`):

```cpp
if (LoadInst *LI = dyn_cast<LoadInst>(I)) {
  ...
  Value *V = IncomingVals[AI->second];      // V == the phi created above
  convertMetadataToAssumes(LI, V, SQ.DL, AC, &DT);
  LI->replaceAllUsesWith(V);
  LI->eraseFromParent();
}
```

`convertMetadataToAssumes` (line 500) only injects the
non-terminator-unreachable UB marker when the replacement value is a
**literal** `UndefValue`:

```cpp
if (isa<UndefValue>(Val) && LI->hasMetadata(LLVMContext::MD_noundef)) {
  // Insert non-terminator unreachable.
  new StoreInst(ConstantInt::getTrue(Ctx),
                PoisonValue::get(PointerType::getUnqual(Ctx)),
                /*isVolatile=*/false, Align(1), LI->getIterator());
  return;
}
```

In the PHI case `Val` is the freshly inserted `PHINode*`, not
`UndefValue`, so the `isa<UndefValue>` test is false. The
`!noundef`-on-load contract — *"reaching this load implies the value is
defined; otherwise immediate UB"* — is silently discarded.

After mem2reg, the value flowing on the uninitialized edge is
unconstrained `undef`; nothing in the IR records that taking that edge
was UB. Downstream UB-exploiting transforms (e.g., propagating poison /
freezing the PHI under `nofreeze`-style reasoning, marking the edge as
dead) can no longer fire.

## Reproducer

```ll
; mem2reg.noundef.phi.ll
define i32 @f(i1 %c, i32 %x) {
entry:
  %a = alloca i32, align 4
  br i1 %c, label %t, label %m
t:
  store i32 %x, ptr %a, align 4
  br label %m
m:
  %v = load i32, ptr %a, align 4, !noundef !0
  ret i32 %v
}
!0 = !{}
```

```text
$ opt -passes='mem2reg' mem2reg.noundef.phi.ll -S
define i32 @f(i1 %c, i32 %x) {
entry:
  br i1 %c, label %t, label %m

t:
  br label %m

m:
  %a.0 = phi i32 [ %x, %t ], [ undef, %entry ]
  ret i32 %a.0
}
```

Before mem2reg the program has immediate UB on `entry -> m` (load
returns undef despite `!noundef`). After mem2reg the IR no longer
encodes that fact:

- No `store i1 true, ptr poison` is inserted in `entry` (or anywhere on
  the path that reaches the PHI through the undef edge).
- The PHI has no `noundef` attribute (PHIs cannot carry per-edge
  `noundef`).
- The fall-through is just plain `undef`.

The single-store / single-block paths (lines 620 and 727) have the same
problem when their `ReplVal` is a `UndefValue::get(LI->getType())` *not*
arrived at by direct literal `UndefValue`, but those paths happen to pass
the literal undef. The multi-block PHI path is the broken one.

## Why this isn't trivially fixed by RAUW

The mem2reg machinery already constructs the PHI before walking the load
out — and it has to, because the PHI is the only value SSA-flowing into
the load's user list across blocks. The fix is to recognize that *any*
incoming value to the PHI that is `UndefValue` represents an entry edge
that, in the source IR, would have executed a `!noundef` load. mem2reg
should either:

1. Insert the non-terminator unreachable on each predecessor edge that
   contributes an `UndefValue` to the PHI, **before** the branch into
   the load's block (i.e., in the predecessor's terminator-region), or
2. Replace the `UndefValue` incoming with a `freeze poison` and let the
   `freeze` carry forward the missing-defined-ness fact (this only
   converts immediate-UB to deferred-poison — a refinement, not a fix).

Option (1) preserves the original UB contract bit-for-bit.

## Notes

- IR-level only; no codegen reproducer because the issue is the
  contract going missing, not the lowering.
- The same hole exists in SROA (which calls the same promotion code).
- The sibling case where `convertMetadataToAssumes` *does* fire
  correctly is exercised in the trivial single-block / no-stores path
  (`promoteSingleBlockAlloca`, line 716), where `ReplVal` is set to
  `UndefValue::get(LI->getType())` directly. The asymmetry between the
  single-block path (correct UB injection) and the multi-block PHI path
  (silent drop) is the actual bug.
