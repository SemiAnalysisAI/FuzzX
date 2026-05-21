# c014: `amdgcn.raw.ptr.tbuffer.load.v3i16` (and v6i16/v3bf16) ICE in SDAG

*Discovery method: code inspection (tbuffer.load/store audit; sibling of c011).*

## The bug

`amdgpu/third_party/llvm-project/llvm/lib/Target/AMDGPU/SIISelLowering.cpp:11394-11399`
(raw) and `:11421-11426` (struct), plus the store mirror at
`:12053-12107`.

The D16 fast-path checks **only `MVT::f16`** scalar type and routes
through `adjustLoadValueType` (which handles odd-lane widening).

All other illegal vector return types -- `<3 x i16>`, `<6 x i16>`,
`<3 x bfloat>` -- skip that branch and fall through to a plain:

```cpp
getMemIntrinsicNode(AMDGPUISD::TBUFFER_LOAD_FORMAT, ..., LoadVT, ...)
```

with the illegal-typed value.

There is **no equivalent of the `buffer.load.format` illegal-type
bitcast branch** (`lowerIntrinsicLoad` lines 7739-7745).  The
illegal-typed `INTRINSIC_W_CHAIN` then reaches `ReplaceNodeResults`
`INTRINSIC_W_CHAIN` (line 8256), which returns the still-illegal
`<3 x i16>` value -> legalizer reports:

```
LLVM ERROR: Do not know how to widen the result of this operator!
```

and aborts.

## Distinction from c011

c011 is a **TFE-tuple chain-drop in `lowerIntrinsicLoad` +
`ReplaceNodeResults`**.  The tbuffer intrinsics do NOT have a TFE
form (defined in `IntrinsicsAMDGPU.td:1857-1960` with single
`llvm_any_ty` result, no `{T, i32}` overload), so no TFE chain-drop
is possible.

The defect here is **missing illegal-vector handling** on the
non-D16 path -- sibling defect, same root-cause family (lowering
helpers used by buffer.load.format never ported to tbuffer.load).

## Reproducer matrix

All ICE at -O0 and -O2 (`build/llvm-fuzzer/bin/llc -mcpu=gfx950`):

| intrinsic | result |
| --- | --- |
| `llvm.amdgcn.raw.ptr.tbuffer.load.v3i16` | ICE |
| `llvm.amdgcn.raw.ptr.tbuffer.load.v6i16` | ICE |
| `llvm.amdgcn.raw.ptr.tbuffer.load.v3bf16` | ICE |
| `llvm.amdgcn.struct.ptr.tbuffer.load.v3i16` | ICE |
| `llvm.amdgcn.raw.ptr.tbuffer.store.v3i16` | ICE |
| `llvm.amdgcn.raw.ptr.tbuffer.load.v3i32` | OK (hasDwordx3LoadStores widens at 11910-11921) |
| `llvm.amdgcn.raw.ptr.tbuffer.load.v3f16` | OK (D16 path) |

## Reproducer

`reduced.ll`:

```llvm
declare <3 x i16> @llvm.amdgcn.raw.ptr.tbuffer.load.v3i16(
    ptr addrspace(8), i32, i32, i32 immarg, i32 immarg)

define amdgpu_kernel void @t(ptr addrspace(8) %rsrc,
                             ptr addrspace(1) %out) {
  %r = call <3 x i16> @llvm.amdgcn.raw.ptr.tbuffer.load.v3i16(
      ptr addrspace(8) %rsrc, i32 0, i32 0, i32 0, i32 0)
  store <3 x i16> %r, ptr addrspace(1) %out
  ret void
}
```

## Suggested fix

In both `tbuffer.load` arms (raw 11394, struct 11421) and the
store mirrors (12056, 12084), broaden the D16 detection to all
16-bit scalar types, OR add a CastVT fallback paralleling lines
7739-7745 of `lowerIntrinsicLoad`:

```cpp
if (LoadVT.getScalarType().getSizeInBits() == 16)
  return adjustLoadValueType(AMDGPUISD::TBUFFER_LOAD_FORMAT_D16, M, DAG, Ops);
// or: if (!isTypeLegal(LoadVT)) { CastVT path with bitcast-back }
```

Note `adjustLoadValueType`'s VTList at 7696 is already 2-VT-only
which is correct for tbuffer (no TFE), so no further fix needed
there.

## Why the fuzzer hasn't caught it

* The IR fuzzer emits tbuffer.load/store with canonical
  `f32/v2f32/v4f32` per the `IntrinsicsAMDGPU.td` comment
  "overloaded for types f32/i32, v2f32/v2i32, v4f32/v4i32".  Per
  `MEMORY.md` (Prefer-random-over-idioms), the random emitter
  should produce tbuffer with random vector return types including
  odd-lane integer/bf16 vectors (`<3 x i16>`, `<6 x i16>`,
  `<3 x bfloat>`).

## Toolchain Results

| Toolchain | Result |
| --- | --- |
| LLVM HEAD with the local PR patches (`build/llvm-fuzzer`) | ICE at -O0 and -O2. |
| ROCm 7.1.1 | Same defect. |

## Family

* c011 (`amdgcn.struct.ptr.buffer.load.format` TFE + illegal data
  type ICE) -- sibling defect, different root cause (TFE chain
  drop vs missing illegal-vector handling).
* c008 (`amdgcn.class.bf16` ICE), c010 (STRICT_FP_EXTEND bf16) --
  bf16/illegal type ICE family.
