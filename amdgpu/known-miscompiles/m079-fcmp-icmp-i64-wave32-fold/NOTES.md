# m079: `amdgcn.fcmp`/`amdgcn.icmp` InstCombine "always-true" fold uses 64-bit EXEC on wave32 -- sibling miscompile to c007 ICE

*Discovery method: code inspection.*  While auditing the
`amdgcn_icmp` / `amdgcn_fcmp` constant folder in
`AMDGPUInstCombineIntrinsic.cpp` for the wave-size-vs-return-type
mismatch documented in `c007-fcmp-i32-wave64-fold-ice`, the **mirror**
case turns out to silently miscompile instead of ICEing.

* `c007` (wave64, `.i32` return): the fold creates
  `read_register("exec", i32)`, ISel sees an i32 EXEC and crashes with
  *"invalid type for register exec"*.
* **This bug** (wave32, `.i64` return): the same fold creates
  `read_register("exec", i64)`.  `"exec"` always maps to the 64-bit
  register pair `AMDGPU::EXEC`, so on wave32 the high 32 bits of the
  result come from `EXEC_HI`, which is architecturally unused in wave32
  (i.e. not initialised by the SDAG path).  The SDAG path used at -O0
  emits `v_cmp_* -> sgpr32`, then `zext i32 -> i64`, so the high 32
  bits are guaranteed to be zero.

## The buggy fold

`amdgpu/third_party/llvm-project/llvm/lib/Target/AMDGPU/AMDGPUInstCombineIntrinsic.cpp:1633-1655`

```cpp
if (auto *CSrc0 = dyn_cast<Constant>(Src0)) {
  if (auto *CSrc1 = dyn_cast<Constant>(Src1)) {
    Constant *CCmp = ConstantFoldCompareInstOperands(
        (ICmpInst::Predicate)CCVal, CSrc0, CSrc1, DL);
    if (CCmp && CCmp->isNullValue()) {
      return IC.replaceInstUsesWith(
          II, IC.Builder.CreateSExt(CCmp, II.getType()));
    }
    // ... "always true" path ...
    CallInst *NewCall = IC.Builder.CreateIntrinsic(Intrinsic::read_register,
                                                   II.getType(), Args);
```

`II.getType()` is whatever width the user spelled on the intrinsic name
(`.i32` or `.i64`).  The fold blindly uses it as the type for
`read_register("exec", ...)`, with no check that the type matches the
subtarget wave size.  Compare against the SDAG path
(`SIISelLowering.cpp:7748-7779`, `lowerICMPIntrinsic`), which correctly
constructs the compare at `getWavefrontSize()` bits and then
`getZExtOrTrunc`'s to `II.getType()`.

## Reproducer

`/tmp/findbug/icmp/reduced.ll`:

```llvm
target triple = "amdgcn-amd-amdhsa"

define protected amdgpu_kernel void @fuzz_kernel(ptr addrspace(1) %out) #0 {
entry:
  %r = call i64 @llvm.amdgcn.fcmp.i64.f32(float 0.0, float 0.0, i32 1)
  store i64 %r, ptr addrspace(1) %out, align 8
  ret void
}

declare i64 @llvm.amdgcn.fcmp.i64.f32(float, float, i32 immarg)

attributes #0 = { convergent nounwind "target-cpu"="gfx1030" }
```

The FuzzX CI box only has a wave64 (gfx950) GPU, so this is shown as a
static (assembly-level) miscompile instead of a runtime mismatch.

### Build commands

```bash
CLANG=amdgpu/build/llvm-fuzzer/bin/clang
$CLANG -O0 -target amdgcn-amd-amdhsa -mcpu=gfx1030 -nogpulib \
       -S /tmp/findbug/icmp/reduced.ll -o /tmp/findbug/icmp/o0.s
$CLANG -O2 -target amdgcn-amd-amdhsa -mcpu=gfx1030 -nogpulib \
       -S /tmp/findbug/icmp/reduced.ll -o /tmp/findbug/icmp/o2.s
```

### -O0 (SDAG path, correct)

```asm
v_cmp_eq_f32_e64 s7, s6, s6        ; low 32 bits = compare result
s_mov_b32 s6, 0                    ; high 32 bits = zero (zext)
v_mov_b32_e32 v1, s7
v_mov_b32_e32 v3, s6
global_store_dwordx2 v0, v[1:2], s[4:5]
```

### -O2 (InstCombine fold, MISCOMPILE)

```asm
v_mov_b32_e32 v0, exec_lo          ; low 32 bits = EXEC_LO  (OK)
v_mov_b32_e32 v1, exec_hi          ; high 32 bits = EXEC_HI (WRONG)
v_mov_b32_e32 v2, 0
global_store_dwordx2 v2, v[0:1], s[0:1]
```

The two `.s` files differ in the high i32 of the stored result.  Whether
this is observable at runtime depends on what the hardware/firmware
leaves in `EXEC_HI` on a wave32 wave; the LLVM SDAG path treats it as
"definitely zero", InstCombine treats it as "whatever the register
contains".

The same `.i64` reproducer also reproduces with `amdgcn.icmp` (verified
locally) -- both branches share the same fold.

## Toolchain Results

| Toolchain                                                  | -O2 asm contains `exec_hi`? |
| ---------------------------------------------------------- | --------------------------- |
| LLVM HEAD with the local PR patches (`build/llvm-fuzzer`)  | Yes (reproduces)            |
| ROCm 7.1.1 (`/opt/rocm-7.1.1/lib/llvm/bin/clang-20`)        | Yes (reproduces)            |

## How a fix should look

The InstCombine fold needs the wave-size check that the SDAG path
already has.  Two reasonable options:

1. Bail out of the fold when `II.getType()->getBitWidth() !=
   Subtarget.getWavefrontSize()`, leaving SDAG to do the
   compare-and-zext/trunc.
2. Build the `read_register` at wave-size width and follow it with the
   appropriate `zext`/`trunc` to `II.getType()`.

The same check would also fix `c007` (which is the wave64/.i32 ICE
counterpart).

## Cases ruled out while auditing the fold

* **Predicate swap on canonicalization** (line 1657-1664): correct --
  uses `CmpInst::getSwappedPredicate`, which handles FCMP `FALSE`/`TRUE`
  and ordered/unordered predicates correctly.
* **NaN handling on `fcmp ueq nan, nan` etc.**: the underlying
  `ConstantFoldCompareInstOperands` does the right thing -- `oeq` of
  NaN folds to `i1 0` (NULL branch, sext to 0 wave mask), `ueq` of NaN
  folds to `i1 1` (read EXEC, which is correct **when the wave size
  matches the return type**).
* **Signed vs unsigned `amdgcn.icmp`**: predicate is passed straight
  through to `getICmpCondCode`, no sign flip.
* **One constant, one runtime input**: canonicalises constant to RHS
  with `getSwappedPredicate` and recurses -- no width issue, no fold.
* **`undef` operand**: `fcmp undef, undef` folds via the standard rule
  (`isUnordered(P) ? 1 : 0`); for `icmp eq/ne` it folds to `UndefValue`,
  which the InstCombine code treats as "true" (read EXEC) -- defensible
  since EXEC is a valid refinement of undef.
* **Boundary predicate values (FCMP_FALSE = 0, FCMP_TRUE = 15, ICMP_SLE
  = 41)**: in-range guards at lines 1624-1628 are correct.
