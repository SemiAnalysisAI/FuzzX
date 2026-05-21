target triple = "x86_64-unknown-linux-gnu"

; Bug also applies to i129: +Inf has BiasedExp 255 < 127 + 129 = 256. Threshold not met.
; Result will be 0x800000 << (255 - 150) = 0x800000 << 105 = 2^128 < 2^129.
define i129 @ui_sat_inf() {
  %r = call i129 @llvm.fptoui.sat.i129.f32(float 0x7FF0000000000000)
  ret i129 %r
}

declare i129 @llvm.fptoui.sat.i129.f32(float)
