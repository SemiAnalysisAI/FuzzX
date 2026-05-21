# c018: `llvm.amdgcn.image.atomic.<op>.<dim>` ICEs/miscompiles in SDAG for any data width != 32/64 bits

*Discovery method: code inspection (image.atomic audit; sibling of
m142, c011, c014, c016, c017).*

## The bug

`amdgpu/third_party/llvm-project/llvm/lib/Target/AMDGPU/SIISelLowering.cpp:10156-10181`
(`SITargetLowering::lowerImage`, atomic branch):

```cpp
bool Is64Bit = VData.getValueSizeInBits() == 64;        // L10165
if (BaseOpcode->AtomicX2) {
  VData = DAG.getBuildVector(Is64Bit ? MVT::v2i64 : MVT::v2i32, ...);
  if (Is64Bit)
    VData = DAG.getBitcast(MVT::v4i32, VData);          // L10171
  ResultTypes[0]  = Is64Bit ? MVT::v2i64 : MVT::v2i32;
  DMask           = Is64Bit ? 0xf : 0x3;
  NumVDataDwords  = Is64Bit ? 4 : 2;
} else {
  DMask           = Is64Bit ? 0x3 : 0x1;
  NumVDataDwords  = Is64Bit ? 2 : 1;                    // L10180
}
```

The branch is a binary 32-vs-64 dispatch on the source data width
and ignores every other case.  `int_amdgcn_image_atomic_swap` is
declared `llvm_any_ty` (`IntrinsicsAMDGPU.td:1387`), so the
overload set includes `<3 x i32>`, `<3 x i16>`, `<6 x i16>`,
`<3 x bfloat>`, `i128`, `bfloat`, etc.  All hit the
`Is64Bit == false` arm with `NumVDataDwords = 1`, `DMask = 0x1`,
and the MIMG selector picks the **1-dword V1 opcode** regardless
of the actual VData register-class width.

bf16 additionally lands here because the only D16 / packed paths
the atomic branch knows about are `IMAGE_ATOMIC_PK_ADD_{F16,BF16}`
(L10160-10163), not generic bf16 swap/add/and/etc.  Same MVT::f16-only
check as m142 in the non-atomic arms.

## Symptom matrix

gfx950, `-global-isel=0`, `-O0` and `-O2`, both `build/llvm-fuzzer`
and ROCm 7.2.3:

| data type (`image.atomic.swap.1d.<T>`) | SDAG behavior |
| --- | --- |
| `<3 x i32>` | crash in `copyPhysReg` (post-RA expand) at O0; crash in `MCInstPrinter::printOperand` (AsmPrinter) at O2 |
| `<3 x i16>` | `Do not know how to widen the result of this operator` |
| `<6 x i16>` | same |
| `<3 x bfloat>` | same |
| `i128` | `Do not know how to expand the result of this operator` |
| `bfloat` | **SILENT MISCOMPILE** -- emits 1-dword `image_atomic_swap dmask:0x1` with garbage upper 16 bits of the dword overwriting the texel |
| `i128` cmpswap | `Cannot select: v2i32 = bitcast v4i64` (L10171 unconditional bitcast wrong for 128-bit-per-lane) |

GISel cleanly errors `unable to legalize` on `<3 x i32>` -- SDAG-only.

## Reproducer

`reduced.ll`:

```llvm
declare <3 x i32> @llvm.amdgcn.image.atomic.swap.1d.v3i32.i32(
    <3 x i32>, i32, <8 x i32>, i32, i32)

define amdgpu_kernel void @t(<8 x i32> inreg %r, i32 %x,
                             <3 x i32> %v, ptr addrspace(1) %o) {
  %res = call <3 x i32> @llvm.amdgcn.image.atomic.swap.1d.v3i32.i32(
      <3 x i32> %v, i32 %x, <8 x i32> %r, i32 0, i32 0)
  store <3 x i32> %res, ptr addrspace(1) %o
  ret void
}
```

The `<3 x i32>` O0 case is the most damaging: post-RA the lowering
emits `IMAGE_ATOMIC_SWAP_V1_V1_gfx90a` (1-dword opcode) consuming a
3-dword VData register (`s96` memop), then `copyPhysReg` ICEs on
the impossible move.

## bf16 silent miscompile

`bfloat` falls into `Is64Bit == false`, `NumVDataDwords = 1`,
`DMask = 0x1`.  HW executes `image_atomic_swap` with the i16 bf16
value occupying the low 16 bits of the dword while the upper 16
bits hold whatever register-junk was zero-extended.  The texel's
upper 16 bits are silently corrupted.  No diagnostic.

## Suggested fix

* Compute `DataDwords = (VData.getValueSizeInBits() + 31) / 32`
  (same as the store path at L10198) instead of branching on
  `Is64Bit`.
* For `AtomicX2`, require per-lane width to be 32 or 64; bail
  (`return Op`) otherwise.
* For bf16 / `<N x bfloat>`, route to the D16 opcode the same way
  the load/store arms should (m142 fix), or bail.

## Why the fuzzer hasn't caught it

* No upstream lit test exercises `image.atomic.swap.<dim>` with any
  data type other than i32 / i64 (`grep
  llvm/test/CodeGen/AMDGPU/llvm.amdgcn.image.atomic*.ll`).
* Per `MEMORY.md` (Prefer-random-over-idioms), the random IR
  emitter should mint `amdgcn.image.atomic.{swap,add,and,or,xor,
  cmpswap,...}.<dim>` with non-stock data overloads.

## Toolchain Results

| Toolchain | Result |
| --- | --- |
| LLVM HEAD with local PR patches (`build/llvm-fuzzer`) | ICE on v3i32/v3i16/v6i16/v3bf16/i128/i128-cmpswap; silent miscompile on bf16. |
| ROCm 7.2.3 | Same defects on all variants. |

## Family

* m142 (image D16 lowering misses bf16 in non-atomic arms).
* c011 (buffer.load.format TFE chain-drop with illegal data type).
* c014 (tbuffer.load illegal-vector ICE).
* c015 (buffer.load.format.i8 drops format encoding).
* c016 (s.buffer.load illegal-data-type ICE).
* c017 (buffer.atomic illegal-data-type ICE).
* c018 (image.atomic illegal-data-type ICE/miscompile) -- this entry.

All sibling defects in the buffer/tbuffer/s_buffer/buffer.atomic/
image.atomic lowering family.  Same root cause: lowering helper
only knows the legal-type shape, no illegal-type bitcast/widen
fallback.
