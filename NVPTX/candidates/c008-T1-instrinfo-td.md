# c008 — fptosi/fptoui to i1 lowered as (a == 0.0) instead of LSB(trunc(a)), miscompiling all float->i1 conversions

- region: T1-instrinfo-td
- file: NVPTXInstrInfo.td 2109,2115,2121,2127,2137,2148,2154,2160
- kind: miscompile
- confidence(finder): 0.85

## Mechanism
The patterns for converting a floating-point value directly to i1 use an integer setp-equal-to-zero on the raw float bits:

  def : Pat<(i1 (fp_to_sint f16:$a)), (SETP_i16ri $a, 0, CmpEQ)>;
  def : Pat<(i1 (fp_to_uint f16:$a)), (SETP_i16ri $a, 0, CmpEQ)>;
  def : Pat<(i1 (fp_to_sint f32:$a)), (SETP_i32ri $a, 0, CmpEQ)>;
  def : Pat<(i1 (fp_to_uint f32:$a)), (SETP_i32ri $a, 0, CmpEQ)>;
  def : Pat<(i1 (fp_to_sint f64:$a)), (SETP_i64ri $a, 0, CmpEQ)>;
  ... (also bf16 and fp_to_uint f64)

SETP_iNN with CmpEQ (CmpMode 0 -> PTXCmpMode::EQ) prints as 'setp.eq.bNN $a, 0', an INTEGER/bitwise comparison of the float's bit pattern against 0. So the emitted i1 result is (a_bits == 0), i.e. (a == +0.0).

LLVM semantics of fptoui/fptosi to i1 require the result to be the low bit of the trunc-toward-zero integer value of a (with poison only when that integer does not fit in i1, i.e. trunc(a) outside {0,1} for unsigned or {0,-1} for signed). For example fptoui(1.5) to i1 = trunc(1.5)=1 -> i1 = 1 (a defined input). The pattern instead yields (1.5 == 0) = 0. And fptoui(0.0) to i1 = 0, but the pattern yields (0.0 == 0) = 1. Both are wrong.

Verified with llc: for 'fptoui float %a to i1' the backend emits 'setp.eq.b32 %p1, %r1, 0; selp.b32 %r2, -1/1, 0, %p1', i.e. result = (a==0). x86 reference for the identical IR emits cvttss2si and takes the low bit (AL), confirming the correct value is LSB(trunc(a)), not (a==0). The same defect was confirmed for f16 (setp.eq.b16), f32 (setp.eq.b32), and f64 (setp.eq.b64). No test in test/CodeGen/NVPTX covers float->i1, which is why this has gone unnoticed.

## Trigger
Any nvptx64 target; an fptoui or fptosi from f16/bf16/f32/f64 directly to i1 with a runtime float operand. Concrete defined inputs that differ: a=1.5 (correct 1, emitted 0) and a=0.0 (correct 0, emitted 1).

## IR
```
target triple = "nvptx64-nvidia-cuda"

define i32 @f2u_i1_zext(float %a) {
  %r = fptoui float %a to i1
  %z = zext i1 %r to i32
  ret i32 %z
}

; For %a = 1.5 the correct result is 1 (trunc(1.5)=1, low bit 1).
; llc emits: setp.eq.b32 %p1, %r1, 0; selp.b32 %r2, 1, 0, %p1  => returns 0.

define i32 @f2s_i1_zext_half(half %a) {
  %r = fptosi half %a to i1
  %z = zext i1 %r to i32
  ret i32 %z
}
```

## llc cmd
`-mtriple=nvptx64 -mcpu=sm_90 -O2`
