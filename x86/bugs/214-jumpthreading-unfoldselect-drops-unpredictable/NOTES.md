# 214 — JumpThreading converts `select !unpredictable` to `br` without copying `!unpredictable`

Component: `llvm/lib/Transforms/Scalar/JumpThreading.cpp` (`tryToUnfoldSelectInCurrBB` ~line 2990-2992 and `unfoldSelectInstr` ~line 2794)

The entire file never references `MD_unpredictable`. When a `select` with `!unpredictable` is converted into a control-flow branch, only `MD_prof` is forwarded. The resulting `br i1` is bare — the branch-prediction hint that the user attached to the select is silently lost.

## Reproducer

`opt -passes=jump-threading -S repro.ll` produces:
```
  br i1 %phi, label %0, label %1     ; no !unpredictable
```

## Severity

Default x86 -O2. `!unpredictable` is a real codegen-affecting hint (controls branch prediction insertion); dropping it changes generated code on architectures that pay attention.

## Fix

In both `unfoldSelectInstr` and `tryToUnfoldSelectInCurrBB`, copy `MD_unpredictable` (and `MD_annotation`) alongside `MD_prof` to the new conditional branch.
