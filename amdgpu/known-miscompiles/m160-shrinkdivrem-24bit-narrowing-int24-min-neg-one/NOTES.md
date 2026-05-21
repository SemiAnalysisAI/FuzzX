# m160: `shrinkDivRem64` 24-bit narrowing of `INT24_MIN / -1` produces wrong sign

*Discovery method: code inspection (during i64 sdiv lowering audit;
direct sibling of m103/m132 at the 24-bit boundary).*

## The bug

`amdgpu/third_party/llvm-project/llvm/lib/Target/AMDGPU/AMDGPUCodeGenPrepare.cpp:1354-1361`
(`shrinkDivRem64`) and the sign-extend at
`AMDGPUCodeGenPrepare.cpp:1155-1162` (`expandDivRem24Impl`).

For i64 sdiv whose LHS has bit pattern `0xFFFFFFFFFF800000` (i.e.
`sext i24 -2^23` or `sext i32 -8388608` clipped) and `RHS = -1`:

* `ComputeNumSignBits(LHS) = 41`, RHS = 64.
* `getDivNumBits` returns `64 - 41 + 1 = 24`.
* `expandDivRem24Impl` fires.

Inside `expandDivRem24Impl`:

1. Operands truncated to i32: `(-2^23, -1)`.
2. FP reciprocal computes the true quotient
   `+2^23 = 0x00800000` (mathematically correct).
3. Sign-extends from 24-bit via `SHL 8 ; AShr 8`:
   * `0x00800000 << 8 = 0x80000000`
   * `AShr 8 = 0xFF800000 = -2^23` (sign extension **corrupts**)
4. Final sext to i64 yields `0xFFFFFFFFFF800000` instead of the true
   `+8388608 = 0x0000000000800000`.

This is the structural sibling of m103/m132 one boundary lower: the
narrowing gate `DivBits <= 24` admits operands whose true signed
quotient (`2^23`) is exactly the value not representable as signed
24-bit, and the in-register sign-extend then negates it.

The same gate is reachable from **i32 sdiv** where LHS has ≥9 sign
bits (`|abs| ≤ 2^23`), RHS = -1, since `getDivNumBits` for i32
returns `32 - 9 + 1 = 24`.  So this also fires for plain i32 sdiv
of `(-2^23) / (-1)` -- yielding `-2^23` instead of `+2^23`.

## Reproducer

`reduced.ll`:

```llvm
define amdgpu_kernel void @t_i32(ptr addrspace(1) %p) {
  %a = sub i32 0, 8388608     ; -2^23
  %b = sub i32 0, 1           ; -1
  %r = sdiv i32 %a, %b        ; expected +2^23 = 0x00800000
  store i32 %r, ptr addrspace(1) %p
  ret void
}
```

Both `t_i32` and `t_i64` reproduce.

`llc -mtriple=amdgcn -mcpu=gfx950 -O2 reduced.ll`: expandDivRem24Impl
fires; emitted code stores `0xFF800000` (i32) / `0xFFFFFFFFFF800000`
(i64) instead of the correct `0x00800000` / `0x0000000000800000`.

## Suggested fix

Tighten `getDivNumBits` so that the returned `DivBits` actually
bounds `|quotient|`, not just `|operand|`.  The simplest fix in
`AMDGPUCodeGenPrepare.cpp:1032` (and the unsigned arm at line 1049)
is to require `DivBits ≤ MaxDivBits - 1` when `RHS` may equal `-1`
(signed) or when one operand may equal `-2^(DivBits-1)`.

Alternatively, in `expandDivRem24Impl` and `shrinkDivRem64`, reject
the input when `LHS == sext(INT_MIN_at_DivBits) && RHS == -1`.

## Why the fuzzer hasn't caught it

* O0 vs O2 differential collapses because both pipelines run
  `AMDGPUCodeGenPrepare`; the interpreter oracle is needed.
* Per `MEMORY.md` (prefer-random-over-idioms), the right hook is
  enriching the i32 constant pool with `INT24_MIN = 0xFF800000`
  (and the i64 sext thereof) and letting the random emitter feed
  them as the LHS of a sdiv whose RHS is `-1`.

## Toolchain Results

| Toolchain | Result |
| --- | --- |
| LLVM HEAD with the local PR patches (`build/llvm-fuzzer`) | Buggy 24-bit sign-extend in expandDivRem24Impl. |
| ROCm 7.1.1 | Same defect. |

## Family

* m040 (24-bit unsigned-positive 24-bit-set boundary).
* m103 (i64->32-bit narrowing INT32_MIN/-1).
* m132 (vector composition of m103).
* m160 (this entry) -- i64->24-bit and i32->24-bit INT24_MIN/-1.

The full family covers the boundary `INT_MIN_at_N / -1` for
N ∈ {24, 32}.  Worth checking N=16 / N=8 if smaller narrowing
gates are added later.
