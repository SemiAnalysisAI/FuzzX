# c015: `amdgcn.{raw,struct,struct.ptr}.buffer.load.format.i8` (and store) drop format encoding in SDAG

*Discovery method: code inspection (audit of buffer.load.format paths,
sibling of c011/c014).*

## The bug

`amdgpu/third_party/llvm-project/llvm/lib/Target/AMDGPU/SIISelLowering.cpp:7730-7732`
(`lowerIntrinsicLoad`) and `:12151-12153`, `:12202-12205` (store
mirrors).

For scalar `i8` format intrinsics, `lowerIntrinsicLoad` reaches:

```cpp
if (!IsD16 && !LoadVT.isVector() && EltType.getSizeInBits() < 32)
  return handleByteShortBufferLoads(DAG, LoadVT, DL, Ops,
                                    M->getMemOperand(), IsTFE);
```

`handleByteShortBufferLoads` (`SIISelLowering.cpp:12760`) ignores the
`IsFormat` flag and unconditionally emits
`AMDGPUISD::BUFFER_LOAD_UBYTE` / `BUFFER_LOAD_USHORT` (or `_TFE`
variants) -- **non-format opcodes**.  The buffer-rsrc format
descriptor is therefore not applied to the loaded byte.

The store mirror `handleByteShortBufferStores` (`:12798`) emits
`BUFFER_STORE_BYTE`/`SHORT` for the same reason.

i16 escapes the bug because the `IsD16` early-return at
`SIISelLowering.cpp:7725` routes 16-bit-element format loads to
`BUFFER_LOAD_FORMAT_D16` first.

## Distinction from c011 / c014

* **c011**: TFE chain-drop in the *vector* illegal-type branch.
* **c014**: missing illegal-vector handling on the *tbuffer* path.
* **c015** (this entry): byte/short scalar branch with
  **format-encoding loss** -- format intrinsic is silently lowered
  as a raw byte/short load/store.

Independent code path; cannot be subsumed by c011 or c014.

## Reproducer

`reduced.ll`:

```llvm
declare i8 @llvm.amdgcn.struct.ptr.buffer.load.format.i8(
    ptr addrspace(8), i32, i32, i32, i32 immarg)

define amdgpu_kernel void @t(ptr addrspace(8) %rsrc, ptr addrspace(1) %out) {
  %r = call i8 @llvm.amdgcn.struct.ptr.buffer.load.format.i8(
      ptr addrspace(8) %rsrc, i32 0, i32 0, i32 0, i32 0)
  store i8 %r, ptr addrspace(1) %out
  ret void
}
```

`llc -mtriple=amdgcn -mcpu=gfx950 -O2`:
* SDAG (broken): `buffer_load_ubyte v1, v0, s[0:3], 0 idxen`
* GISel (correct): `buffer_load_format_x v1, v0, s[0:3], 0 idxen`

Same defect for `.raw.buffer.load.format.i8`,
`.struct.buffer.load.format.i8`, and the symmetric `.store.format.i8`
variants (`buffer_store_byte` instead of `buffer_store_format_x`).
The `raw.ptr.*.format` variants are gated to `llvm_anyfloat_ty`, so
i8 isn't reachable through them.

## Suggested fix

Option A: Pass `IsFormat` into `handleByteShortBufferLoads/Stores`
and select `BUFFER_LOAD_FORMAT_X` / `BUFFER_STORE_FORMAT_X` opcodes.
Requires adding i8/i16 patterns in `BUFInstructions.td:1449-1456` /
`:1566-1572` that load via i32-format then trunc/extend.

Option B (matches GISel): Bail out before line 7730 for
`IsFormat && i8`, falling through to a CastVT-style path that issues
`BUFFER_LOAD_FORMAT_X` to an i32 then truncates.

## Why the fuzzer hasn't caught it

* The IR fuzzer emits `buffer.load.format` with canonical `f32`/`v4f32`
  result types per `IntrinsicsAMDGPU.td` comments.  Per `MEMORY.md`
  (Prefer-random-over-idioms), the random emitter should pick `i8` as
  a scalar result type for `struct/struct.ptr` `buffer.{load,store}.format`
  intrinsic families.
* The defect is a semantic miscompile (format descriptor ignored),
  not an ICE, so detection requires HW-verified runtime comparison
  of formatted vs raw byte interpretation.

## Toolchain Results

| Toolchain | Result |
| --- | --- |
| LLVM HEAD with the local PR patches (`build/llvm-fuzzer`) | SDAG emits `buffer_load_ubyte` / `buffer_store_byte` (format dropped); GISel correct. |
| ROCm 7.1.1 | Same defect. |

## Family

* c011 (TFE chain-drop), c014 (tbuffer illegal-vector ICE) --
  sibling code path defects in the same intrinsic family.
