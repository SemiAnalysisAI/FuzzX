# m131: `AMDGPUTargetLowering::SimplifyDemandedBitsForTargetNode` treats `amdgcn_set_inactive` as 1-source

*Discovery method: code inspection; witness constructed manually.*

## The bug

`amdgpu/third_party/llvm-project/llvm/lib/Target/AMDGPU/AMDGPUISelLowering.cpp:5836-5851`:

```cpp
bool AMDGPUTargetLowering::SimplifyDemandedBitsForTargetNode(
    SDValue Op, const APInt &OriginalDemandedBits,
    const APInt &OriginalDemandedElts, KnownBits &Known, TargetLoweringOpt &TLO,
    unsigned Depth) const {
  switch (Op.getOpcode()) {
  case ISD::INTRINSIC_WO_CHAIN: {
    switch (Op.getConstantOperandVal(0)) {
    case Intrinsic::amdgcn_readfirstlane:
    case Intrinsic::amdgcn_readlane:
    case Intrinsic::amdgcn_set_inactive:     // <-- BUG: not 1-source!
    case Intrinsic::amdgcn_wwm: {
      if (SimplifyDemandedBits(Op.getOperand(1), OriginalDemandedBits,
                               OriginalDemandedElts, Known, TLO, Depth + 1))
        return true;
      break;
    }
    ...
```

`llvm.amdgcn.set.inactive(%active, %inactive)` is **2-source**: it
returns `%active` on EXEC-on lanes and `%inactive` on EXEC-off lanes.
The lowering (`SIInstructions.td:340`, `V_SET_INACTIVE_B32`) is
expanded by `SIWholeQuadMode.cpp` into a WWM sequence that writes
`%inactive` to the physical VGPR slots of currently-inactive lanes.

The SimplifyDemandedBits hook only recurses into `Op.getOperand(1)`
(`%active`), so it concludes the result's `KnownBits` are those of
`%active` alone -- silently dropping `%inactive`. If `%active` has
known-zero high bits and `%inactive` is a large constant with those
bits set, downstream `lshr` / `and` / `icmp` / `zext` see an
overstated zero-bits set and fold to wrong values.

The same `case` lumps `amdgcn_wwm` in. `wwm` likewise re-enables
EXEC-off lanes and reads their physical values -- so propagating
`Known` from `wwm`'s single source is correct *as long as* the source
itself doesn't have wider lane-value variability than its known-bits
suggest (i.e., as long as the source is not itself a `set_inactive`).
`readfirstlane` / `readlane` are correct: they read a single specific
lane's value, which (assuming that lane is active) equals operand(1).

## Reproducer

See `reduced.ll`. Two kernels, identical shape, only the **operand
order** of `set_inactive` differs.

```llvm
; @test_buggy: set_inactive(active = id & 0xFF, inactive = 0xFFFF0000)
;              ^ operand(1) Known.Zero = 0xFFFFFF00; lshr 16 -> folded to 0
;
; @test_ref:   set_inactive(0xFFFF0000, id & 0xFF)
;              ^ operand(1) Known.One = 0xFFFF0000; lshr 16 -> 0xFFFF (kept)
;
; Both functions are EXEC-divergent: only lanes (id >= 32) execute
; the if.then body, so lanes 0..31 are inactive when set_inactive runs.
; readlane(v, 0) then reads lane 0's physical VGPR, which under the
; V_SET_INACTIVE_B32 lowering holds the *inactive* value 0xFFFF0000.
; The correct runtime value of `lshr 16` is therefore 0xFFFF in BOTH
; kernels; the buggy fold makes @test_buggy emit 0.
```

`llc -mtriple=amdgcn-amd-amdhsa -mcpu=gfx1100 -O2 reduced.ll -o -`:

```asm
test_buggy:
        v_cmpx_lt_u32_e32 31, v0
        s_cbranch_execz .LBB0_2
; %bb.1:                                ; %if.then
        s_load_b64 s[0:1], s[4:5], 0x0
        v_dual_mov_b32 v1, 0 :: v_dual_lshlrev_b32 v0, 2, v0   ; <-- stores 0
        s_waitcnt lgkmcnt(0)
        global_store_b32 v0, v1, s[0:1]
.LBB0_2:
        s_endpgm

test_ref:
        v_cmpx_lt_u32_e32 31, v0
        s_cbranch_execz .LBB1_2
; %bb.1:                                ; %if.then
        s_load_b64 s[0:1], s[4:5], 0x0
        v_dual_mov_b32 v1, 0xffff :: v_dual_lshlrev_b32 v0, 2, v0  ; <-- 0xFFFF
        s_waitcnt lgkmcnt(0)
        global_store_b32 v0, v1, s[0:1]
.LBB1_2:
        s_endpgm
```

Two kernels with operationally-equivalent semantics (after sliding the
constant from inactive to active operand of `set_inactive`) emit
different stores. The buggy one stores `0`, the well-folded one
stores `0xFFFF`. The lane-0 physical VGPR holds `0xFFFF0000` after the
WWM lowering of `V_SET_INACTIVE_B32` in BOTH kernels, so the runtime
correct answer is `0xFFFF` for both. Only `test_buggy` is wrong.

## Why this is "latent" and why the earlier full-grid kernel didn't witness

In a normal full-grid launch with no divergent control flow, EXEC = all
ones at the `set_inactive` site, so `%active` is read on every lane,
and propagating `Known` from operand(1) gives the same answer as
ground truth. The fold is **accidentally correct** in the no-divergence
case.

Witnessing the bug needs:

1. **Divergent EXEC** at the `set_inactive` site so some lanes are
   genuinely inactive there;
2. A **cross-lane read** (here `readlane(v, 0)` reading a lane that's
   inactive at the set_inactive site) so active lanes can observe the
   inactive value;
3. The cross-lane read must be one that **still triggers the buggy
   propagation** -- i.e., `readlane` / `readfirstlane`, both of which
   are in the same buggy `case` and so recurse into the inactive
   `set_inactive`'s operand(1). `permlane64` / `ds_permute` / `dpp` /
   plain `bitcast` to a VGPR don't propagate `KnownBits` at all, so
   they would *block* the fold and hide the bug.

The fuzzer's IR emitter happens to emit `set_inactive` at module scope
without divergent EXEC and without a following `readlane`-shaped use,
so it never lines up all three conditions at once.

## Suggested fix

Split the `case` and intersect `KnownBits` from both operands of
`set_inactive`:

```cpp
case Intrinsic::amdgcn_set_inactive: {
  KnownBits Known2;
  if (SimplifyDemandedBits(Op.getOperand(1), OriginalDemandedBits,
                           OriginalDemandedElts, Known, TLO, Depth + 1) ||
      SimplifyDemandedBits(Op.getOperand(2), OriginalDemandedBits,
                           OriginalDemandedElts, Known2, TLO, Depth + 1))
    return true;
  Known = Known.intersectWith(Known2);
  break;
}
```

Audit `set_inactive_chain_arg` and any future "merge-by-EXEC"
intrinsics added here.

## Toolchain Results

| Toolchain | Result |
| --- | --- |
| LLVM HEAD with the local PR patches (`build/llvm-fuzzer/bin/llc`) | Reproduces (`v_mov_b32 v1, 0` in `test_buggy`, `v_mov_b32 v1, 0xffff` in `test_ref`). |
| `build/rocm-staging-llvm/bin/llc` | Reproduces. |

Targeted at `gfx1100` (wave32 makes the EXEC-divergent setup compact;
`gfx950` ICEs in Branch Relaxation on the divergent shape -- separate
unrelated codegen bug worth filing).

## Why the fuzzer hasn't caught it

* The IR emitter rarely combines `set_inactive` with a divergent
  EXEC region AND a downstream `readlane(.., constant_lane)` whose
  read target is in the EXEC-off region;
* Per `MEMORY.md` (Prefer-random-over-idioms), the right hook is to
  let the emitter occasionally pair `set_inactive` with
  `readlane(..., <const>)` (or `readfirstlane`) inside a
  divergence-introduced region, with **non-constant** active operand
  and **high-bit-set constant** inactive operand;
* Differential lowering: a "swap the operands of set_inactive and
  re-run llc" mutation (rotation of operand order for commutative-
  looking 2-source merge intrinsics) would have flagged the asm
  difference (`0` vs `0xFFFF`) immediately.
