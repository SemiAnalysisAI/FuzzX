# m125: `AMDGPUPrintfRuntimeBinding` uses `sext` (instead of `zext`) to widen scalar `half`/`bfloat` args, corrupting bits for negative values

*Discovery method: code inspection.*  Companion to m112 (`%s` slot
off-by-4 in same pass).

## The bug

`amdgpu/third_party/llvm-project/llvm/lib/Target/AMDGPU/AMDGPUPrintfRuntimeBinding.cpp:191-203`:

```cpp
if (ArgType->isFloatingPointTy()) {
  // ... bitcast half/bfloat to i16 ...
  Arg = IRB.CreateBitCast(Arg, IRB.getIntNTy(...));
}
...
// Widen to i32:
Arg = IRB.CreateSExt(Arg, IRB.getInt32Ty(), "");   // <-- BUG: should be zext
```

For scalar `half` / `bfloat` printf args (`%f` -> 16-bit FP type, total
alloc 2 bytes < 4), the pass widens to a 32-bit slot via
`bitcast (half) -> i16 -> sext -> i32`.

`sext` is the wrong widener for FP-derived bits.  When the half's
sign bit is set (any negative value), `sext` replicates that bit
into the top 16 bits of the i32 slot, flipping them from `0x0000`
to `0xFFFF`.

Example: `half -2.0` has bit pattern `0xC000`.

| widener | i32 value | bytes in printf record |
| --- | --- | --- |
| `sext i16 0xC000 to i32`  | `0xFFFFC000` | runtime reads garbage f32 |
| `zext i16 0xC000 to i32`  | `0x0000C000` | preserves half bit pattern (correct lift) |
| `fpext half to float`      | `0xC0000000` | true f32 representation of `-2.0` |

The runtime `%f` formatter reads the i32 slot and interprets it as an
f32.  Neither `sext` nor `zext` is correct in absolute terms (an
`fpext` to f32 would be), but `sext` is unambiguously wrong: it
destroys the half's bit pattern AND fails to give a valid f32 of the
intended value.  `zext` would at least preserve the bit pattern that
the runtime could fpext-on-the-fly.

## Reproducer

`reduced.ll`:

```llvm
@.fmt = private unnamed_addr addrspace(4) constant [4 x i8] c"%f\0A\00"
declare i32 @printf(ptr addrspace(4), ...)

define amdgpu_kernel void @k(half %h) {
  call i32 (ptr addrspace(4), ...) @printf(ptr addrspace(4) @.fmt, half %h)
  ret void
}
```

`opt -mtriple=amdgcn-amd-amdhsa -mcpu=gfx950
-passes=amdgpu-printf-runtime-binding -S reduced.ll`:

```llvm
%1 = bitcast half %h to i16
%2 = sext i16 %1 to i32              ; <-- buggy widener
store i32 %2, ptr addrspace(1) ..., align 4    ; arg slot
```

For input `h = -2.0` (`0xC000`):
* Slot bits: `0xFFFFC000` (sext-corrupted).
* Runtime `%f` reads f32 = `-169086976.0` (wildly wrong).

For `h = +2.0` (`0x4000`): slot bits `0x00004000` (zero high half --
no corruption, but still not a valid f32 representation of `+2.0`).

The sibling bf16 case has the same defect.

## Companion compile crash on FP-vector `<1 x half>`

The agent also flagged a compile-time crash for `<1 x half>` / `<1 x
bfloat>` printf args (the `isFloatingPointTy()` guard at line 191
checks scalar only, so FP-vector with sub-DWORD alloc skips the
bitcast and ends up at `sext <1 x half> to <1 x i32>`, which the IR
verifier rejects).  That is a separate ICE -- noted here for context
but not the focus of this bug.

## Suggested fix

Replace `IRB.CreateSExt` at line 201/203 with `IRB.CreateZExt` when
the source was an FP type.  Even better, when promoting FP args for
`%f`, emit an `fpext` to f32 (or directly to f64 for vararg promotion)
so the runtime sees a proper f32/f64 bit pattern.

## Toolchain Results

| Toolchain | Result |
| --- | --- |
| LLVM HEAD with the local PR patches (`build/llvm-fuzzer`) | Reproduces. |
| ROCm 7.2.3 (`/opt/rocm-7.2.3/lib/llvm/bin/opt`) | Same defect. |
| ROCm 7.1.1 (`/opt/rocm-7.1.1/lib/llvm/bin/opt`) | Same defect. |

## Why the fuzzer hasn't caught it

* FuzzX IR emitter doesn't produce `printf` declarations.
* The interpreter oracle would need a printf-aware runtime check
  (compare stdout against expected text).
* Per `MEMORY.md` (Prefer-random-over-idioms), the right hook is to
  add `printf("%f", half_val)` / `printf("%f", bfloat_val)` shapes
  with negative half operands to the random IR emitter.
