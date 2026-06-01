# 025 — Assertion `ArgOuts.size() == 1` in LowerCall for scalar integer call arg/return of 65-127 bits (i65/i72/i96)

- **Kind:** crash (assert/UB)
- **Reachable via:** default llc
- **Component:** NVPTXISelLowering.cpp 1513-1521 (ArgDeclare lambda, assert at 1516)  (round-4 area `U07-asmprinter-param-retval`)
- **Candidate id:** r4_05

## Summary

scalar integer call argument hits `assert(ArgOuts.size()==1)` in LowerCall

## Mechanism / root cause

shouldPassAsArray(Ty) returns false for a scalar integer with getScalarSizeInBits() in [65,127] (it only catches >=128). But such an integer legalizes to multiple PTX registers (i72 -> 2x i64, seen as `ld.param.v2.b64`). In LowerCall the ArgDeclare lambda takes the non-array MakeDeclareScalarParam branch and asserts ArgOuts.size()==1:

  if (IsByVal || shouldPassAsArray(Arg.Ty))
    return MakeDeclareArrayParam(...);
  assert(ArgOuts.size() == 1 && "We must pass only one value as non-array");
  ...
  return MakeDeclareScalarParam(ParamSymbol, TySize);

For i72 ArgOuts.size()==2, so the assertion fires (asserts build). NOTE: in a release build the assert is gone and the code declares a single `.param .b128` (promoteScalarArgumentSize(128)) and stores the 2 i64 pieces at offsets 0 and 8, which is self-consistent with the callee, so this is an over-strict assertion rather than a release-mode miscompile. Reported as low value per the task's classification of asserts.

## Trigger

A direct call returning or taking a scalar integer whose bit width is in [65,127] (e.g. i65, i72, i96). The callee/standalone definition compiles fine; only the call site asserts.

## Reproducer

```
declare i72 @ext_i72(i72 %x)

define i72 @call_i72(i72 %x) {
  %r = call i72 @ext_i72(i72 %x)
  ret i72 %r
}
```

Command: `llc -mtriple=nvptx64 -mcpu=sm_70 -o - repro.ll`

## Observed (wrong) output

```
Assertion failed: (ArgOuts.size() == 1 && "We must pass only one value as non-array"), function operator(), file NVPTXISelLowering.cpp, line 1516.
PLEASE submit a bug report to https://github.com/llvm/llvm-project/issues/ and include the crash backtrace and instructions to reproduce the bug.
Stack dump:
0.	Program arguments: /Users/justinlebar/code/llvm2/build/bin/llc -mtriple=nvptx64 -mcpu=sm_70 r4_05.ll -o -
1.	Running pass 'Function Pass Manager' on module 'r4_05.ll'.
2.	Running pass 'NVPTX DAG->DAG Pattern Instruction Selection' on function '@call_i72'
 #7 ... llvm::NVPTXTargetLowering::LowerCall(...)
 #8 ... llvm::TargetLowering::LowerCallTo(...)
```

## Expected

llc should compile the call without asserting, matching how the standalone definition already compiles: declare the param/return as a single .param .b128 and store/load the two legalized i64 pieces (st.param.v2.b64 / ld.param.v2.b64) at offsets 0 and 8. Concretely, the fix is that shouldPassAsArray (or the LowerCall path) must handle scalar integers wider than 64 bits, not just >=128, so the over-strict `assert(ArgOuts.size() == 1)` is not reached for i65..i127. The definition side already emits, for a 2-arg i72 add, `.param .b128` params with `ld.param.v2.b64 {%rd1,%rd2}` — the call site should mirror this.

## Verification

Independent verify + adversarial refute confirmed 

> Confirmed the mechanism in source and reproduced the crash on valid IR with the asserts-enabled llc.

Source check: shouldPassAsArray (NVPTXUtilities.h:64-67) returns true only when getScalarSizeInBits() >= 128 (plus aggregates/vectors/half/bfloat). A scalar integer in [65,127] bits returns false. In LowerCall's ArgDeclare lambda (NVPTXISelLowering.cpp:1509-1521), when neither IsByVal nor shouldPassAsArray holds, it executes `assert(ArgOuts.size() == 1 && "We must pass only one value as non-array")` (line 1516). But ComputePTXValueVTs legalizes e.g. i72 into 2 PTX i64 pieces, so ArgOuts.size()==2 and the assert fires.

Reproduction: `llc -mtriple=nvptx64 -mcpu=sm_70` on a 5-line module (direct call returning/taking i72) aborts with exactly:
  Assertion failed: (ArgOuts.size() == 1 && "We must pass only one value as non-array"), function operator(), file NVPTXISelLowering.cpp, line 1516.
Stack trace confirms NVPTXTargetLowering::LowerCall during ISel of @call_i72. The IR parses cleanly (not rejected, not UB — it is just a plain call of a value).

Class verified: i65, i72, i96, i127 all hit the assert; i128 (>=128) takes the array path and compiles cleanly (`.b8 func_retval0[16]`), c
