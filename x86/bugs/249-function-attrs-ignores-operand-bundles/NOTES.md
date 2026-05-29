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

---

## CORRECTION (re-audit at HEAD) — NOT A BUG

The premise that an unknown operand bundle "can have arbitrary effects
(allocate, free, synchronize, throw, fail to return)" is **wrong**. LangRef
§"Operand Bundle Semantics" bounds an *unknown* operand bundle to exactly:
(1) its operands escape in unknown ways, and (2) the call has unknown heap
read/write effects — and explicitly: "An operand bundle at a call site cannot
change the implementation of the called function."

So a bundle cannot make a callee unwind, free, synchronize, or fail to return
beyond what the callee itself does. Inferring `nounwind`/`nofree`/`nosync`/
`willreturn` through a bundled call is therefore sound — only the `memory`
attribute must be conservative (and `checkFunctionMemoryAccess` already bails on
`hasOperandBundles()`). The existing test `Transforms/FunctionAttrs/
operand-bundles-scc.ll` asserts exactly this behavior. Adding a blanket
`hasOperandBundles()` guard to the nofree/nosync/nounwind/willreturn predicates
regresses 4 in-tree tests. **WONTFIX.**
