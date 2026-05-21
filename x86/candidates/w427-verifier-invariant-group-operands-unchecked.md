# w427 — Verifier does not check !invariant.group must reference an empty metadata

Severity: low/medium (well-formedness invariant; documented in LangRef but unenforced).

## Summary

`Verifier::visitInstruction` at `llvm/lib/IR/Verifier.cpp:5848-5851` validates
only that an instruction carrying `!invariant.group` is a load or store:

```cpp
5848  if (I.hasMetadata(LLVMContext::MD_invariant_group)) {
5849    Check(isa<LoadInst>(I) || isa<StoreInst>(I),
5850          "invariant.group metadata is only for loads and stores", &I);
5851  }
```

LangRef requires more (`llvm/docs/LangRef.rst:8508-8509`):

> The experimental ``invariant.group`` metadata may be attached to
> ``load``/``store`` instructions **referencing a single metadata with no
> entries**.

The verifier does not enforce the "no entries" condition. Crafted IR with
arbitrary operands in the referenced node is silently accepted and round-trips
through bitcode verbatim. Contrast `!nonnull` and `!noundef`, both of which
enforce `MD->getNumOperands() == 0` (e.g.
`llvm/lib/IR/Verifier.cpp:5860` for nonnull).

## Reproducer

`badinvariant.ll`:

```llvm
target datalayout = "e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-f80:128-n8:16:32:64-S128"
target triple = "x86_64-unknown-linux-gnu"

define i32 @load_ig(ptr %p) {
  %v = load i32, ptr %p, !invariant.group !0
  ret i32 %v
}

!0 = !{i32 1, i32 2, !"foo"}   ; per LangRef this should be !{}
```

```
$ opt badinvariant.ll -S
; ModuleID = 'badinvariant.ll'
...
  %v = load i32, ptr %p, align 4, !invariant.group !0
...
!0 = !{i32 1, i32 2, !"foo"}
```

No diagnostic.

## Suggested fix

In `Verifier::visitInstruction`, extend the existing block:

```cpp
if (MDNode *MD = I.getMetadata(LLVMContext::MD_invariant_group)) {
  Check(isa<LoadInst>(I) || isa<StoreInst>(I),
        "invariant.group metadata is only for loads and stores", &I);
  Check(MD->getNumOperands() == 0, "invariant.group metadata must be empty",
        &I);
}
```

## Why this matters for fuzzing

Crafted bitcode encoding a non-empty `!invariant.group` node can survive into
later passes. The pointer-equality reasoning around `invariant.group` is keyed
purely by `MDNode` identity (`getMetadata(...)`), so passing nonsense operands
inside the node does not corrupt analysis — but it widens the surface for
divergent identity (i.e. two textually-different but semantically equivalent
nodes are deduplicated by content; a malformed node can be made deliberately
unique, defeating that). This is the kind of soft invariant bitcode-mutation
fuzzers can leverage to provoke divergent behavior between text/bitcode paths
and across optimization levels.
