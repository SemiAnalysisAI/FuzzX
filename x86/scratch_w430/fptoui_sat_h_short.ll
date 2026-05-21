target triple = "x86_64-unknown-linux-gnu"

; Hint: fptoui.sat of a negative half should produce 0, NaN should produce 0.
; But the expansion turns this into sext(fptosi half->i32), so:
;   -1.0      -> sext(-1) = all-ones i256 (WRONG, expected 0)
;   NaN       -> sext(INT_MIN) (WRONG, expected 0)
;   65504.0   -> sext(65504) = 65504 (OK)
define i256 @ui_sat(half %x) {
  %r = call i256 @llvm.fptoui.sat.i256.f16(half %x)
  ret i256 %r
}

declare i256 @llvm.fptoui.sat.i256.f16(half)
