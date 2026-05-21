# X86DomainReassignment: SHR/SHL replaced by KSHIFTR/KSHIFTL with different over-shift semantics

File: llvm/lib/Target/X86/X86DomainReassignment.cpp:653-654,680-684,705-706,712-713,741-742,757-758

## Reasoning

The pass blindly converts `X86::SHR32ri`, `X86::SHL32ri`, `X86::SHR64ri`, `X86::SHL64ri` (and 8/16-bit/NDD variants) to `X86::KSHIFTRD/Q/B/Wki` etc. via `createReplacer`. The semantics of these instructions DIFFER for shift counts >= register width:

- GPR `SHL64ri imm` masks the immediate to the low 6 bits (Intel SDM, vol 2, SHL/SHR/SAL/SAR: "the count is masked to 5 bits, or 6 bits if in 64-bit mode and REX.W is used"). So `SHL64 x, 64` is `x << 0` == `x`.
- AVX-512 `KSHIFTLQ k, imm8` uses the full imm8 — `imm8 >= 64` returns 0.

The `InstrReplacer` converter (line 144) just transfers explicit operands unchanged, so a `SHR32ri` with immediate 32 (legal as an unmasked imm operand in the MIR after some folding/combining) becomes `KSHIFTRDki 32` which yields 0 rather than the original value. The same applies to `SHR8ri`/`KSHIFTRBki` with `imm >= 8`, `SHR16ri`/`KSHIFTRWki` with `imm >= 16`. Crafted IR that shifts by a constant exactly equal to the operand width through an AND/OR/XOR mask chain (forming a closure that goes through the K domain) will silently flip results between GPR and K paths.

## Reproducer sketch (IR, AVX-512 BWI)

```llvm
target triple = "x86_64-unknown-linux-gnu"
; Build a chain that becomes a closure of GR32 ops:
;   t = (a & b) ^ ((a | b) >> 32)
; The reassigner currently sees AND32rr/OR32rr/XOR32rr/SHR32ri and forms a
; mask-domain closure replacing SHR32ri with KSHIFTRDki, which differs in the
; out-of-range shift count.
define i32 @f(i32 %a, i32 %b) #0 {
  %ab = and i32 %a, %b
  %ob = or  i32 %a, %b
  %s  = lshr i32 %ob, 32         ; in IR this is poison, but post-isel a folded constant of 32 reaches SHR32ri
  %x  = xor i32 %ab, %s
  ret i32 %x
}
attributes #0 = { "target-features"="+avx512bw,+avx512dq" }
```

The IR `lshr 32` is poison so the front-end may fold; the actual trigger needs a MIR test with an immediate that is = width (e.g. emit explicit `SHR32ri 32` via inline asm-free MIR test). MIR reproducer:

```
name: shift_overshift
body: |
  bb.0:
    %0:gr32 = COPY $edi
    %1:gr32 = COPY $esi
    %2:gr32 = AND32rr %0, %1, implicit-def dead $eflags
    %3:gr32 = OR32rr  %0, %1, implicit-def dead $eflags
    %4:gr32 = SHR32ri %3, 32, implicit-def dead $eflags
    %5:gr32 = XOR32rr %2, %4, implicit-def dead $eflags
    $eax = COPY %5
    RET 0, $eax
```

## Expected wrong outcome

After x86-domain-reassignment, the SHR32ri becomes KSHIFTRDki with the immediate 32 preserved, returning 0 instead of the original GPR semantic of returning `%3` unchanged (mask to 5 bits → shift 0). The final value differs: `(a&b) ^ 0` vs `(a&b) ^ (a|b)`. With `a=1,b=2`: GPR returns `0 ^ 3 = 3`; reassigned returns `0 ^ 0 = 0`.
