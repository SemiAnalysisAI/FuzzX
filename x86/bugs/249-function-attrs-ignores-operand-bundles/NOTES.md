# 249 — FunctionAttrs `InstrBreaksNoFree`/`InstrBreaksNoSync`/`InstrBreaksNonThrowing` ignore unknown operand bundles → unsound `nofree`/`nosync`/`nounwind` inference

Component: `llvm/lib/Transforms/IPO/FunctionAttrs.cpp` lines 1903-1948

The three per-instruction predicates short-circuit when the called function carries `nofree`/`nosync`/`nounwind`, using `CallBase::hasFnAttr(...)`. This API does NOT account for operand bundles. Per LangRef, unknown operand bundles can have **arbitrary effects** (allocate, free, synchronize, throw, fail to return).

Sibling `addMemoryAttrs`/`checkFunctionMemoryAccess` at line 204 *correctly* bails when `Call->hasOperandBundles()` is true. The three predicates above forget to.

`Instruction::willReturn()` (`Instruction.cpp:1294-1302`) has the same hole, consumed by `functionWillReturn` at `FunctionAttrs.cpp:2207`.

## Reproducer

```ll
declare void @leaf() nofree nosync nounwind willreturn
define void @caller() {
  call void @leaf() [ "side_effects"() ]
  ret void
}
```

`opt -passes=function-attrs -S`:
- `@leaf`: `Function Attrs: nofree nosync nounwind willreturn` (correct, declared)
- `@caller`: `Function Attrs: mustprogress nofree nosync nounwind willreturn` (UNSOUND — the `"side_effects"` bundle can do any of those)

## Severity

Default x86 -O2. Unsound inference: downstream passes can move, eliminate, or duplicate `@caller` invocations based on inferred attrs that the bundle was specifically meant to suppress.

## Fix

Add `if (CB->hasOperandBundles()) return true;` (or specifically check `!CB->doesNotFreeMemory()`/`!CB->hasReadingOperandBundles()` analogues) at the head of each of `InstrBreaksNoFree`, `InstrBreaksNoSync`, `InstrBreaksNonThrowing`. Also fix `Instruction::willReturn()` to bail on unknown bundles.
