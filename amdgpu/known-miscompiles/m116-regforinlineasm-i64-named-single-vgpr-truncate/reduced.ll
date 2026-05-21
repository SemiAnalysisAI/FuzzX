; Reproduces silent width-mismatch in
; SITargetLowering::getRegForInlineAsmConstraint
; (SIISelLowering.cpp:19164-19211).
;
; For named-physreg single-DWORD constraints like `{v0}`, `{s4}`,
; `{a0}` the parser returns NumRegs == 1.  The multi-DWORD width
; enforcement (line 19175-19183) is ONLY entered when NumRegs > 1.
; The NumRegs == 1 fallthrough at 19204-19208 only checks
; `VT.isVector() && VT.getSizeInBits() != 32` -- it never compares
; scalar VT bit-width to the 32-bit register class width.
;
; Result: `={v0}` is silently accepted for an `i64` result type and
; bound to a single 32-bit VGPR.  The codegen then synthesises the
; i64's upper half from thin air (initialized to 0).
;
; The range form `{v[0:0]}` (semantically equivalent) is correctly
; rejected with "could not allocate output register".  The off-by-one
; is that `{vN}` succeeds without width-checking while `{v[N:N]}`
; fails.
;
; Run with:
;   llc -mtriple=amdgcn -mcpu=gfx950 -O2 reduced.ll

source_filename = "m116-regforinlineasm-i64-named-single-vgpr-truncate"
target triple = "amdgcn-amd-amdhsa"

define i64 @bad_named_v0_i64() {
  ; ={v0} binds to a 32-bit VGPR but the asm result type is i64.
  ; The codegen materializes v1=0 for the upper half "from thin air".
  %r = call i64 asm sideeffect "v_mov_b32 $0, 42", "={v0}"()
  ret i64 %r
}

; Expected behaviour (one of):
;   - Reject the constraint at IR validation with a width mismatch.
;   - Allocate v0:v1 as a tuple and require the asm to set both.
; Observed: silently allocate v0 only; emit `v_mov_b32_e32 v1, 0`
; before the asm, so the function returns {v0=42, v1=0} as the i64.
