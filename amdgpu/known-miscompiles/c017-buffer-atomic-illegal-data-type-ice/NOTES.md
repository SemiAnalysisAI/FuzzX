# c017: `amdgcn.{raw,struct}.ptr.buffer.atomic.*` illegal data types ICE in SDAG

*Discovery method: code inspection (buffer.atomic audit; sibling of c011/c014/c015/c016).*

## The bug

`amdgpu/third_party/llvm-project/llvm/lib/Target/AMDGPU/SIISelLowering.cpp`:

* `lowerRawBufferAtomicIntrin` (`11196-11222`, mem node at `11219-11221`)
* `lowerStructBufferAtomicIntrin` (`11224-11250`, mem node at `11247-11249`)
* cmpswap raw arm (`11541-11563`)
* cmpswap struct arm (`11565-11587`)

Unlike `lowerIntrinsicLoad`'s 7739-7745 (which has a CastVT/bitcast
fallback for `buffer.load.format` with illegal value types), the
four buffer.atomic lowerings hand the user-typed value straight to
`getMemIntrinsicNode` with no illegal-type bitcast/widen path.

This applies to the `{add,sub,swap,and,or,xor,inc,dec,smin,smax,umin,umax,...}`
arms and the `fadd` arm (with `llvm_anyfloat_ty`).

## Reproducer matrix

All ICE at -O0 and -O2 on gfx950 (`build/llvm-fuzzer/bin/llc` and
`build/rocm-7.2.3-llvm-cov-release/bin/llc`):

| intrinsic / type | error |
| --- | --- |
| `raw.ptr.buffer.atomic.add.v3i16`   | `Do not know how to widen the result of this operator!` |
| `raw.ptr.buffer.atomic.add.i128`    | `Do not know how to expand the result of this operator!` |
| `raw.ptr.buffer.atomic.swap.v6i8`   | widen result |
| `raw.ptr.buffer.atomic.swap.i24`    | **segfault** (no LLVM_ERROR) |
| `raw.ptr.buffer.atomic.fadd.bf16`   | `Cannot select AMDGPUISD::BUFFER_ATOMIC_FADD bf16` |
| `struct.ptr.buffer.atomic.add.v3i16` | widen result |
| `struct.ptr.buffer.atomic.cmpswap.i128` | expand result |

At -O2 the `v3i16` raw case additionally hits a different assertion:
`trunc source and destination must both be a vector or neither`
(DAGCombiner pre-legalize).  GISel errors cleanly with "unable to
legalize" -- distinct path.

## Reproducer

`reduced.ll`:

```llvm
declare <3 x i16> @llvm.amdgcn.raw.ptr.buffer.atomic.add.v3i16(
    <3 x i16>, ptr addrspace(8), i32, i32, i32 immarg)

define amdgpu_kernel void @t(ptr addrspace(8) %rsrc, <3 x i16> %v,
                             ptr addrspace(1) %out) {
  %r = call <3 x i16> @llvm.amdgcn.raw.ptr.buffer.atomic.add.v3i16(
      <3 x i16> %v, ptr addrspace(8) %rsrc, i32 0, i32 0, i32 0)
  store <3 x i16> %r, ptr addrspace(1) %out
  ret void
}
```

## Suggested fix

Mirror `lowerIntrinsicLoad`'s illegal-type bitcast branch (CastVT
chosen as a legal carrier of the same byte size) in
`lowerRawBufferAtomicIntrin`, `lowerStructBufferAtomicIntrin`, and
the two `_atomic_cmpswap` arms, with a bitcast-back on the result.

Reject sub-byte / odd-bit types (i24, i1) before reaching
`getMemIntrinsicNode`, and gate bf16 fadd on
`hasAtomicBufferGlobalPkAddBF16Inst` or fall back to expansion.

## Why the fuzzer hasn't caught it

Per `MEMORY.md` (Prefer-random-over-idioms), the random emitter for
buffer.atomic should mint random data types including odd-lane vectors
(`<3 x i16>`, `<6 x i8>`, `<3 x bf16>`), wide scalars (i128),
sub-byte scalars (i24), and bf16 fadd.

## Toolchain Results

| Toolchain | Result |
| --- | --- |
| LLVM HEAD with the local PR patches (`build/llvm-fuzzer`) | ICEs on all 7 variants. |
| ROCm 7.2.3 | Same defect on all 7 variants. |

## Family

* c011 (buffer.load.format TFE chain-drop with illegal data type).
* c014 (tbuffer.load illegal-vector ICE).
* c015 (buffer.load.format.i8 drops format encoding).
* c016 (s.buffer.load illegal data type ICE).
* c017 (buffer.atomic illegal data type ICE) -- this entry.

All five are sibling defects in the buffer/tbuffer/s_buffer/atomic
lowering family.  Note: no `s.buffer.atomic` intrinsic exists in
LLVM (only `s.buffer.load`).
