# m099: `GCNTTIImpl::isAlwaysUniform` over-claims `(tid_x & divergent_mask)` uniform

*Discovery method: code inspection.*  Sibling shape to `m086` -- a
target-side "uniformity" hook lying about an instruction whose
divergence depends on an operand it never inspects.

## The bug

`amdgpu/third_party/llvm-project/llvm/lib/Target/AMDGPU/AMDGPUTargetTransformInfo.cpp:1219-1225`
(`GCNTTIImpl::isAlwaysUniform`):

```cpp
Value *Mask;
if (match(V, m_c_And(m_Intrinsic<Intrinsic::amdgcn_workitem_id_x>(),
                     m_Value(Mask)))) {
  return computeKnownBits(Mask, DL).countMinTrailingZeros() >=
             ST->getWavefrontSizeLog2() &&
         XDimDoesntResetWithinWaves;
}
```

The intent: `workitem.id.x` varies in the low `log2(wavefrontSize)`
bits within a wave and is uniform in the higher bits (when
`XDimDoesntResetWithinWaves`).  AND-ing with a `Mask` whose
`countMinTrailingZeros >= log2(wavefrontSize)` masks the
lane-varying low bits away, leaving only the uniform high bits.  This
is sound **only when `Mask` is itself uniform across the wave**.

The hook checks `Mask`'s known-trailing-zero count but **never checks
that `Mask` is uniform**.  If `Mask` is a divergent value whose low
bits happen to be known zero (e.g. `shl %div, log2(wavefrontSize)`),
then the high bits of `Mask` carry per-lane garbage, the AND result is
genuinely divergent, and yet the hook returns `true`.

The generic `UniformityAnalysis` framework treats this as a pinned
`AlwaysUniform` override (`addUniformOverride` ->
`UniformOverrides` -- see
`include/llvm/ADT/GenericUniformityImpl.h:818,855,1185`): even after
divergence has propagated through the operands, the value is forced
uniform.

When `AMDGPUUniformIntrinsicCombine` then queries `UniformityInfo` on a
`readlane(over-claimed-uniform, anyLane)` (or `permlane64` /
`wave_shuffle`), it sees a "uniform" value, strips the readlane, and
replaces uses with the divergent value directly.  Every lane now
observes its own per-lane value instead of the broadcast of the
requested lane.

The fix is to also require `Mask` to be uniform:

```cpp
if (match(V, m_c_And(m_Intrinsic<Intrinsic::amdgcn_workitem_id_x>(),
                     m_Value(Mask)))) {
  return isAlwaysUniform(Mask) &&
         computeKnownBits(Mask, DL).countMinTrailingZeros() >=
             ST->getWavefrontSizeLog2() &&
         XDimDoesntResetWithinWaves;
}
```

The sibling `m_LShr/m_AShr(tid_x, ConstantInt)` arm (lines 1212-1217)
is sound because the shift amount is a constant and `tid_x` itself is
the only operand whose divergence matters.

## Reproducer

`reduced.ll`:

```llvm
define amdgpu_kernel void @fuzz_kernel(ptr addrspace(1) %inp,
                                       ptr addrspace(1) %out) #0
                                       !reqd_work_group_size !0 {
entry:
  %tid   = call i32 @llvm.amdgcn.workitem.id.x()
  %gep   = getelementptr i32, ptr addrspace(1) %inp, i32 %tid
  %div   = load i32, ptr addrspace(1) %gep, align 4
  %mask  = shl i32 %div, 6                   ; 6 = log2(64) on wave64
  %val   = and i32 %tid, %mask               ; hook lies: AlwaysUniform
  %rfl   = call i32 @llvm.amdgcn.readlane(i32 %val, i32 0)
  %dst   = getelementptr i32, ptr addrspace(1) %out, i32 %tid
  store i32 %rfl, ptr addrspace(1) %dst, align 4
  ret void
}
!0 = !{i32 256, i32 1, i32 1}
```

(See the file for the full IR plus harness directives.)

## IR-level demonstration

`-O2` middle-end output (with the buggy hook):

```llvm
%mask = shl i32 %div, 6
%val  = and i32 %mask, %tid
store i32 %val, ptr addrspace(1) %dst, align 4    ; readlane gone
```

The `readlane` is stripped by `AMDGPUUniformIntrinsicCombine` --
`UniformityInfo` reported `%val` uniform thanks to the hook.

## Asm-level demonstration (gfx950)

```bash
ROOT=amdgpu
CLANG=$ROOT/build/llvm-fuzzer/bin/clang
LL=$ROOT/known-miscompiles/m096-tti-uniform-and-tid-divergent-mask/reduced.ll

# Correct codegen (hook bypassed):
$CLANG -O2 -nogpulib -target amdgcn-amd-amdhsa -mcpu=gfx950 \
  -mllvm -amdgpu-enable-uniform-intrinsic-combine=false \
  -S -x ir $LL -o /tmp/good.s

# Buggy codegen (default):
$CLANG -O2 -nogpulib -target amdgcn-amd-amdhsa -mcpu=gfx950 \
  -S -x ir $LL -o /tmp/bad.s
```

Good (asm contains `v_readlane_b32 s0, v0, 0` then broadcasts
`s0` to every lane):

```asm
v_and_b32_e32 v0, v2, v0
v_readlane_b32 s0, v0, 0
v_mov_b32_e32 v0, s0
global_store_dword v1, v0, s[2:3]
```

Bug (no `readlane`; each lane stores its own value):

```asm
v_and_b32_e32 v0, v2, v0
global_store_b32 v1, v0, s[2:3]
```

## Runtime check (gfx950, block_dim=256)

Inputs: lanes 0..63 -> 0; lanes 64..127 alternate 0/1; lanes 128..255 -> 0.

A correct compile broadcasts wave 1's lane-0 value (which is
`64 & (input[64] << 6) = 64 & 0 = 0`) to all of wave 1.  Expected:
output[64..127] = 0.

```
$ bash /tmp/audit-tti/runcompare.sh
[65] good=0x00000000 bad=0x00000040
[67] good=0x00000000 bad=0x00000040
[69] good=0x00000000 bad=0x00000040
[71] good=0x00000000 bad=0x00000040
[73] good=0x00000000 bad=0x00000040
[75] good=0x00000000 bad=0x00000040
[77] good=0x00000000 bad=0x00000040
[79] good=0x00000000 bad=0x00000040
total_mismatches=32/256
```

(32 mismatches = the 32 odd lanes in wave 1.)

Note: `run_ll_reproducer.sh -O0` does **not** catch the bug because
`createAMDGPUUniformIntrinsicCombineLegacyPass` is added to the
codegen pipeline unconditionally
(`AMDGPUTargetMachine.cpp:1446-1447`) and is on by default
(`AMDGPUTargetMachine.cpp:644-647`).  The O0-vs-O2 differential
collapses; we have to disable the pass on one side to get a clean
baseline.

## Toolchain Results

| Toolchain | Result |
| --- | --- |
| LLVM HEAD with the local PR patches (`build/llvm-fuzzer`) | Reproduces: middle-end strips readlane; 32/256 lanes wrong at runtime. |
| ROCm 7.1.1 (`/opt/rocm-7.1.1/lib/llvm/bin/clang-20`) | Does **not** reproduce -- `AMDGPUUniformIntrinsicCombine` is not in the pipeline; the readlane survives.  The TTI hook still over-claims, but no consumer exploits the false claim. |

The default-pipeline `clang -O2` miscompile is new to recent LLVM
(post-`AMDGPUUniformIntrinsicCombine` introduction).
