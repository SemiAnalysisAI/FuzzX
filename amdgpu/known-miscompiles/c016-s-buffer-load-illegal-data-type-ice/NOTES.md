# c016: `llvm.amdgcn.s.buffer.load.<T>` ICEs in SDAG for illegal data types

*Discovery method: code inspection (s_buffer_load audit; sibling of c011/c014/c015).*

## The bug

`amdgpu/third_party/llvm-project/llvm/lib/Target/AMDGPU/SIISelLowering.cpp:10537-10632`
(`SITargetLowering::lowerSBuffer`) handles only:

* `i16` at line 10559 (widened to i32)
* v3-of-legal-scalar at line 10567 (widened to v4)

All other illegal result types fall through to
`getMemIntrinsicNode(AMDGPUISD::SBUFFER_LOAD, ..., VT, ...)` at line
10579 holding an illegal value type.  The divergent path assertion
at line 10604-10605 restricts scalars to `i32`/`f32` only.

`SIISelLowering.cpp:8199-8246` (`ReplaceNodeResults` for
`Intrinsic::amdgcn_s_buffer_load`) hard-asserts `VT == MVT::i8` at
line 8212.  Any other illegal scalar reaching `ReplaceNodeResults`
(e.g. `i4`, `i24`) hits this assert in asserts builds; in NDEBUG
builds it falls through and emits `SBUFFER_LOAD_UBYTE` with the
wrong type, which then fails legalization.

## Reproducer matrix

All ICE at -O0 and -O2 on gfx950, both `build/llvm-fuzzer` and ROCm 7.2.3:

| return type | error |
| --- | --- |
| `i1` | `Cannot select SBUFFER_LOAD i1` |
| `i4` | `Do not know how to promote this operator` |
| `i24` | `Do not know how to promote this operator` |
| `<2 x i1>` | `Do not know how to split the result of this operator` |
| `<3 x i16>` | `Do not know how to widen the result of this operator` |
| `<6 x i8>` | `Do not know how to widen the result of this operator` |
| `i128` | `Do not know how to expand the result of this operator` |

## Reproducer (i128 variant)

`reduced.ll`:

```llvm
declare i128 @llvm.amdgcn.s.buffer.load.i128(<4 x i32>, i32, i32 immarg)

define amdgpu_kernel void @t(<4 x i32> %r, ptr addrspace(1) %o) {
  %v = call i128 @llvm.amdgcn.s.buffer.load.i128(<4 x i32> %r, i32 0, i32 0)
  store i128 %v, ptr addrspace(1) %o
  ret void
}
```

`llc -mtriple=amdgcn -mcpu=gfx950 -O0 reduced.ll`: ICE.

## Suggested fix

Mirror `lowerIntrinsicLoad`'s illegal-type bitcast branch (lines
7739-7745) and extend the vec3->vec4 widen at 10567 for the full
illegal-type space on both uniform and divergent paths.  Drop the
`VT == MVT::i8` assertion in `ReplaceNodeResults`; bitcast/promote
to a legal carrier type instead.

## Why the fuzzer hasn't caught it

Per `MEMORY.md` (Prefer-random-over-idioms), the random emitter
should mint `amdgcn.s.buffer.load` with non-stock return types
(`i1`/`i4`/`i24`/`i128`/`<v3 x i16>`/`<v6 x i8>`) on gfx950.

## Toolchain Results

| Toolchain | Result |
| --- | --- |
| LLVM HEAD with the local PR patches (`build/llvm-fuzzer`) | ICEs on all 7 variants at -O0 and -O2. |
| ROCm 7.2.3 | Same defect on all 7 variants. |

## Family

* c011 (buffer.load.format TFE chain-drop with illegal data type).
* c014 (tbuffer.load illegal-vector ICE).
* c015 (buffer.load.format.i8 drops format encoding).
* c016 (s.buffer.load illegal-data-type ICE) -- this entry.

All four are sibling defects in the buffer/tbuffer/s_buffer load
lowering family.
