target triple = "x86_64-unknown-linux-gnu"

; For i256, +Inf should saturate to all-ones (UINT_MAX).
; +Inf has biased exp = 255 (max for f32), so threshold 383 is never reached.
; Bug: +Inf treated as a normal large value.
define i256 @ui_sat_f(float %x) {
  %r = call i256 @llvm.fptoui.sat.i256.f32(float %x)
  ret i256 %r
}

declare i256 @llvm.fptoui.sat.i256.f32(float)
