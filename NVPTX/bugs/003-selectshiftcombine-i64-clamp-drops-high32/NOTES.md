# 003 — PerformSELECTShiftCombine folds guarded i64 shift to clamp shift, dropping the high 32 bits of the guard's shift amount

- **Kind:** miscompile
- **Reachable via:** default llc (after legalize)
- **Component:** NVPTXISelLowering.cpp 6732-6770  (region `L03b-combines-setcc-extract`)
- **Candidate id:** c006

## Summary

guarded i64 shift folded to PTX clamp shift using only the low 32 bits of the shift amount

## Mechanism / root cause

PerformSELECTShiftCombine rewrites `select (icmp ult shift_amt, BW), (x << shift_amt), 0` (and the ugt variant) into `NVPTXISD::SHL_CLAMP/SRL_CLAMP(x, ShiftOp.getOperand(1))`, relying on PTX shl/shr clamping amounts >= BitWidth to 0. The bug: PTX shift instructions take a 32-bit `.u32` shift-amount operand. For an i64 shift, ShiftOp.getOperand(1) is the shift amount TRUNCATED to i32 (the matcher even uses m_TruncOrSelf(m_Deferred(ShiftAmt)) at lines 6745-6746 to peek through that truncate). The guard `icmp ult shift_amt, 64` however compares the FULL i64 value. The combine emits the clamp using only the truncated low-32-bit amount (lines 6766-6769: `ShiftOp.getOperand(1)`), so the high 32 bits that the guard tested are silently discarded.

Concretely, with shift_amt = 0x0000000100000005 (i64): the guard `icmp ult i64 0x100000005, 64` is FALSE, so the IR `select` returns its `0` operand -> result 0 (the poison shl is in the unselected arm, so the select is well-defined). The generated PTX does `ld.param.b32 %r1` (low 32 bits = 5), then `shl.b64 %rd2, %rd1, %r1`, which clamps 5 < 64 and shifts: x << 5. For x=1 the IR yields 0 but the PTX yields 32.

Verified with the built llc: `guarded_shl_i64` and `guarded_srl_i64_ugt` both emit an unguarded `shl.b64/shr.u64` with the shift amount loaded as `.b32`, dropping the high half. The existing test test_guarded_i64_ult_shl in shift-opt.ll exhibits the same unguarded codegen, confirming the combine fires for i64. The combine is reached for ISD::SELECT (line 7141) with only an isAfterLegalizeDAG gate (no OptLevel gate), so it fires even at -O0. The i32/i64-with-i32-amount cases are safe; only shift-amount types wider than 32 bits (i64, i128) where the low 32 bits are < BitWidth but the high bits are nonzero are miscompiled. The fix needs to ensure the guard's compared value and the clamp's 32-bit amount agree on the dropped high bits (e.g. require ShiftAmt to be <= 32 bits, or that its high bits are known zero).

## Trigger

nvptx64, any sm/ptx, integer type with shift-amount wider than 32 bits (i64 is the common case). Pattern `shift >= 64 ? 0 : x >> shift` or `shift < 64 ? x << shift : 0` with an i64 shift amount whose high 32 bits are nonzero but low 32 bits are < 64. Fires at all opt levels (only gated on isAfterLegalizeDAG).

## Reproducer

See `repro.ll` / `cmd.sh`.

```
target triple = "nvptx64-nvidia-cuda"

; IR semantics: when %shift >= 64 the icmp is false, so select returns 0
; (the poison shl is in the unselected arm). For %shift=0x100000005 -> result 0.
define i64 @guarded_shl_i64(i64 %x, i64 %shift) {
  %cmp = icmp ult i64 %shift, 64
  %shl = shl i64 %x, %shift
  %sel = select i1 %cmp, i64 %shl, i64 0
  ret i64 %sel
}

; IR semantics: when %shift > 63 the icmp is true, so select returns 0.
define i64 @guarded_srl_i64_ugt(i64 %x, i64 %shift) {
  %cmp = icmp ugt i64 %shift, 63
  %shr = lshr i64 %x, %shift
  %sel = select i1 %cmp, i64 0, i64 %shr
  ret i64 %sel
}
```

Command:

```
llc -mtriple=nvptx64 -mcpu=sm_90 -O2 -o - repro.ll
```

## Observed (wrong) output

```
// guarded_shl_i64:
	ld.param.b64 	%rd1, [guarded_shl_i64_param_0];
	ld.param.b32 	%r1, [guarded_shl_i64_param_1];   // loads only low 32 bits of the i64 shift amount
	shl.b64 	%rd2, %rd1, %r1;                       // unguarded shift; select/guard gone
	st.param.b64 	[func_retval0], %rd2;
	ret;

// guarded_srl_i64_ugt:
	ld.param.b64 	%rd1, [guarded_srl_i64_ugt_param_0];
	ld.param.b32 	%r1, [guarded_srl_i64_ugt_param_1]; // low 32 bits only
	shr.u64 	%rd2, %rd1, %r1;                        // unguarded shift
	st.param.b64 	[func_retval0], %rd2;
	ret;

// ISel debug confirms: t18: i64 = NVPTXISD::SHL_CLAMP t4, t20, where
// t20: i32,ch = load<(... load (s32), align 8 ...)> of guarded_shl_i64_param_1
// i.e. the clamp amount is the low 32 bits of the i64 guard value.

// For x=1, shift=0x0000000100000005: PTX yields 1 << 5 = 32.
```

## Expected

For x=1, shift=0x0000000100000005 the IR select returns 0 (icmp ult 4294967301, 64 is false), so a correct compiler must produce 0, not 32. The combine must only fire when the guard's compared value and the 32-bit clamp amount agree on the dropped high bits — e.g. require the shift-amount type to be <= 32 bits, or that the high bits above bit 31 are known zero (knownBits), before rewriting to SHL_CLAMP/SRL_CLAMP. Equivalent correct PTX would keep the guard, e.g. compare the full i64 amount against 64 (setp.ge.u64 over the 64-bit amount) and selp 0, or otherwise not drop the high bits. For the i32-amount case the combine is already correct.

## Verification

Verified empirically with the built llc (independent verify + adversarial
refute both `confirmed`, finder confidence 0.85, verify confidence 0.97).

> Confirmed real miscompile. PerformSELECTShiftCombine (NVPTXISelLowering.cpp:6732-6770) rewrites `select (icmp ult shift_amt, BW), (x<<shift_amt), 0` (and ugt variant) into NVPTXISD::SHL_CLAMP/SRL_CLAMP(x, ShiftOp.getOperand(1)). The guard's icmp compares the FULL i64 shift amount (ShiftAmt bound in m_SetCC), but the clamp's amount operand is the shift's operand, which for an i64 shift is the amount truncated to i32 (the matcher peeks through that trunc via m_TruncOrSelf(m_Deferred(ShiftAmt)) at 6745-6746). The SHIFT multiclass (NVPTXInstrInfo.td:1393-1408) takes the amount as B32:$b ("shift is always int32"), so PTX shl.b64/shr.u64 only see the low 32 bits.

Verified codegen with the built llc: both guarded_shl_i64 and guarded_srl_i64_ugt emit an unguarded shl.b64/shr.u64 with the amount loaded via ld.param.b32 (low 32 bits of the i64 param), and the select/guard is entirely gone. ISel debug shows SHL_CLAMP64_rr / SRL_CLAMP64_rr consuming a 32-bit load (s32) of the i64 shift-amount parameter, dropping the high 32 bits.

Concrete defined-input discrepancy for guarded_shl_i64 with x=1, shift=0x0000000100000005 (=4294967301): IR `icmp ult 4294967301, 64` is FALSE, so select returns its 0 operand (the poison shl is in the UNSELECTED arm, so the select is well-defined) => IR result 0. PTX loads low 32 bits = 5, computes shl.b64 1,5 = 32. 0 != 32. Adversarial checks: (a) the poison shl is harmless because select picks the other arm; (b) the i32-amount variant is safe because guard 
