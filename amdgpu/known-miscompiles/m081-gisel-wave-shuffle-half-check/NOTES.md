# m081: GISel selectWaveShuffleIntrin XORs ThreadID with the *shifted* index, not the original

*Discovery method: code inspection.*  Found by reading the manual
GISel selection routines in
`llvm/lib/Target/AMDGPU/AMDGPUInstructionSelector.cpp` and comparing
`selectWaveShuffleIntrin` to its SDAG counterpart
`lowerWaveShuffle` in `llvm/lib/Target/AMDGPU/SIISelLowering.cpp`.

## Bug

`selectWaveShuffleIntrin` lowers `@llvm.amdgcn.wave.shuffle` on
wave64 targets without wave-wide bpermute (GFX10 wave64 and GFX11
wave64) using a "permute each half, then select" scheme.  The half
selection compares bit 5 of the source-lane index with bit 5 of the
current lane id (`ThreadID`).  The intent (matching SDAG) is
`(ThreadID ^ Index) & 32`, but GISel overwrites the index virtual
register with `Index << 2` *before* the xor, so what it actually
computes is `(ThreadID ^ (Index << 2)) & 32`, i.e. bit 5 of ThreadID
XOR **bit 3** of Index.

Whenever bit 3 and bit 5 of Index disagree, the selector picks the
wrong permute path:

* `bit3(Index)=0, bit5(Index)=1`: SDAG correctly routes through
  `permlane64` (other half), GISel routes through direct bpermute
  (same half) and returns the value of a lane in the *current* half.
* `bit3(Index)=1, bit5(Index)=0`: SDAG correctly uses direct
  bpermute, GISel goes through `permlane64` and returns the value
  from lane `Index XOR 32` (the other half).

## Buggy code

`llvm/lib/Target/AMDGPU/AMDGPUInstructionSelector.cpp`, around
lines 4082-4128 in `selectWaveShuffleIntrin`:

```cpp
// ds_bpermute requires index to be multiplied by 4
Register ShiftIdxReg = MRI->createVirtualRegister(DstRC);
BuildMI(... AMDGPU::V_LSHLREV_B32_e64, ShiftIdxReg)
    .addImm(2)
    .addReg(IdxReg);

Register PoisonIdxReg = MRI->createVirtualRegister(DstRC);
BuildMI(... AMDGPU::V_SET_INACTIVE_B32, PoisonIdxReg)
    .addImm(0)
    .addReg(ShiftIdxReg)              // <-- shifted
    ...

...

Register XORReg = MRI->createVirtualRegister(DstRC);
BuildMI(... AMDGPU::V_XOR_B32_e64, XORReg)
    .addReg(ThreadIDReg)
    .addReg(PoisonIdxReg);            // <-- BUG: should be IdxReg
                                      //     (or set_inactive(IdxReg)),
                                      //     not the shifted version.
```

The SDAG path (`lowerWaveShuffle` in `SIISelLowering.cpp`,
~lines 8131-8134) gets it right because it keeps `Index` and
`ShiftedIndex` as two distinct SDValues:

```cpp
SDValue SameOrOtherHalf =
    DAG.getNode(ISD::AND, SL, MVT::i32,
                DAG.getNode(ISD::XOR, SL, MVT::i32, ThreadID, Index),
                DAG.getTargetConstant(32, SL, MVT::i32));
```

## Reproducer

`reduced.ll` (in this directory).  The function loads a per-lane
shuffle index that, at lane 0, selects source lane 8 — bit 3 of the
index is 1 but bit 5 is 0, so SDAG correctly stays in-half and reads
value 8 from lane 8, while GISel mis-routes through permlane64 and
returns value 40 from lane 40.

The file is compiled with `target-cpu=gfx1100` and
`target-features=+wavefrontsize64` because the buggy code path only
runs on wave64 GFX10/GFX11.  Our local hardware is gfx950 (GFX9),
which takes a different fast path and is not affected.

## Verification

```bash
cd /tmp/findbug/gisel_sel

CLANG=/home/orenamd@semianalysis.com/FuzzX/amdgpu/build/llvm-fuzzer/bin/clang
"$CLANG" -nogpulib -target amdgcn-amd-amdhsa -mcpu=gfx1100 \
    -Xclang -target-feature -Xclang +wavefrontsize64 \
    -mllvm -global-isel -S -x ir reduced.ll -o reduced.gisel.s
"$CLANG" -nogpulib -target amdgcn-amd-amdhsa -mcpu=gfx1100 \
    -Xclang -target-feature -Xclang +wavefrontsize64 \
    -S -x ir reduced.ll -o reduced.sdag.s

diff <(grep -E 'v_xor|v_lshlrev_b32|v_mbcnt|v_permlane64|ds_bpermute|v_cmp_eq' reduced.sdag.s) \
     <(grep -E 'v_xor|v_lshlrev_b32|v_mbcnt|v_permlane64|ds_bpermute|v_cmp_eq' reduced.gisel.s)
```

Relevant lines (annotated):

SDAG (correct):
```
v_xor_b32_e64 v6, s0, v0       ; v6 = Index (kept in v6)
v_lshlrev_b32_e64 v0, s0, v6   ; v0 = ShiftedIndex (v6 preserved)
...
v_mbcnt_lo_u32_b32 v5, s1, 0   ; v5 = ThreadID
v_xor_b32_e64 v5, v5, v6       ; v5 = ThreadID XOR Index   <-- CORRECT
```

GISel (buggy):
```
v_xor_b32_e64 v0, v0, v3       ; v0 = Index (in-place)
v_lshlrev_b32_e64 v0, 2, v0    ; v0 = ShiftedIndex (OVERWRITES Index!)
...
v_mbcnt_lo_u32_b32 v5, -1, 0   ; v5 = ThreadID
v_xor_b32_e64 v5, v5, v2       ; v5 = ThreadID XOR set_inactive(ShiftedIndex)  <-- WRONG
```

## Why no runtime mismatch on our hardware

Our gfx950 is GFX9.  `GCNSubtarget::supportsWaveWideBPermute()`
returns true for `getGeneration() <= GFX9`, so GFX9 takes the
single-`ds_bpermute_b32` fast path that has no permlane64 and no
half-select; the buggy code is not executed.  The bug needs a
wave64 GFX10 or GFX11 device to manifest at runtime.

## Aside: secondary issue with V_PERMLANE64_B32 on GFX9

The slow path unconditionally emits `V_PERMLANE64_B32`, but
`GCNSubtarget::hasPermLane64()` is `getGeneration() >= GFX11`.  The
slow path is unreachable on subtargets that lack permlane64 *only*
because the predicate gating it (`supportsWaveWideBPermute()`) is
true for GFX9 and earlier.  If those predicates ever drift, the
selector would silently emit an instruction the target does not
have.  Worth gating the slow path on `hasPermLane64()` explicitly.

## Fix sketch

Pass the *unshifted* index register to the xor, e.g.:

```cpp
// Need the inactive-poisoned, *unshifted* index for the half check.
Register PoisonUnshiftedIdxReg = MRI->createVirtualRegister(DstRC);
BuildMI(... V_SET_INACTIVE_B32, PoisonUnshiftedIdxReg)
    .addImm(0).addReg(IdxReg).addImm(0)
    .addReg(UndefValReg).addReg(UndefExecReg);

...

BuildMI(... V_XOR_B32_e64, XORReg)
    .addReg(ThreadIDReg)
    .addReg(PoisonUnshiftedIdxReg);   // unshifted, matches SDAG
```

Or, since SDAG xors against the original `Index` SDValue (no
`set_inactive`) and the AND-with-32 masks away anything outside bit 5,
just xor against `IdxReg` directly.
