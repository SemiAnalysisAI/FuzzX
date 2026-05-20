# m082: AMDGPULowerKernelArguments transplants `range` ParamAttr onto widened i32 kernarg load with mismatched operand type

*Discovery method: code inspection.*

## The bug

`AMDGPULowerKernelArguments.cpp:288-314` widens any sub-dword scalar
kernel-argument load to i32 (the `DoShiftOpt` path, since pre-GFX12
hardware has no sub-dword scalar loads):

```cpp
bool DoShiftOpt = Size < 32 && !ArgTy->isAggregateType();
// ...
if (DoShiftOpt) {
  ArgPtr = Builder.CreateConstInBoundsGEP1_64(
      Builder.getInt8Ty(), KernArgSegment, AlignDownOffset, /*...*/);
  AdjustedArgTy = Builder.getInt32Ty();         // <-- load is i32 now
}
// ...
LoadInst *Load = Builder.CreateAlignedLoad(AdjustedArgTy, ArgPtr, AdjustedAlign);
```

Immediately after, lines 322-327 transplant the argument's `range`
ParamAttr onto the *widened* load without re-typing it:

```cpp
if (Arg.hasAttribute(Attribute::Range)) {
  const ConstantRange &Range =
      Arg.getAttribute(Attribute::Range).getValueAsConstantRange();
  Load->setMetadata(LLVMContext::MD_range,
                    MDB.createRange(Range.getLower(), Range.getUpper()));
}
```

`ConstantRange.getLower()/getUpper()` return `APInt`s of the
*argument's* bit width (e.g. i8 for `i8 range(i8 0, 4) %a`).
`MDBuilder::createRange` materialises a `ConstantInt` of that same
type. The result is `!range !{i8 0, i8 4}` attached to an `i32` load.

This is two bugs in one:

1. **The IR is invalid.** `Verifier.cpp:4602` requires the range MD's
   element type to equal the instruction's scalar type:
   ```
   Range types must match instruction type!
     %0 = load i32, ptr addrspace(4) %a.kernarg.offset.align.down,
                                     align 16, !range !0
   ```
   The IR verifier therefore aborts compilation if `--verify-each`
   runs.  The Clang -O2 pipeline only verifies at the boundaries, so
   normal compiles slip through.

2. **It is semantically wrong.** The widened i32 load reads the entire
   dword that contains `%a`. With a sub-dword arg, the high bits of
   that dword contain whatever the *next* kernarg (`%b` in the
   reproducer) and/or padding contributed -- they are NOT constrained
   by `%a`'s range attribute. Telling downstream passes "this i32 is
   in `[0, 4)`" lets them prove the high bits zero and fold away any
   computation that depends on `%b`. The narrow i8 view of `%a` only
   recovers as `trunc i32 %0 to i8`, so by the time the actual i8 is
   live, range MD on the i32 load has already misinformed analysis on
   the un-truncated bits.

Compare with the `nonnull`/`dereferenceable`/`align` block above
(lines 329-356): those are guarded by `if (isa<PointerType>(ArgTy))`,
so widening doesn't affect them.

Compare also with `Arg.hasAttribute(Attribute::NoUndef)` at lines
319-320: `noundef` is set on the widened load unconditionally. That
is *also* unsound when the dword spans uninitialised padding between
explicit kernargs, but it's harder to weaponise than `range`.

## Reproducer

`/tmp/findbug/kernarg/reduced.ll`:

```llvm
target triple = "amdgcn-amd-amdhsa"

define protected amdgpu_kernel void @fuzz_kernel(i8 range(i8 0, 4) %a, i8 %b,
                                                 ptr addrspace(1) %out) #0 {
entry:
  %za  = zext i8 %a to i32
  %zb  = zext i8 %b to i32
  %shl = shl i32 %zb, 8
  %sum = or i32 %shl, %za
  store i32 %sum, ptr addrspace(1) %out, align 4
  ret void
}

attributes #0 = { nounwind "target-cpu"="gfx950" }
```

## Demonstration #1 -- verifier rejects the lowered IR

```
$ /opt/rocm-7.1.1/lib/llvm/bin/opt -passes=amdgpu-lower-kernel-arguments \
    -mtriple=amdgcn-amd-amdhsa -mcpu=gfx950 -S reduced.ll
Range types must match instruction type!
  %0 = load i32, ptr addrspace(4) %a.kernarg.offset.align.down,
                 align 16, !range !0, !invariant.load !1
LLVM ERROR: Broken module found, compilation aborted!
```

Same with the FuzzX build (`build/rocm-7.2.3-extract/.../opt`) and
with ROCm 7.1.1's `clang-20`.

## Demonstration #2 -- bad MD slips into the pipeline

Stop-after the pass and dump the IR:

```
$ clang -O2 -mcpu=gfx950 -nogpulib -S -mllvm \
    -stop-after=amdgpu-lower-kernel-arguments reduced.ll -o reduced.mir
...
%0 = load i32, ptr addrspace(4) %a.kernarg.offset.align.down,
              align 16, !range !0, !invariant.load !1
...
!0 = !{i8 0, i8 4}        ; <-- i8 range MD on i32 load
```

## Toolchain Results

| Toolchain                                                  | Verifier rejects? | Bad MD reaches codegen? |
| ---------------------------------------------------------- | ----------------- | ----------------------- |
| LLVM HEAD with the FuzzX patches (`build/llvm-fuzzer`)     | Yes               | Yes (no in-pipeline verify) |
| ROCm 7.2.3 (`build/rocm-7.2.3-extract/.../opt`)            | Yes               | Yes                     |
| ROCm 7.1.1 (`/opt/rocm-7.1.1/lib/llvm/bin/opt`)            | Yes               | Yes                     |

## How a fix should look

Two minimally-invasive options, mirroring the same fixes available
for `c007`/`m079`:

1. Drop the range MD on the `DoShiftOpt` path -- the `trunc` to i8
   that follows already discards bits ≥ Size, and InstCombine will
   propagate the i8 range from the trunc's user (`zext i8 -> i32`)
   anyway via `computeKnownBits`.
2. Build the range MD at the load's bit-width by zero-extending the
   ConstantRange:
   ```cpp
   ConstantRange Wide = Range.zeroExtend(
       cast<IntegerType>(AdjustedArgTy)->getBitWidth());
   ```
   This is only sound when `OffsetDiff == 0` AND no later kernarg
   spans bits `[Size, 32)` of the widened load -- i.e. effectively
   never for two adjacent sub-dword args. Option (1) is simpler.

## Cases ruled out while auditing

* `nonnull` / `dereferenceable` / `dereferenceable_or_null` /
  `align`: guarded by `isa<PointerType>(ArgTy)`, never reach the
  widened-load path (pointers are always ≥ 32 bits, so `DoShiftOpt`
  is false).
* `noundef` (lines 319-320): set on the widened load unconditionally.
  Unsound when the dword spans padding between explicit args, but
  amdhsa launch usually zero-initialises padding, so the worst case
  is a downstream poison-vs-undef refinement, not a runtime
  miscompile of an otherwise-correct program. Documented but not
  reproduced.
* `IsV3` widening (lines 307-311): the V3->V4 hack only applies when
  `Size >= 32`, so it does not interact with the sub-dword `DoShiftOpt`
  metadata path.
* `AlignDownOffset` arithmetic: `alignDown(EltOffset, 4)` and
  `commonAlignment(KernArgBaseAlign=16, AlignDownOffset)` are
  correct; the load's `align` operand on the widened load matches.
* Byref args (line 253): take the address-cast path; no load
  emitted; no MD issue.
* Inreg / preload args (line 248): skipped entirely; preload pass
  handles them separately.
* `AMDGPULowerKernelAttributes.cpp` range-MD annotations
  (`annotateGridSizeLoadWithRangeMD` and friends): each guards on
  `Load->getType()->isIntegerTy(N)` matching its `APInt(N, ...)`, so
  no width mismatch there.
* `AMDGPUPreloadKernelArguments.cpp`: doesn't emit loads, only marks
  args `inreg` and clones the function signature.  Offset arithmetic
  matches the explicit-load offset arithmetic in
  `AMDGPULowerKernelArguments.cpp`.
* `AMDGPULowerExecSync.cpp`: not actually a kernel-argument pass
  (despite the directory) -- handles named-barrier LDS globals; no
  kernarg interaction.
