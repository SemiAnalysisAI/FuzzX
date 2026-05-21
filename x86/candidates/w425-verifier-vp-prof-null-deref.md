# w425 — Verifier null-deref on malformed VP !prof metadata

Severity: high (crash; assert-equivalent in release; reachable via bitcode).

## Summary

`Verifier::visitProfMetadata` validates `!prof` metadata of kind `"VP"`
(value-profile). After confirming `isValueProfileMD(MD)` is true, the loop at
Verifier.cpp:5432-5438 walks the odd-indexed "value" operands and immediately
calls `ProfileValue->getZExtValue()` on the result of
`mdconst::dyn_extract<ConstantInt>(MD->getOperand(I))` without checking the
return for null. `mdconst::dyn_extract` returns `nullptr` whenever the operand
is not a `ConstantInt` (including the cases of an `MDString` operand or an
explicit `null` operand), so the next line dereferences a null pointer and the
verifier crashes.

`isValueProfileMD` only validates: (a) >= 5 operands, (b) operand 0 is the
`MDString "VP"`. It does **not** validate that the value/count operands at odd
indices >= 3 are `ConstantInt` — yet the verifier code immediately assumes they
are.

## Source

File: `llvm/lib/IR/Verifier.cpp`

```cpp
5431    DenseSet<uint64_t> ProfileValues;
5432    for (unsigned I = 3; I < MD->getNumOperands(); I += 2) {
5433      ConstantInt *ProfileValue =
5434          mdconst::dyn_extract<ConstantInt>(MD->getOperand(I));
5435      uint64_t ProfileValueInt = ProfileValue->getZExtValue();   // <-- null deref
5436      auto [ValueIt, Inserted] = ProfileValues.insert(ProfileValueInt);
5437      Check(Inserted, "VP !prof should not have duplicate profile values", MD);
5438    }
```

Compare to the well-formed branch a few lines up (5404-5410), which validates
each `branch_weights` operand with `Check(MDO, ...)` and
`Check(mdconst::dyn_extract<ConstantInt>(MDO), ...)` *before* using it. The VP
path has no such guard.

`isValueProfileMD` definition: `llvm/lib/IR/ProfDataUtils.cpp:132` calls
`isTargetMD(ProfileData, "VP", MinVPOps=5)` at
`llvm/lib/IR/ProfDataUtils.cpp:54-70`. It only checks operand 0 is the
`MDString "VP"` and that there are >= 5 operands.

## Reproducer

`vp_prof_null.ll`:

```llvm
target datalayout = "e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-f80:128-n8:16:32:64-S128"
target triple = "x86_64-unknown-linux-gnu"

declare void @callee()

define void @f() {
  call void @callee(), !prof !0
  ret void
}

; VP record:
;   operand 0: "VP" (string)
;   operand 1: kind (i32 0 = IndirectCallTarget)
;   operand 2: total count (i64 100)
;   operand 3+: pairs of (value, count). Make operand 3 a string instead of a ConstantInt.
!0 = !{!"VP", i32 0, i64 100, !"oops", i64 50}
```

```
$ opt vp_prof_null.ll -S
PLEASE submit a bug report ...
Stack dump:
0.  Program arguments: opt vp_prof_null.ll -S
...
9  opt    llvm::verifyModule(llvm::Module const&, llvm::raw_ostream*, bool*) + 84
```

Also reproduces with `-O2` and with the analogous explicit-`null` operand
(`!"VP", i32 0, i64 100, null, i64 50`).

### Bitcode path

The bitcode reader does not perform this validation either, so a fuzzed bitcode
file or an `opt -disable-verify ... -o file.bc` followed by re-running the
verifier on `file.bc` ICEs the same way (also confirmed). This means crafted
`.bc` files containing a malformed `VP` `!prof` node trigger a crash in any
downstream tool that runs the verifier.

## Suggested fix

Add a guard for `ProfileValue == nullptr` analogous to the branch_weights path:

```cpp
auto &MDO = MD->getOperand(I);
Check(MDO, "VP !prof value operand should not be null", MD);
ConstantInt *ProfileValue = mdconst::dyn_extract<ConstantInt>(MDO);
Check(ProfileValue, "VP !prof value operand is not a const int", MD);
if (!ProfileValue) continue;
uint64_t ProfileValueInt = ProfileValue->getZExtValue();
...
```

Optionally, also validate the count operand at index `I+1` (currently entirely
unchecked).
