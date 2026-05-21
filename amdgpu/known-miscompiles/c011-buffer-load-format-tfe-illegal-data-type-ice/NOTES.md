# c011: `amdgcn.struct.ptr.buffer.load.format` with TFE + illegal data type (`<3 x i16>`) ICEs in SDAG legalization

*Discovery method: code inspection + HW-verified reproducer.*

Sibling shape to c008/c010 (intrinsic + bf16/illegal type ICE family)
and m143 (strict-FP bf16 chain drop).  Different defect: TFE result
tuple is mis-handled when the data type requires type-legalization.

## The bug

`amdgpu/third_party/llvm-project/llvm/lib/Target/AMDGPU/SIISelLowering.cpp:7707-7746,
8256-8270`.

Two cooperating defects:

1. **`SITargetLowering::lowerIntrinsicLoad` illegal-type branch (line 7740):**

```cpp
SDVTList VTList = DAG.getVTList(CastVT, MVT::Other);
```

This unconditionally builds a 2-result VTList for the
illegal-type/CastVT path, regardless of `IsTFE`.  The TFE i32 result
is never plumbed in this branch.  (The legal-type branch at line
7735 correctly uses `M->getVTList()`.)

Lines 7743-7745 then return `MERGE_VALUES` of 2 values only.

2. **`SITargetLowering::ReplaceNodeResults` `ISD::INTRINSIC_W_CHAIN`
   else branch (line 8263-8266):**

```cpp
} else {
  Results.push_back(Res);
  Results.push_back(Res.getValue(1));
}
```

Pushes exactly 2 results regardless of `N->getNumValues()`.  For a
3-value TFE node `(data, status:i32, chain:Other)`, the 3rd value is
dropped, leading to mismatched `ReplaceAllUsesWith` -> crash.

Reachability: TFE enabled AND data type is illegal (neither D16 at
line 7725 nor scalar <32-bit at line 7730).  `<3 x i16>` qualifies.
`<6 x i16>` and `<3 x bfloat>` likely also reproduce.

## Reproducer

`reduced.ll`:

```llvm
declare {<3 x i16>, i32}
  @llvm.amdgcn.struct.ptr.buffer.load.format.sl_v3i16i32s(
      ptr addrspace(8), i32, i32, i32, i32 immarg)

define amdgpu_kernel void @t(ptr addrspace(8) %rsrc,
                             ptr addrspace(1) %out,
                             ptr addrspace(1) %status) {
  %r = call {<3 x i16>, i32}
    @llvm.amdgcn.struct.ptr.buffer.load.format.sl_v3i16i32s(
      ptr addrspace(8) %rsrc, i32 0, i32 0, i32 0, i32 0)
  %d = extractvalue {<3 x i16>, i32} %r, 0
  %s = extractvalue {<3 x i16>, i32} %r, 1
  store <3 x i16> %d, ptr addrspace(1) %out
  store i32 %s, ptr addrspace(1) %status
  ret void
}
```

`llc -mtriple=amdgcn -mcpu=gfx950 -O0 reduced.ll`: crashes in
`SelectionDAG::ReplaceAllUsesWith` (Legalize phase).  Same at -O2.
GISel cleanly errors with "unable to legalize" -- distinct path.

(Note: the *raw* variant `raw.ptr.buffer.load.format.v3i16` does
not accept the `{T,i32}` TFE tuple because it uses `llvm_anyfloat_ty`.
The **struct** variant uses `llvm_any_ty` so `<3 x i16>` is the right
hook.)

## Suggested fix

In `lowerIntrinsicLoad` illegal-type branch, build VTList based on
`IsTFE`:

```cpp
SDVTList VTList = IsTFE
    ? DAG.getVTList(CastVT, MVT::i32, MVT::Other)
    : DAG.getVTList(CastVT, MVT::Other);
```

And in the MERGE_VALUES construction at 7743-7745, include the
status result when `IsTFE` is set.

In `ReplaceNodeResults` else branch (8263-8266), iterate
`N->getNumValues()` similar to the MERGE_VALUES arm at 8258-8262:

```cpp
} else {
  for (unsigned I = 0; I < N->getNumValues(); ++I)
    Results.push_back(Res.getValue(I));
}
```

## Why the fuzzer hasn't caught it

* `amdgcn.struct.ptr.buffer.load.format` with TFE is a niche shape;
  the IR fuzzer rarely emits the `{vec, i32}` aggregate-return form
  with an illegal vector element count.
* Per `MEMORY.md` (Prefer-random-over-idioms), the random emitter
  should generate `amdgcn.struct.ptr.buffer.load.format` with random
  result aggregates including odd-lane vectors (v3/v6 of i16/bf16/
  i32/f32).

## Toolchain Results

| Toolchain | Result |
| --- | --- |
| LLVM HEAD with the local PR patches (`build/llvm-fuzzer`) | ICE at -O0 and -O2 in SelectionDAG legalization. |
| ROCm 7.1.1 | Same defect. |
