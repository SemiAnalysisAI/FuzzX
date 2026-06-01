# 063 — cp.reduce.async.bulk.tensor.* (TMA store-with-reduction) emitted on any target — sm_90/PTX8.0 Requires<> bypassed by C++ getMachineNode selection

- **Kind:** other (invalid PTX)
- **Reachable via:** llc -mcpu<sm_90
- **Component:** NVPTXIntrinsics.td (CP_ASYNC_BULK_TENSOR_REDUCE_INTR: lines 860-892) NVPTXISelDAGToDAG.cpp:1965-1987,2102-2210; NVPTXIntrinsics.td:860-892  (round-8 area `X07-cpasync-mbarrier-arch2`)
- **Note:** "invalid PTX" entries are validated against the PTX ISA + in-tree corroboration; no local `ptxas` was available to execute the rejection.

## Summary

`cp.reduce.async.bulk.tensor.*` is custom-selected via `getMachineNode`, bypassing its `[hasPTX<80>,hasSM<90>]` Requires → emitted on pre-Hopper targets

## Mechanism / root cause

The whole cp.reduce.async.bulk.tensor.* family is custom-selected in C++, not pattern-matched. In NVPTXIntrinsics.td the instruction records have NO DAG pattern (only the asm string) and carry a guard purely as a comment-like attribute:

  multiclass CP_ASYNC_BULK_TENSOR_REDUCE_INTR<int dim, bit shared32, string mode> {
    ...
    def "" : NVPTXInst<(outs),
              !con((ins rc:$src, B64:$tmap), dims_dag, (ins TMAReductionFlags:$red_op)),
              !strconcat(prefix, "${red_op}", suffix, asm_str, ";")>,   // <-- no pattern list
              Requires<[hasPTX<80>, hasSM<90>]>;
    def _CH : NVPTXInst<... >, Requires<[hasPTX<80>, hasSM<90>]>;
  }

Because there is no pattern, the Requires<[hasPTX<80>, hasSM<90>]> predicate is never consulted: a TableGen Requires<> only gates the ISel *pattern matcher*. The instruction is reached exclusively via SelectCpAsyncBulkTensorReduceCommon, which builds the node directly with CurDAG->getMachineNode():

  unsigned Opcode = GetCpAsyncBulkTensorS2GReductionOpcode(NumDims, IsShared32, IsCacheHint, IsIm2Col);
  ReplaceNode(N, CurDAG->getMachineNode(Opcode, DL, N->getVTList(), Ops));   // line 1986

getMachineNode emits the chosen opcode unconditionally, with no subtarget check. So an llvm.nvvm.cp.async.bulk.tensor.reduce.* intrinsic is lowered to cp.reduce.async.bulk.tensor on ANY -mcpu, and llc stamps the module with whatever low .target/.version the cpu implies. Confirmed emission of 'cp.reduce.async.bulk.tensor.1d.global.shared::cta.add.tile.bulk_group ...' under .version 5.0 / .target sm_50 and sm_60. This is invalid PTX: the PTX ISA introduces cp.reduce.async.bulk.tensor at PTX ISA 8.0 and requires sm_90 (the in-tree test cp-async-bulk-tensor-reduce.ll gates its ptxas-verify on 'ptxas-sm_90 && ptxas-isa-8.0'); ptxas for sm_50/sm_60 rejects the mnemonic. Same root-cause CLASS as README #052 (cvta.shared::cluster via getMachineNode), but a DISTINCT instruction family and a distinct selection routine, and #052 is explicitly the only getMachineNode-bypass instance listed. The contrast is verifiable: the sibling pattern-based store cp.async.bulk.tensor.s2g.tile.1d correctly produces 'Cannot select' at sm_60, because its predicate IS enforced. The reduce family (8 reduction ops add/min/max/inc/dec/and/or/xor x {tile, im2col} x dims 1..5 x shared32/64 x {plain,_CH}) is uniformly affected.

## Trigger

Call any llvm.nvvm.cp.async.bulk.tensor.reduce.<op>.<tile|im2col>.<N>d intrinsic and compile for a target below sm_90 / PTX 8.0 (e.g. -mcpu=sm_50/sm_60/sm_70, or default). The Hopper-only instruction is emitted into a module declaring a pre-Hopper .target, which ptxas rejects.

## Reproducer

```
target triple = "nvptx64-nvidia-cuda"

declare void @llvm.nvvm.cp.async.bulk.tensor.reduce.add.tile.1d(ptr addrspace(3), ptr, i32, i64, i1 immarg)
declare void @llvm.nvvm.cp.async.bulk.tensor.reduce.and.im2col.3d(ptr addrspace(3), ptr, i32, i32, i32, i64, i1 immarg)

define void @red1d(ptr addrspace(3) %src, ptr %tmap, i32 %d0) {
  call void @llvm.nvvm.cp.async.bulk.tensor.reduce.add.tile.1d(ptr addrspace(3) %src, ptr %tmap, i32 %d0, i64 0, i1 0)
  ret void
}

define void @red_im2col(ptr addrspace(3) %src, ptr %tmap, i32 %d0, i32 %d1, i32 %d2) {
  call void @llvm.nvvm.cp.async.bulk.tensor.reduce.and.im2col.3d(ptr addrspace(3) %src, ptr %tmap, i32 %d0, i32 %d1, i32 %d2, i64 0, i1 0)
  ret void
}
```

Command: `llc -mtriple=nvptx64 -mcpu=sm_60 -mattr=+ptx60 -o - repro.ll`

## Verification

Reproduced with the built llc (crash/emitted-PTX matches; finder confidence 0.85). 
