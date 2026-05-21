# w430: ExpandIRInsts `expandFPToI` half-input fast path mishandles `fptoui.sat`/`fptosi.sat` intrinsics

## Where

`llvm/lib/CodeGen/ExpandIRInsts.cpp:594-617` — the `BitWidth >= 32 && isHalfTy()` shortcut in `expandFPToI` dispatches on `FPToI->getOpcode()`, but is called from `Intrinsic::Call` for `llvm.fptoui.sat` / `llvm.fptosi.sat` (lines `1255-1259, 1285-1291`). The opcode of an `IntrinsicInst` is `Instruction::Call`, never `FPToUI` / `FPToSI`, so the `if (FPToI->getOpcode() == FPToUI)` branch at line 607 is never taken for the saturating intrinsics. Both saturating intrinsics fall into the `else` arm and are lowered with `Builder.CreateFPToSI(FloatVal, i32)` followed by `sext`. This silently:

- turns `fptoui.sat.iN.f16` (N >= 32) into a *signed*, non-saturating fptosi + sign-extend, losing both the sign/zero-extension choice and the saturation semantics;
- turns `fptosi.sat.iN.f16` (N >= 32) into a *non-saturating* fptosi, so NaN → INT_MIN sign-extended (which is wrong: LangRef requires NaN -> 0) and out-of-range f16 values just propagate the unsaturated `cvttss2si` result.

The expansion is reached because `MaxLegalFpConvertBitWidth = 128` on x86-64 (`X86ISelLowering.cpp:177`), so anything >128 bits is sent through `expandFPToI` regardless of the input float type.

## Repro

`.ll` (`/home/orenamd@semianalysis.com/FuzzX/x86/scratch_w430/sat_half_i256.ll`):

```llvm
target triple = "x86_64-unknown-linux-gnu"
define i256 @ui_sat(half %x) {
  %r = call i256 @llvm.fptoui.sat.i256.f16(half %x)
  ret i256 %r
}
define i256 @si_sat(half %x) {
  %r = call i256 @llvm.fptosi.sat.i256.f16(half %x)
  ret i256 %r
}
declare i256 @llvm.fptoui.sat.i256.f16(half)
declare i256 @llvm.fptosi.sat.i256.f16(half)
```

`llc -O2 -mtriple=x86_64-unknown-linux-gnu` after `expand-ir-insts` produces **identical** IR for both:

```
%1 = fptosi half %x to i32
%2 = sext i32 %1 to i256
ret i256 %2
```

(Verified with `-stop-after=expand-ir-insts`.) Down to assembly, both `ui_sat` and `si_sat` emit the same `__extendhfsf2; cvttss2si; cltq; sar 63; ...` sequence.

## Why it's wrong

- LangRef for `fptoui.sat`: "If the input is NaN, returns 0. If the input is too small, returns 0. If too large, returns UINT_MAX." Here a half `-1.0` would lower to `cvttss2si` returning `0xFFFFFFFFFFFFFFFF` (after `cltq`), sign-extended to `i256 -1`, instead of the required `0`. NaN similarly returns the all-ones sign extension of `0x80000000`.
- LangRef for `fptosi.sat`: "If the input is NaN, returns 0." Here NaN lowers to `cvttss2si`'s NaN sentinel (`0x80000000`), sign-extended to `i256` SIGNED_MIN-of-i32 (negative but nowhere near `i256` SIGNED_MIN/0), instead of `0`.

For comparison, plain `fptoui half %x to i256` (non-intrinsic) is handled correctly because the opcode check matches and emits `zext (fptoui half to i32)`.

## Fix sketch

The half-input fast path must branch on the *signedness/saturation* derived from the original op kind (`FPToI` opcode *or* `Intrinsic::ID` for the intrinsic form) and `IsSigned` / `IsSaturating`. The cleanest fix is to detect the intrinsic form first and emit `fptoui.sat.i32.f16` / `fptosi.sat.i32.f16` + `zext`/`sext` (those i32 intrinsics are legal on x86), or simply not take the fast path for the saturating intrinsics.

## Candidate-level confidence

High. IR diff is unambiguous (`fptoui.sat` -> non-saturating `fptosi`), and the assembly confirms zero saturation logic is emitted. No optimization on the consumer side could recover the dropped saturation/sign choice.
