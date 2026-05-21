# m150: `s_barrier_init` / `s_barrier_signal_var` M0 lowering discards mask immediately after computing it

*Discovery method: code inspection (during amdgcn.s.barrier audit).*

## The bug

`amdgpu/third_party/llvm-project/llvm/lib/Target/AMDGPU/SIISelLowering.cpp:12450-12459`
(in the lowering of `amdgcn.s.barrier.init` and
`amdgcn.s.barrier.signal.var`):

```cpp
// Intent (per the comments at 12448-12449): pack BarrierID into
// M0[5:0] and member-count into M0[21:16].

M0Val = SDValue(DAG.getMachineNode(AMDGPU::S_AND_B32, ..., CntOp,
                                   DAG.getTargetConstant(0x3F, ...)), 0);  // (1)
constexpr unsigned ShAmt = 16;
M0Val = DAG.getNode(ISD::SHL, DL, MVT::i32, CntOp,                          // (2)
                    DAG.getShiftAmountConstant(ShAmt, MVT::i32, DL));
```

Line (1) computes `CntOp & 0x3F` and assigns to `M0Val`, then line
(2) **immediately overwrites `M0Val`** with `CntOp << 16` -- using
the **unmasked** `CntOp`.  The intended `(CntOp & 0x3F) << 16`
expression is never produced; line (1) is dead code.

If the user passes a member-count with bits >=6 set (e.g. 0x40,
0x80, 0x100), those bits land in M0[27:22], corrupting unrelated
fields when the hardware decodes M0.

## Reproducer

`reduced.ll`:

```llvm
declare void @llvm.amdgcn.s.barrier.init(i32, i32)

define amdgpu_kernel void @t(i32 %cnt) {
  call void @llvm.amdgcn.s.barrier.init(i32 0, i32 %cnt)
  ret void
}
```

With `%cnt = 0x40`:
* Buggy lowering: `M0 = (0x40 << 16) = 0x400000` -> `M0[22] = 1`
  (corrupts the field bordering the member-count field).
* Intended: `M0 = ((0x40 & 0x3F) << 16) = 0` (just the barrier ID).

`llc -mtriple=amdgcn -mcpu=gfx950 -O2 reduced.ll`: the emitted
`S_MOV_B32 m0, ...` sequence corresponds to `CntOp << 16`, not
`(CntOp & 0x3F) << 16`.

## Suggested fix

Replace the two-step assignment with a proper compose:

```cpp
M0Val = SDValue(DAG.getMachineNode(AMDGPU::S_AND_B32, DL, MVT::i32,
                                   CntOp,
                                   DAG.getTargetConstant(0x3F, DL, MVT::i32)),
                0);
M0Val = DAG.getNode(ISD::SHL, DL, MVT::i32, M0Val,         // <-- shift the MASKED value
                    DAG.getShiftAmountConstant(16, MVT::i32, DL));
M0Val = DAG.getNode(ISD::OR, DL, MVT::i32, M0Val, BarID);  // <-- OR with barrier ID
```

## Adjacent findings (not filed separately)

Same audit identified two further defects in this area:

* `SOPInstructions.td:507-573, 1651-1664` -- `S_BARRIER`,
  `S_BARRIER_WAIT`, `S_BARRIER_{SIGNAL,INIT,JOIN}_{IMM,M0}` /
  `S_WAKEUP_BARRIER_*` pseudos set `hasSideEffects = 1` and
  `isConvergent = 1` but inherit `mayLoad = 0; mayStore = 0`.
  Combined with `IntrNoMem` on the intrinsics
  (`IntrinsicsAMDGPU.td:282-328`), MachineScheduler /
  PostRAScheduler can reorder independent `global_load` / `flat_load`
  / LDS ops across `s_barrier`, breaking the synchronization.
  Compare `S_WAKEUP` (`SOPInstructions.td:1685-1686`) which sets both.
  `SIInsertWaitcnts` does not compensate.

* `SOPInstructions.td:2318-2335, 2917-2922` -- split-barrier real
  encodings exist only for gfx12/gfx13.  On gfx950 the split-barrier
  intrinsics select to pseudos with no real opcode (MC failure)
  rather than being rejected up front at the intrinsic-lowering
  layer.

## Why the fuzzer hasn't caught it

* The IR fuzzer rarely emits `amdgcn.s.barrier.init` /
  `amdgcn.s.barrier.signal.var` with a runtime-variable member
  count.  Per `MEMORY.md` (Prefer-random-over-idioms), the random
  emitter should generate these intrinsics with non-zero member
  counts including values >0x3F.
* The differential O0-vs-O2 oracle would not catch this directly
  (both opt levels share the same lowering).  An asm-pattern oracle
  comparing M0 setup against the documented bit layout would.

## Toolchain Results

| Toolchain | Result |
| --- | --- |
| LLVM HEAD with the local PR patches (`build/llvm-fuzzer`) | Lowering present; mask is dead code. |
| ROCm 7.1.1 | Same defect. |
