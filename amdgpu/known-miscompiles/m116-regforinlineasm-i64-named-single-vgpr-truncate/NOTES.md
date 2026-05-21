# m116: `getRegForInlineAsmConstraint` silently accepts `={v0}` for `i64`, synthesising the upper half from thin air

*Discovery method: code inspection.*

## The bug

`amdgpu/third_party/llvm-project/llvm/lib/Target/AMDGPU/SIISelLowering.cpp:19164-19211`
(`SITargetLowering::getRegForInlineAsmConstraint`):

For named-physreg constraints like `{v0}`, `{s4}`, `{a0}` the parser
returns `NumRegs == 1` (`BaseInfo.cpp:1645-1650`).  The multi-DWORD
width enforcement at lines 19175-19183:

```cpp
if (NumRegs > 1) {
  unsigned Width = NumRegs * 32;
  if (Width != VT.getSizeInBits())
    return std::make_pair(0U, nullptr);   // reject
  ...
}
```

is ONLY entered when `NumRegs > 1`.  The `NumRegs == 1` fallthrough at
lines 19204-19208:

```cpp
if (VT.isVector() && VT.getSizeInBits() != 32)
  return std::make_pair(0U, nullptr);
return std::make_pair(Reg, RC);
```

only checks `VT.isVector()` -- it NEVER compares the scalar VT
bit-width to the 32-bit register class width.

Result: `i64 = ={v0}` is silently accepted and bound to a single VGPR.
The codegen then synthesises the i64's upper half from thin air
(`v_mov_b32_e32 v1, 0`).

The range form `{v[0:0]}` (semantically equivalent -- one register
starting at index 0) is correctly rejected with "could not allocate
output register" because the `[N:N]` parser branch returns `{}` when
`End - Idx + 1 == 1` which fails the `NumRegs > 1` test, falling
through to the generic handler which doesn't know the name.  The
off-by-one is that the parser treats `{vN}` and `{v[N:N]}`
asymmetrically.

## Reproducer

`reduced.ll`:

```llvm
define i64 @bad_named_v0_i64() {
  %r = call i64 asm sideeffect "v_mov_b32 $0, 42", "={v0}"()
  ret i64 %r
}
```

`llc -mtriple=amdgcn -mcpu=gfx950 -O2`:

```asm
bad_named_v0_i64:
        v_mov_b32_e32 v1, 0          ; <-- materialised garbage upper-half
        ;APP
        v_mov_b32 v0, 42             ; the actual asm
        ;NO_APP
        s_setpc_b64 s[30:31]         ; returns {v0=42, v1=0} as i64
```

Same defect with `={s4}` (asm writes s4, codegen fills upper half with
`v1=0`).  Contrast: `={v[0:1]}` correctly enforces width and emits
`v_mov_b64`.

Caller sees `i64 0x0000000000000002A` instead of whatever the asm
actually computed.  In a real client the asm body might be deliberately
writing both halves of `v0:v1` and expecting the constraint to reserve
both -- the silent truncation makes a 64-bit-write asm read back as a
32-bit value, with the upper bits clobbered by the synthesised
`v_mov_b32_e32 v1, 0` that precedes the asm.

## Suggested fix

In the named-physreg block, before returning at line 19207-19208,
check the scalar VT bit-width:

```cpp
if (VT != MVT::Other && VT.getSizeInBits() != RC->getSizeInBits())
  return std::make_pair(0U, nullptr);
```

Or unify with the multi-DWORD path: always run the 19175-19201 width
check with `NumRegs >= 1`.

## Toolchain Results

| Toolchain | Result |
| --- | --- |
| LLVM HEAD with the local PR patches (`build/llvm-fuzzer`) | Reproduces. |
| ROCm 7.1.1 (`/opt/rocm-7.1.1/lib/llvm/bin/llc`) | Same defect. |

## Related: `'a'` constraint silent fallthrough

`SIISelLowering.cpp:19127-19129`:

```cpp
case 'a':
  if (!Subtarget->hasMAIInsts()) break;
  ...
```

`break` leaves `RC == nullptr`; falls through to named-physreg
handler, which won't match the bare `"a"` constraint, then generic
handler emits a confusing "constraint not supported" diagnostic.
Quality-of-diagnostic, not miscompile.

## Why the fuzzer hasn't caught it

* The IR fuzzer rarely emits inline asm.
* The interpreter oracle skips kernels with `asm sideeffect`.
* Per `MEMORY.md` (Prefer-random-over-idioms), the right hook would
  be to add named-physreg inline-asm shapes to the random emitter
  with varied result types (i64, v2i32, v4i32) to surface this and
  sibling width-mismatch issues.
