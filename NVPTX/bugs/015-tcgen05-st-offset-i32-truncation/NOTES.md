# 015 — tcgen05.st 16x32bx2 i64 offset immarg truncated to i32 in SelectTcgen05St (miscompile/assert)

- **Kind:** crash (assert) / miscompile in release
- **Reachable via:** llc -mcpu=sm_100a
- **Component:** NVPTXISelDAGToDAG.cpp 2086-2088  (class `C2-truncating-immediate-width`)
- **Found in:** round-2 class sweep (sibling of round-1 finds)

## Summary

`tcgen05.st.16x32bx2` i64 offset immediate built as i32 in `SelectTcgen05St` (sibling of #013)

## Mechanism / root cause

In NVPTXDAGToDAGISel::SelectTcgen05St the immediate offset operand is materialized as:

  Operands.push_back(CurDAG->getTargetConstant(
      cast<ConstantSDNode>(N->getOperand(3))->getZExtValue(), DL,
      MVT::i32)); // Offset

The offset is declared as i64 everywhere it matters: the intrinsic class NVVM_TCGEN05_ST in include/llvm/IR/IntrinsicsNVVM.td uses `llvm_i64_ty` for the 16x32bx2 offset and marks it ImmArg, and the instruction operand in lib/Target/NVPTX/NVPTXIntrinsics.td (TCGEN05_ST_INST, line 5939) is `i64imm:$offset`. By choosing MVT::i32 the C++ selection code drops the high 32 bits of the 64-bit immediate. The 16x32bx2 LD path (lowerTcgen05LdRed) keeps the offset as an i64 SDValue and is matched by tablegen against `i64imm:$offset`, so it is correct; only the hand-written ld/st selection truncates. In a release (no-assertions) build, getTargetConstant silently truncates, so e.g. offset 0x100000002 is emitted as `, 2` in the `tcgen05.st.sync.aligned.16x32bx2.* [taddr], <offset>, {...}` instruction -> store to wrong tensor-memory location. In an assertions build it instead trips the APInt `isUIntN(BitWidth,val)` assert (confirmed crash, see llc_cmd).

## Trigger

Call llvm.nvvm.tcgen05.st.16x32bx2.x* with an i64 immediate offset whose value does not fit in 32 bits (e.g. i64 4294967298 = 0x1_0000_0002).

## Reproducer

```
target triple = "nvptx64-nvidia-cuda"

define void @st_big_offset(ptr addrspace(6) %taddr, <2 x i32> %stv2) {
  tail call void @llvm.nvvm.tcgen05.st.16x32bx2.x2(ptr addrspace(6) %taddr, i64 4294967298, <2 x i32> %stv2, i1 0)
  ret void
}
```

Command: `llc -march=nvptx64 -mcpu=sm_100a -mattr=+ptx86 -o - repro.ll`

## Verification

Reproduced with the built NVPTX `llc` (confirmed_with_llc=True, finder confidence 0.88).
