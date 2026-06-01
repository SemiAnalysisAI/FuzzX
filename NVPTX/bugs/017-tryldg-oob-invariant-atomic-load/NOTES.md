# 017 — tryLDG OOB operand read / crash on invariant atomic load (AtomicSDNode misclassified as MLoad)

- **Kind:** crash (assert/UB)
- **Reachable via:** default llc, sm_70+
- **Component:** NVPTXISelDAGToDAG.cpp 1266-1293 (crash at 1275-1276); reached via tryLoad 1108-1123 + canLowerToLDG 781-787  (round-3 area `T14-ldg-ldu`)
- **Candidate id:** r3_00

## Summary

invariant atomic load (e.g. const __restrict__ + atomic) routed to tryLDG reads operands 3/4 of a 2-operand AtomicSDNode → OOB/assert

## Mechanism / root cause

tryLDG handles three kinds of MemSDNode: plain LoadSDNode, NVPTXISD::MLoad, and (now) ISD::ATOMIC_LOAD, which all route through tryLoad -> canLowerToLDG -> tryLDG. The opening block:

  if (const auto *Load = dyn_cast<LoadSDNode>(LD)) {
    ExtensionType = Load->getExtensionType();
    UsedBytesMask = UINT32_MAX;
  } else {
    ExtensionType = LD->getConstantOperandVal(4);   // OOB for ATOMIC_LOAD
    UsedBytesMask = LD->getConstantOperandVal(3);    // OOB for ATOMIC_LOAD
  }

assumes any non-LoadSDNode MemSDNode is an MLoad (which carries UsedBytesMask at operand 3 and ExtensionType at operand 4). But ISD::ATOMIC_LOAD is an AtomicSDNode (not a LoadSDNode), and a load-atomic node has only 2 operands {chain, ptr}. So getConstantOperandVal(4)/(3) reads past the operand list. canLowerToLDG only checks Subtarget.hasLDG() && CodeAddrSpace==Global && N.isInvariant(); isInvariant() lives on MemSDNode (the AtomicSDNode base), so an atomic load with MOInvariant (from !invariant.load) passes the guard and falls into the bad else-branch. In a debug build this hits assert(Num < NumOperands) in SelectionDAGNodes.h getOperand (line 1063). In a release build it reads OOB memory and then emits ld.global.nc for the atomic load, silently dropping the atomic ordering (e.g. monotonic) -> latent miscompile. Note: the LDG invariant guard itself is otherwise sound (LDG only fires for !invariant.load loads; LDU only fires for explicit nvvm.ldu.global.* intrinsics), so the only defect in this area is the atomic-load operand confusion.

## Trigger

A global-space atomic load that is invariant. Realistic compiler-generated path: a CUDA kernel reading a 'const __restrict__' (readonly noalias) pointer with an atomic load -> NVPTXTagInvariantLoads adds !invariant.load -> tryLoad -> canLowerToLDG true -> tryLDG OOB. Also reproducible directly by attaching !invariant.load to a load-atomic. Confirmed at -O0, default, and -mcpu=sm_70.

## Reproducer

```
target triple = "nvptx64-nvidia-cuda"

define ptx_kernel void @k(ptr addrspace(1) %in, ptr addrspace(1) %out) {
  %v = load atomic i32, ptr addrspace(1) %in monotonic, align 4, !invariant.load !0
  store i32 %v, ptr addrspace(1) %out
  ret void
}
!0 = !{}
```

Command: `llc -mtriple=nvptx64 -mcpu=sm_70 -o - repro.ll`

## Observed (wrong) output

```
Assertion failed: (Num < NumOperands && "Invalid child # of SDNode!"), function getOperand, file SelectionDAGNodes.h, line 1063.
PLEASE submit a bug report to https://github.com/llvm/llvm-project/issues/ and include the crash backtrace and instructions to reproduce the bug.
Stack dump:
1.	Running pass 'Function Pass Manager' on module ...
2.	Running pass 'NVPTX DAG->DAG Pattern Instruction Selection' on function '@k'
 #7 llvm::NVPTXDAGToDAGISel::tryLDG(llvm::MemSDNode*)
 #8 llvm::NVPTXDAGToDAGISel::Select(llvm::SDNode*)
 #9 llvm::SelectionDAGISel::DoInstructionSelection()

(Same assertion also reproduces from the fully attribute-driven input below, where NVPTXTagInvariantLoads adds the !invariant.load metadata automatically — no hand-written metadata:
  define ptx_kernel void @k2(ptr addrspace(1) noalias readonly %in, ptr addrspace(1) %out) {
    %v = load atomic i32, ptr addrspace(1) %in monotonic, align 4
    store i32 %v, ptr addrspace(1) %out
    ret void
  })
```

## Expected

A correct backend should not crash. The atomic monotonic load must be lowered as a regular atomic global load, e.g. 'ld.relaxed.sys.global.b32' (exactly what is emitted when !invariant.load is absent), preserving the monotonic ordering. tryLDG must not read operand 3/4 for an AtomicSDNode (which has only operands {chain, ptr}); the proper fix is to special-case ISD::ATOMIC_LOAD (treat as non-extending, full UsedBytesMask) or to exclude atomic loads from canLowerToLDG so they never reach tryLDG. Lowering an atomic load to ld.global.nc (ldg) would itself be a miscompile since it drops the atomic ordering, so the load must stay a real atomic load.

## Verification

Independent verify + adversarial refute, both `confirmed` (verify confidence 1).

> Confirmed real assertion crash on valid IR.

Mechanism (verified against source at NVPTXISelDAGToDAG.cpp): Select() dispatches ISD::ATOMIC_LOAD to tryLoad() (lines 106-110). tryLoad() calls canLowerToLDG() (line 1122), which returns true when Subtarget.hasLDG() && CodeAddrSpace==Global && N.isInvariant() (lines 781-787). isInvariant() lives on MemSDNode, the AtomicSDNode base, so an atomic load carrying MOInvariant (from !invariant.load) passes the guard and reaches tryLDG(). In tryLDG (lines 1271-1277), the non-LoadSDNode branch unconditionally does LD->getConstantOperandVal(4) and (3), assuming an NVPTXISD::MLoad layout. But a load-atomic AtomicSDNode has only 2 operands {chain, ptr}, so index 4 (and 3) are out of bounds.

Confirmed with -debug-only=isel that the DAG node is exactly 2-operand: 't7: i32,ch = AtomicLoad<(invariant load monotonic (s32) from %ir.in, addrspace 1)> t0, t4' (t0=chain idx0, t4=ptr idx1). getConstantOperandVal(4) -> getOperand(4) trips assert(Num < NumOperands && "Invalid child # of SDNode!") in SelectionDAGNodes.h:1063.

Build is 'Optimized build with assertions' (LLVM_ENABLE_ASSERTIONS=ON), so the OOB read fires the assertion rather than reading garbage. Stack frame #7 is NVPTXDAGToDAGISel::tryLDG, called from Select (#8), as predicted.

Adversarial checks performed:
1. IR is verifier-clean: opt -passes=verify accepts the load-atomic with !invariant
