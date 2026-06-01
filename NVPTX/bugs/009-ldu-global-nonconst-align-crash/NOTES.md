# 009 — getTgtMemIntrinsic crashes on nvvm_ldu_global_* with a non-constant alignment operand (unguarded cast<ConstantInt>)

- **Kind:** crash (assert/UB)
- **Reachable via:** default llc
- **Component:** NVPTXISelLowering.cpp 4791-4803  (region `M-getTgtMemIntrinsic`)
- **Candidate id:** c002

## Summary

`llvm.nvvm.ldu.global.*` with a non-constant alignment operand crashes `getTgtMemIntrinsic`

## Mechanism / root cause

The nvvm_ldu_global_i/f/p case does:

    Info.align = cast<ConstantInt>(I.getArgOperand(1))->getMaybeAlignValue();

Argument 1 is the pointer alignment. It is read with cast<ConstantInt> (a checked-in-asserts, unchecked-in-release downcast), assuming the operand is always a compile-time constant. But the intrinsic declarations in IntrinsicsNVVM.td (lines ~2169-2172) do NOT mark this argument ImmArg:

    let IntrProperties = [IntrReadMem, IntrArgMemOnly, IntrNoCallback, IntrWillReturn, NoCapture<ArgIndex<0>>] in {
      def int_nvvm_ldu_global_i : Intrinsic<[llvm_anyint_ty], [llvm_anyptr_ty, llvm_i32_ty]>;
      ...

With no ImmArg constraint, the IR verifier accepts a non-constant i32 (e.g. a function argument or a loaded value) as the alignment operand. getTgtMemIntrinsic is invoked from SelectionDAGBuilder::visitTargetIntrinsic during ISel for every ldu.global call, so a runtime alignment operand makes cast<ConstantInt> fail: in an asserts build it aborts with 'cast<Ty>() argument of incompatible type'; in a release build the static_cast yields a bogus ConstantInt* and ->getMaybeAlignValue() dereferences garbage (null/UAF-style crash). Confirmed: llc aborts in frame NVPTXTargetLowering::getTgtMemIntrinsic. The fix is to use dyn_cast and fall back to no alignment (or to add ImmArg to the .td). Either dyn_cast or auto*CI=dyn_cast<ConstantInt>(...); if(CI) Info.align=CI->getMaybeAlignValue();

## Trigger

Any nvvm_ldu_global_i / _f / _p call whose second (alignment) argument is not a ConstantInt, e.g. a function parameter. Any -mcpu (sm_60+). No special PTX version needed.

## Reproducer

See `repro.ll` / `cmd.sh`.

```
target triple = "nvptx64-nvidia-cuda"

declare i32 @llvm.nvvm.ldu.global.i.i32.p0(ptr, i32)

define i32 @t(ptr %p, i32 %align) {
  %v = call i32 @llvm.nvvm.ldu.global.i.i32.p0(ptr %p, i32 %align)
  ret i32 %v
}
```

Command:

```
llc -mtriple=nvptx64 -mcpu=sm_60 -o - repro.ll
```

## Observed (wrong) output

```
Assertion failed: (isa<To>(Val) && "cast<Ty>() argument of incompatible type!"), function cast, file Casting.h, line 572.
PLEASE submit a bug report to https://github.com/llvm/llvm-project/issues/ and include the crash backtrace and instructions to reproduce the bug.
Stack dump:
0.	Program arguments: /Users/justinlebar/code/llvm2/build/bin/llc -mtriple=nvptx64 -mcpu=sm_60 /Users/justinlebar/code/FuzzX/NVPTX/scratch/c002.ll -o -
1.	Running pass 'Function Pass Manager' on module '.../c002.ll'.
2.	Running pass 'NVPTX DAG->DAG Pattern Instruction Selection' on function '@t'
 #8 0x000000010135ee94 llvm::NVPTXTargetLowering::getTgtMemIntrinsic(llvm::SmallVectorImpl<llvm::TargetLoweringBase::IntrinsicInfo>&, llvm::CallBase const&, llvm::MachineFunction&, unsigned int) const
 #9 0x0000000102a2a488 llvm::SelectionDAGBuilder::visitTargetIntrinsic(llvm::CallInst const&, unsigned int)
EXIT: 134

(Verifier control: `opt -passes=verify c002.ll` exits 0 — IR is valid. Constant-alignment control compiles cleanly to `ldu.global.b32`.)
```

## Expected

llc should compile the function without crashing, emitting an ldu.global load (and simply dropping/defaulting the alignment when the operand is not a compile-time constant). A correct implementation would use `auto *CI = dyn_cast<ConstantInt>(I.getArgOperand(1)); if (CI) Info.align = CI->getMaybeAlignValue();` (else Info.align.reset()), or mark the alignment argument ImmArg in IntrinsicsNVVM.td so the verifier enforces a constant. Either way: no assertion failure on valid IR.

## Verification

Verified empirically with the built llc (independent verify + adversarial
refute both `confirmed`, finder confidence 0.95, verify confidence 0.99).

> Confirmed real assertion failure on VALID IR.

Source confirmation: NVPTXISelLowering.cpp:4791-4803 handles nvvm_ldu_global_i/f/p and at line 4799 does `Info.align = cast<ConstantInt>(I.getArgOperand(1))->getMaybeAlignValue();`. The intrinsic declarations in IntrinsicsNVVM.td:2169-2172 use IntrProperties = [IntrReadMem, IntrArgMemOnly, IntrNoCallback, IntrWillReturn, NoCapture<ArgIndex<0>>] with NO ImmArg on argument 1 (the alignment). So the IR verifier accepts a non-constant i32 as the alignment operand.

Validity check: I ran the reproducer through `opt -passes=verify` and it passed cleanly (exit 0, no diagnostics) — the module is valid IR, not UB and not rejected by the verifier. A non-constant alignment operand (a plain function parameter) is legal because the intrinsic lacks ImmArg.

Crash check: llc aborts with `Assertion failed: (isa<To>(Val) && "cast<Ty>() argument of incompatible type!"), function cast, file Casting.h, line 572`. Stack frame #8 is exactly `llvm::NVPTXTargetLowering::getTgtMemIntrinsic(...)`, invoked from SelectionDAGBuilder::visitTargetIntrinsic during NVPTX ISel — matching the claimed mechanism precisely. The assert message corresponds to the cast<ConstantInt> downcast at line 4799.

Contrast control: an otherwise-identical function passing a constant alignment (i32 4) compiles cleanly to valid PTX (ldu.global.b32), proving the non-constant operand is the trigger and the path is not otherwise broken.

This is an asserts-build abort; in a release (n
