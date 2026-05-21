# c008: `llvm.amdgcn.class.bf16` ICEs in ISel with "Cannot select AMDGPUISD::FP_CLASS bf16"

*Discovery method: code inspection.*

## The bug

`amdgpu/third_party/llvm-project/llvm/lib/Target/AMDGPU/SIISelLowering.cpp:10931-10933`
(`LowerINTRINSIC_WO_CHAIN`, case `Intrinsic::amdgcn_class`):

```cpp
case Intrinsic::amdgcn_class:
  return DAG.getNode(AMDGPUISD::FP_CLASS, DL, MVT::i1,
                     Op.getOperand(1), Op.getOperand(2));
```

`int_amdgcn_class` is polymorphic over `llvm_anyfloat_ty`
(`IntrinsicsAMDGPU.td:548`).  This lowering unconditionally builds
`AMDGPUISD::FP_CLASS` for the source VT.

`VOPCInstructions.td:1223-1229` defines `VOPCClassPat64` patterns
only for `_F16`/`_F32`/`_F64` -- there is no `V_CMP_CLASS_BF16`
instruction (gfx950 has no bf16 class instruction; bf16 FP class
checks are normally expanded via i16 compares).

Result: `llvm.amdgcn.class.bf16` aborts ISel with:

```
LLVM ERROR: Cannot select: i1 = AMDGPUISD::FP_CLASS ... bf16
```

at both `-O0` and `-O2` on gfx950.

## Reproducer

`reduced.ll`:

```llvm
declare i1 @llvm.amdgcn.class.bf16(bfloat, i32)

define i1 @class_bf16(bfloat %x) {
  %r = call i1 @llvm.amdgcn.class.bf16(bfloat %x, i32 3)
  ret i1 %r
}
```

`llc -mtriple=amdgcn -mcpu=gfx950 -O0 reduced.ll`:

```
LLVM ERROR: Cannot select: i1 = AMDGPUISD::FP_CLASS ... bf16
```

## Suggested fix

Two options:

1. **Reject the bf16 overload at the IR layer.**  Restrict
   `int_amdgcn_class` to non-bf16 floats in `IntrinsicsAMDGPU.td`,
   matching the underlying hardware capability.

2. **Expand bf16 at the lowering layer.**  In
   `LowerINTRINSIC_WO_CHAIN` for `Intrinsic::amdgcn_class`, when
   `SrcVT == MVT::bf16`, bitcast source to `i16` and expand to the
   same integer-compare sequence that `ISD::IS_FPCLASS.bf16` already
   uses (`SIISelLowering.cpp` IS_FPCLASS legalization).  The generic
   `llvm.is.fpclass.bf16` already works fine -- the amdgcn-specific
   intrinsic should follow the same path.

Sibling shape to:

* `c001-sudot-isel-ice` -- intrinsic without target subtarget gate.
* `c003-permlane16-isel-ice` -- intrinsic legal on declaration but no
  selector for the relevant target generation.
* `c006-tanh-isel-ice` -- bf16 amdgcn intrinsic missing selector.
* `m118` -- bf16 over-promise in `isCanonicalized`.

## Why the fuzzer hasn't caught it

* The IR fuzzer rarely emits `llvm.amdgcn.class.bf16`.  Per
  `MEMORY.md` (Prefer-random-over-idioms), the right hook is to add
  `amdgcn.class.<ty>` to the random emitter's intrinsic pool with
  all valid FP type overloads.
* If a `FUZZX_ALLOW_C008_AMDGCN_CLASS_BF16_ISEL_ICE` env var is
  added to suppress this generation by default (like the existing
  c001 suppression), tests with `_=1` can re-enable.

## Toolchain Results

| Toolchain | Result |
| --- | --- |
| LLVM HEAD with the local PR patches (`build/llvm-fuzzer`) | ICEs at both -O0 and -O2. |
| ROCm 7.1.1 | Same defect. |

GISel also fails ("unable to translate") -- but this report is
SDAG-only.
