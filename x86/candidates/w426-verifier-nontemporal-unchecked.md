# w426 — Verifier accepts ill-formed !nontemporal metadata

Severity: medium (well-formedness invariant; downstream consumers can be tricked).

## Summary

The LangRef requires that `!nontemporal` metadata on `load`/`store` "must
reference a single metadata name `<index>` corresponding to a metadata node
with one `i32` entry of value 1." (see
`llvm/docs/LangRef.rst:11934-11936` for load and `:12078-12080` for store).

`Verifier::visitInstruction` validates a long list of well-known metadata kinds
(`!nonnull`, `!align`, `!range`, `!invariant.group`, `!dereferenceable`, …) at
`llvm/lib/IR/Verifier.cpp:5848-5899`, but `MD_nontemporal` is **never
mentioned**:

```
$ grep -n nontemporal llvm/lib/IR/Verifier.cpp
(no output)
```

Consequently any tuple shape passes: zero operands, multiple operands, string
operands, wrong integer type, value other than 1, etc. The bitcode round-trip
preserves the bogus node verbatim, so downstream consumers (codegen, MIR
selection, runtime instrumentation passes) inherit it.

## Reproducer

`nontemp.ll`:

```llvm
target datalayout = "e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-f80:128-n8:16:32:64-S128"
target triple = "x86_64-unknown-linux-gnu"

define i32 @f(ptr %p) {
  %v = load i32, ptr %p, align 4, !nontemporal !0
  ret i32 %v
}
define void @g(ptr %p, i32 %v) {
  store i32 %v, ptr %p, align 4, !nontemporal !1
  ret void
}
!0 = !{}                          ; empty (LangRef requires {i32 1})
!1 = !{!"hi", !"there", i64 999}  ; strings + wrong-typed integer
```

```
$ opt nontemp.ll -S
; ModuleID = 'nontemp.ll'
...
  %v = load i32, ptr %p, align 4, !nontemporal !0
  store i32 %v, ptr %p, align 4, !nontemporal !1
...
!0 = !{}
!1 = !{!"hi", !"there", i64 999}
```

Verifier reports no error. `opt -O2` likewise accepts. Bitcode round-trip
(`opt -o tmp.bc` then `opt tmp.bc -S`) preserves the malformed node verbatim.

For comparison, the analogous `!nonnull` node is validated:

```
5853    if (MDNode *MD = I.getMetadata(LLVMContext::MD_nonnull)) {
5854      Check(I.getType()->isPointerTy(), "nonnull applies only to pointer types", &I);
5855      ...
5860      Check(MD->getNumOperands() == 0, "nonnull metadata must be empty", &I);
5861    }
```

## Suggested fix

In `Verifier::visitInstruction` (around `Verifier.cpp:5848`), add:

```cpp
if (MDNode *MD = I.getMetadata(LLVMContext::MD_nontemporal)) {
  Check(isa<LoadInst>(I) || isa<StoreInst>(I) || isa<MemTransferInst>(I),
        "nontemporal metadata applies only to load/store/memcpy/memmove", &I);
  Check(MD->getNumOperands() == 1, "nontemporal metadata must have exactly one operand", &I);
  auto *CI = mdconst::dyn_extract<ConstantInt>(MD->getOperand(0));
  Check(CI && CI->getType()->isIntegerTy(32) && CI->getZExtValue() == 1,
        "nontemporal metadata operand must be the i32 constant 1", &I);
}
```
