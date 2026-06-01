# c015 — tcgen05.ld.16x32bx2 i64 offset immediate truncated to i32 in SelectTcgen05Ld

- region: D5-select-dispatch
- file: NVPTXISelDAGToDAG.cpp 294-300
- kind: miscompile
- confidence(finder): 0.18

## Mechanism
The tcgen05.ld.16x32bx2 intrinsics declare their offset operand as llvm_i64_ty (IntrinsicsNVVM.td line 3207: !if(!eq(Shape, "16x32bx2"), [llvm_i64_ty], [])), and the corresponding instruction defines it as i64imm:$offset (NVPTXIntrinsics.td line 5915). In SelectTcgen05Ld's hasOffset branch the immediate is materialized as a 32-bit target constant:

  auto OffsetNode = CurDAG->getTargetConstant(
      cast<ConstantSDNode>(N->getOperand(3))->getZExtValue(), DL, MVT::i32);

SelectionDAG::getConstant(uint64_t, ..., EVT VT, ...) builds APInt(VT.getScalarSizeInBits(), Val) (SelectionDAG.cpp:1727), so with VT=MVT::i32 the value is zero-truncated to 32 bits before being placed in the instruction's i64imm $offset field. Any offset immediate >= 2^32 therefore has its upper 32 bits silently dropped, emitting a different constant into the PTX text than the IR specified. The non-offset siblings are unaffected. NOTE: in practice the PTX tcgen05.ld 16x32bx2 offset is a small lane offset, so a value >= 2^32 is semantically nonsensical and would not appear in real code; this makes the discrepancy reachable from valid (non-UB) IR but of negligible practical impact.

## Trigger
Target with tcgen05 support (e.g. -mcpu=sm_100a), a call to @llvm.nvvm.tcgen05.ld.16x32bx2.xN with an i64 immediate offset >= 4294967296 (2^32). The offset is an ImmArg so it must be a literal constant.

## IR
```
declare <2 x i32> @llvm.nvvm.tcgen05.ld.16x32bx2.x2(ptr addrspace(6), i64 immarg, i1 immarg)

define <2 x i32> @t(ptr addrspace(6) %taddr) {
  ; offset = 0x1_0000_0002 ; low 32 bits are 2, high bit set
  %v = tail call <2 x i32> @llvm.nvvm.tcgen05.ld.16x32bx2.x2(ptr addrspace(6) %taddr, i64 4294967298, i1 0)
  ret <2 x i32> %v
}
```

## llc cmd
`-mtriple=nvptx64 -mcpu=sm_100a -O2`
