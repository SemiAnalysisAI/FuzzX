# 020 — PerformSELECTShiftCombine drops overflow guard on non-i64 shift when the guard compare is on a wider amount (miscompile)

- **Kind:** miscompile
- **Reachable via:** default llc
- **Component:** NVPTXISelLowering.cpp 6732-6769 (matchers 6743-6760, rewrite 6766-6769)  (round-3 area `T12-shift-rotate`)
- **Candidate id:** r3_05

## Summary

PerformSELECTShiftCombine matches a guard comparing a wider (i64) amount against a narrower (i32) shift, dropping the guard

## Mechanism / root cause

PerformSELECTShiftCombine recognizes a guarded shift `(select (ugt amt, BW-1) 0, (shift x, amt))` (and the ULT/SHL forms) and rewrites it to NVPTXISD::SRL_CLAMP/SHL_CLAMP, relying on PTX shr/shl clamping shift amounts >= BitWidth to 0. The shift amount used in the rewrite is `ShiftOp.getOperand(1)` (line 6768-6769), and the matchers permit a width mismatch between the guard's compared value and the shift's amount: (a) the shift amount is matched with `m_TruncOrSelf(m_Deferred(ShiftAmt))` (lines 6745-6746), so the actual shift amount may be a *truncation* of the (wider) ShiftAmt that the icmp tested; (b) the guard constant is matched with `m_SpecificInt(APInt(BitWidth, BitWidth-1))`, and m_SpecificInt uses APInt::isSameValue which compares values irrespective of bit width (SDPatternMatch.h:1335), so `icmp ugt i64 %s, 31` matches even though BitWidth (the SELECT/shift type) is 32. Result: for an i32 shift guarded by a 64-bit comparison, the combine deletes the guard entirely and emits a bare `shr.u32`/`shl.b32` that consumes only the low 32 bits of the 64-bit amount (the param is loaded with `ld.param.b32`). When the high 32 bits of the amount are nonzero but the low 32 bits are < the shift width, the guard would have produced 0 but the emitted code shifts by the (small) low bits instead. The fix should require the icmp's compared value and the shift's amount to be the same SDValue (no trunc), and the guard constant width to match the shift width. Distinct from the listed i64 bug #3 (which is about an i64 *destination* shift's 32-bit amount register); here the destination shift is i32/non-i64 and the defect is the cross-width matching that fires the combine at all, matching the assignment's hint about '32-bit-amount truncation for non-i64 wide types'.

## Trigger

i32 (or other non-i64) logical shift whose overflow guard compares a wider (i64) amount, e.g. C like `unsigned long n; uint32_t r = (n > 31) ? 0 : (x >> (uint32_t)n);`. Concretely with x = 0xFFFFFFFF and s = 0x100000000 (= 2^32): correct result is 0 (since 2^32 > 31), but emitted PTX computes `x >> low32(s) = x >> 0 = 0xFFFFFFFF`. Verified against x86_64 llc which keeps the full-width guard (`cmpq $32,%rsi; cmovbl`). Fires at all SM versions and at default -O2 (no OptLevel guard in the combine).

## Reproducer

```
define i32 @guarded_shift_wide(i32 %x, i64 %s) {
  %st = trunc i64 %s to i32
  %c = icmp ugt i64 %s, 31
  %sh = lshr i32 %x, %st
  %r = select i1 %c, i32 0, i32 %sh
  ret i32 %r
}

define i32 @guarded_shl_wide_ult(i32 %x, i64 %s) {
  %st = trunc i64 %s to i32
  %c = icmp ult i64 %s, 32
  %sh = shl i32 %x, %st
  %r = select i1 %c, i32 %sh, i32 0
  ret i32 %r
}
```

Command: `llc -mtriple=nvptx64 -mcpu=sm_70 -o - repro.ll`

## Observed (wrong) output

```
.visible .func  (.param .b32 func_retval0) guarded_shift_wide(
	.param .b32 guarded_shift_wide_param_0,
	.param .b64 guarded_shift_wide_param_1
)
{
	.reg .b32 	%r<4>;
// %bb.0:
	ld.param.b32 	%r1, [guarded_shift_wide_param_0];
	ld.param.b32 	%r2, [guarded_shift_wide_param_1];   // reads only LOW 32 bits of i64 %s
	shr.u32 	%r3, %r1, %r2;                          // no guard/compare; shifts by low32(%s)
	st.param.b32 	[func_retval0], %r3;
	ret;
}

.visible .func  (.param .b32 func_retval0) guarded_shl_wide_ult(
	.param .b32 guarded_shl_wide_ult_param_0,
	.param .b64 guarded_shl_wide_ult_param_1
)
{
	.reg .b32 	%r<4>;
// %bb.0:
	ld.param.b32 	%r1, [guarded_shl_wide_ult_param_0];
	ld.param.b32 	%r2, [guarded_shl_wide_ult_param_1];   // reads only LOW 32 bits of i64 %s
	shl.b32 	%r3, %r1, %r2;                          // no guard/compare; shifts by low32(%s)
	st.param.b32 	[func_retval0], %r3;
	ret;
}

For input %x=0xFFFFFFFF, %s=0x100000000 (2^32): ld.param.b32 loads low32(2^32)=0, so shr.u32 0xFFFFFFFF,0 = 0xFFFFFFFF.
```

## Expected

The guard compares the full i64 amount, so for %s=2^32 (which is > 31) guarded_shift_wide must return 0 and guarded_shl_wide_ult must return 0. Correct lowering keeps the full-width guard, e.g. x86_64 emits `cmpq $32, %rsi; cmovbl` (returns 0 for any %s>=32 including 2^32). The combine should only fire when the icmp's compared value and the shift's amount are the same SDValue with matching width (no trunc), and the guard constant width matches the shift width; otherwise it must leave the guard in place. For these wide-guard inputs the emitted result should be 0, not 0xFFFFFFFF / 1.

## Verification

Independent verify + adversarial refute, both `confirmed` (verify confidence 0.97).

> Confirmed real miscompile in PerformSELECTShiftCombine (NVPTXISelLowering.cpp:6732-6769).

Source mechanism verified: (1) m_SpecificInt(APInt(BitWidth, BitWidth-1)) compares via APInt::isSameValue (SDPatternMatch.h:1335; APInt.h:557-568), which zero-extends the narrower APInt, so the i32 guard constant APInt(32,31) matches an i64 icmp constant 31. (2) The shift amount is matched with m_TruncOrSelf(m_Deferred(ShiftAmt)) (line 6745-6746), so a `trunc i64 %s to i32` matches against the i64 %s captured from the SetCC LHS. (3) The rewrite at line 6768-6769 uses ShiftOp.getOperand(1) (the truncated low-32-bit amount) and discards the guard. So for an i32 shift whose guard compares the full i64 amount, the combine fires and emits a bare clamp-shift whose amount is only the low 32 bits of the i64 value.

Emitted PTX (nvptx64, sm_70) for the repro: both functions load the 8-byte param with `ld.param.b32` (low 32 bits only) and emit a bare `shr.u32`/`shl.b32` with NO guard/compare.

Concrete defined miscompiling input for guarded_shift_wide(%x=0xFFFFFFFF, %s=0x100000000=2^32): IR is fully well-defined (no UB/poison): trunc(2^32)->i32 = 0; icmp ugt i64 2^32, 31 = true; select returns 0. Correct result = 0. PTX: ld.param.b32 reads low32(2^32) = 0 (NVPTX is little-endian, datalayout `e-`), shr.u32 0xFFFFFFFF, 0 = 0xFFFFFFFF. Wrong (0xFFFFFFFF != 0). The guarded_shl case is analogous (e.g. %
