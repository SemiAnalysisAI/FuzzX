# c002 — getTgtMemIntrinsic crashes on nvvm_ldu_global_* with a non-constant alignment operand (unguarded cast<ConstantInt>)

- region: M-getTgtMemIntrinsic
- file: NVPTXISelLowering.cpp 4791-4803
- kind: segfault
- confidence(finder): 0.95

## Mechanism
The nvvm_ldu_global_i/f/p case does:

    Info.align = cast<ConstantInt>(I.getArgOperand(1))->getMaybeAlignValue();

Argument 1 is the pointer alignment. It is read with cast<ConstantInt> (a checked-in-asserts, unchecked-in-release downcast), assuming the operand is always a compile-time constant. But the intrinsic declarations in IntrinsicsNVVM.td (lines ~2169-2172) do NOT mark this argument ImmArg:

    let IntrProperties = [IntrReadMem, IntrArgMemOnly, IntrNoCallback, IntrWillReturn, NoCapture<ArgIndex<0>>] in {
      def int_nvvm_ldu_global_i : Intrinsic<[llvm_anyint_ty], [llvm_anyptr_ty, llvm_i32_ty]>;
      ...

With no ImmArg constraint, the IR verifier accepts a non-constant i32 (e.g. a function argument or a loaded value) as the alignment operand. getTgtMemIntrinsic is invoked from SelectionDAGBuilder::visitTargetIntrinsic during ISel for every ldu.global call, so a runtime alignment operand makes cast<ConstantInt> fail: in an asserts build it aborts with 'cast<Ty>() argument of incompatible type'; in a release build the static_cast yields a bogus ConstantInt* and ->getMaybeAlignValue() dereferences garbage (null/UAF-style crash). Confirmed: llc aborts in frame NVPTXTargetLowering::getTgtMemIntrinsic. The fix is to use dyn_cast and fall back to no alignment (or to add ImmArg to the .td). Either dyn_cast or auto*CI=dyn_cast<ConstantInt>(...); if(CI) Info.align=CI->getMaybeAlignValue();

## Trigger
Any nvvm_ldu_global_i / _f / _p call whose second (alignment) argument is not a ConstantInt, e.g. a function parameter. Any -mcpu (sm_60+). No special PTX version needed.

## IR
```
target triple = "nvptx64-nvidia-cuda"
declare i32 @llvm.nvvm.ldu.global.i.i32.p0(ptr, i32)
define i32 @t(ptr %p, i32 %align) {
  %v = call i32 @llvm.nvvm.ldu.global.i.i32.p0(ptr %p, i32 %align)
  ret i32 %v
}
```

## llc cmd
`-mtriple=nvptx64 -mcpu=sm_60`
