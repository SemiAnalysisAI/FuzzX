# 013 — tcgen05.ld.16x32bx2 i64 offset immediate truncated to i32 in SelectTcgen05Ld

- **Kind:** crash (assert) / miscompile in release
- **Reachable via:** llc -mcpu=sm_100a
- **Component:** NVPTXISelDAGToDAG.cpp 294-300  (region `D5-select-dispatch`)
- **Candidate id:** c015

## Summary

`tcgen05.ld.16x32bx2` i64 offset immediate is built as i32, asserting (or truncating in release)

## Mechanism / root cause

The tcgen05.ld.16x32bx2 intrinsics declare their offset operand as llvm_i64_ty (IntrinsicsNVVM.td line 3207: !if(!eq(Shape, "16x32bx2"), [llvm_i64_ty], [])), and the corresponding instruction defines it as i64imm:$offset (NVPTXIntrinsics.td line 5915). In SelectTcgen05Ld's hasOffset branch the immediate is materialized as a 32-bit target constant:

  auto OffsetNode = CurDAG->getTargetConstant(
      cast<ConstantSDNode>(N->getOperand(3))->getZExtValue(), DL, MVT::i32);

SelectionDAG::getConstant(uint64_t, ..., EVT VT, ...) builds APInt(VT.getScalarSizeInBits(), Val) (SelectionDAG.cpp:1727), so with VT=MVT::i32 the value is zero-truncated to 32 bits before being placed in the instruction's i64imm $offset field. Any offset immediate >= 2^32 therefore has its upper 32 bits silently dropped, emitting a different constant into the PTX text than the IR specified. The non-offset siblings are unaffected. NOTE: in practice the PTX tcgen05.ld 16x32bx2 offset is a small lane offset, so a value >= 2^32 is semantically nonsensical and would not appear in real code; this makes the discrepancy reachable from valid (non-UB) IR but of negligible practical impact.

## Trigger

Target with tcgen05 support (e.g. -mcpu=sm_100a), a call to @llvm.nvvm.tcgen05.ld.16x32bx2.xN with an i64 immediate offset >= 4294967296 (2^32). The offset is an ImmArg so it must be a literal constant.

## Reproducer

See `repro.ll` / `cmd.sh`.

```
declare <2 x i32> @llvm.nvvm.tcgen05.ld.16x32bx2.x2(ptr addrspace(6), i64 immarg, i1 immarg)

define <2 x i32> @t(ptr addrspace(6) %taddr) {
  %v = tail call <2 x i32> @llvm.nvvm.tcgen05.ld.16x32bx2.x2(ptr addrspace(6) %taddr, i64 4294967298, i1 0)
  ret <2 x i32> %v
}
```

Command:

```
llc -mtriple=nvptx64 -mcpu=sm_100a -O2 -o - repro.ll
```

## Observed (wrong) output

```
Assertion failed: (llvm::isUIntN(BitWidth, val) && "Value is not an N-bit unsigned value"), function APInt, file APInt.h, line 128.
PLEASE submit a bug report to https://github.com/llvm/llvm-project/issues/ and include the crash backtrace and instructions to reproduce the bug.
Stack dump:
1.	Running pass 'Function Pass Manager' on module '.../c015.ll'.
2.	Running pass 'NVPTX DAG->DAG Pattern Instruction Selection' on function '@t'
 #7 llvm::SelectionDAG::getConstant(unsigned long long, llvm::SDLoc const&, llvm::EVT, bool, bool)
 #8 llvm::NVPTXDAGToDAGISel::SelectTcgen05Ld(llvm::SDNode*, bool)
 #9 llvm::NVPTXDAGToDAGISel::tryIntrinsicChain(llvm::SDNode*)
#10 llvm::NVPTXDAGToDAGISel::Select(llvm::SDNode*)
```

## Expected

A correct compiler must not assert/crash on this valid, verifier-accepted IR. The offset is declared i64 (llvm_i64_ty) and the instruction operand is i64imm:$offset, so SelectTcgen05Ld should build the target constant with MVT::i64 instead of MVT::i32. With that fix the full 64-bit immediate is preserved and emitted, e.g. "tcgen05.ld.sync.aligned.16x32bx2.x2.b32 {%r2, %r3}, [%r1], 4294967298;". (In a no-asserts build the current code instead silently truncates the immediate to its low 32 bits, emitting ", 2;" — a miscompile of the constant — which the MVT::i64 fix also resolves.)

## Verification

Verified empirically with the built llc (independent verify + adversarial
refute both `confirmed`, finder confidence 0.18, verify confidence 0.95).

> Mechanism confirmed in source. The tcgen05.ld.16x32bx2 intrinsic declares its offset as llvm_i64_ty with ImmArg (IntrinsicsNVVM.td:3207-3211), and the instruction defines i64imm:$offset (NVPTXIntrinsics.td:5915). But SelectTcgen05Ld (NVPTXISelDAGToDAG.cpp:296-297) materializes the immediate with CurDAG->getTargetConstant(...getZExtValue(), DL, MVT::i32). getTargetConstant(uint64_t,...) forwards to getConstant which builds APInt(VT.getScalarSizeInBits()=32, Val, isSigned=false) (SelectionDAG.cpp:1725-1729). The APInt ctor default implicitTrunc=false, so for any offset >= 2^32 the assertion isUIntN(32,val) fails (APInt.h:128).

Empirically reproduced with the provided assertions-enabled llc: offset i64 4294967298 (0x1_0000_0002) crashes with "Assertion failed: (llvm::isUIntN(BitWidth, val) && \"Value is not an N-bit unsigned value\")" in SelectionDAG::getConstant called from NVPTXDAGToDAGISel::SelectTcgen05Ld (full stack captured). The crash is deterministic across reruns.

The IR is valid and non-UB: opt -passes=verify accepts it (exit 0); the offset operand type is i64 and 4294967298 is a legal i64 literal ImmArg. A control case with offset i64 2 compiles cleanly and emits the correct PTX (\"...x2.b32 {%r2,%r3}, [%r1], 2;\"), proving the path works and the assertion is specifically triggered by the >2^32 value.

Note on classification: the candidate labeled this \"miscompile\" (silent truncation to low 32 bits). That is exactly what would happen in a Release/no-asserts build,
