# 001 — fptosi/fptoui to i1 lowered as (a == 0.0) instead of LSB(trunc(a)), miscompiling all float->i1 conversions

- **Kind:** miscompile
- **Reachable via:** default llc -O0/-O2
- **Component:** NVPTXInstrInfo.td 2109,2115,2121,2127,2137,2148,2154,2160  (region `T1-instrinfo-td`)
- **Candidate id:** c008

## Summary

`fptoui`/`fptosi` of any float to `i1` is lowered as `(a == 0.0)` instead of `trunc(a)&1`

## Mechanism / root cause

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

## Reproducer

See `repro.ll` / `cmd.sh`.

```
target triple = "nvptx64-nvidia-cuda"

define i32 @f2u_i1_zext(float %a) {
  %r = fptoui float %a to i1
  %z = zext i1 %r to i32
  ret i32 %z
}

define i32 @f2s_i1_zext(float %a) {
  %r = fptosi float %a to i1
  %z = zext i1 %r to i32
  ret i32 %z
}
```

Command:

```
llc -mtriple=nvptx64 -mcpu=sm_90 -O2 -o - repro.ll
```

## Observed (wrong) output

```
.visible .func  (.param .b32 func_retval0) f2u_i1_zext( .param .b32 f2u_i1_zext_param_0 )
{
	.reg .pred 	%p<2>;
	.reg .b32 	%r<3>;
	ld.param.b32 	%r1, [f2u_i1_zext_param_0];
	setp.eq.b32 	%p1, %r1, 0;
	selp.b32 	%r2, 1, 0, %p1;
	st.param.b32 	[func_retval0], %r2;
	ret;
}

(f2s_i1_zext is identical: setp.eq.b32 %p1, %r1, 0; selp.b32 %r2, 1, 0, %p1)

So for a=1.5f the function returns selp(1.5_bits==0 ? 1 : 0) = 0 (correct is 1); for a=0.0f it returns 1 (correct is 0). The float bits are loaded with ld.param.b32 into an integer register and compared bitwise to 0 with no cvt.
```

## Expected

Result should be LSB(trunc_toward_zero(a)). Reference x86_64 codegen for the same IR:
  f2u_i1_zext: cvttss2si %xmm0, %eax ; movzbl %al, %eax ; ret      (=> a=1.5 -> 1, a=0.0 -> 0)
  f2s_i1_zext: cvttss2si %xmm0, %eax ; movzbl %al, %eax ; andl $1, %eax ; ret
Correct NVPTX would convert with round-toward-zero to an integer (e.g. cvt.rzi.s32.f32) and take/AND the low bit (analogous to the existing trunc-to-i1 pattern at line 2209: SETP_i32ri (AND_b32ri $a, 1), 0, CmpNE), NOT compare the float's bit pattern to 0. The current code computes (a == +0.0).

## Verification

Verified empirically with the built llc (independent verify + adversarial
refute both `confirmed`, finder confidence 0.85, verify confidence 0.98).

> Confirmed real miscompile. NVPTXInstrInfo.td lines 2109/2115/2121/2127/2137/2148/2154/2160 match (i1 (fp_to_sint/fp_to_uint fXX:$a)) to (SETP_iNNri $a, 0, CmpEQ). The ISETP multiclass (lines 1634-1657) prints "setp.${cmp:ICmp}.${cmp:IType}NN", so CmpEQ -> "setp.eq.bNN $a, 0", a BITWISE integer comparison of the float register's raw bits against 0 — the float operand is consumed directly with no cvt. The emitted i1 is therefore (bitcast(a) == 0) == (a == +0.0).

LangRef requires fptoui/fptosi to i1 = LSB of trunc-toward-zero(a), with poison only when that integer doesn't fit i1 (outside {0,1} unsigned, {0,-1} signed). I exhibited two fully well-defined inputs (no poison/UB) where NVPTX is wrong:
 - a=0.0f: trunc=0 fits -> correct result 0; NVPTX computes (0x00000000==0)=1. WRONG.
 - a=1.5f: trunc=1 fits in unsigned i1 -> correct result 1; NVPTX computes (0x3FC00000==0)=0. WRONG.
Both verified numerically (bit patterns 0x00000000 and 0x3FC00000) and against an x86_64 reference for the identical IR, which emits cvttss2si + low-bit extract (movzbl / andl $1) — LSB(trunc_toward_zero(a)) — and disagrees with NVPTX on both inputs. Note -0.0 (0x80000000) happens to coincidentally agree (NVPTX=0, correct=0) because its bits are nonzero. The defect spans f16/bf16/f32/f64, signed and unsigned. No NVPTX test covers float->i1 (only i16/i32/i64), which is why it went unnoticed. I considered and ruled out: the inputs are not UB/poison; nothing masks/normalizes the result (it is the direct r

## Independent cross-check

`opt -O2` constant-folds the truth:
- `fptoui float 1.5 to i1` → **1**  (trunc(1.5)=1, low bit 1)
- `fptoui float 0.0 to i1` → **0**

NVPTX emits `setp.eq.b32 %p, a, 0` ⇒ result `(a == 0.0)`:
- a=1.5 → 0 (should be 1)  ✗
- a=0.0 → 1 (should be 0)  ✗

x86 reference codegen for the same IR is `cvttss2si %xmm0, %eax` (convert toward
zero, take the low bit) — i.e. `LSB(trunc(a))`, confirming the correct lowering
is the truncated-integer low bit, not an equality-with-zero test.
