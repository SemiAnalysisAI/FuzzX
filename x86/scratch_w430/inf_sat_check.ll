target triple = "x86_64-unknown-linux-gnu"

@inf = constant float 0x7FF0000000000000  ; +Inf as f32

define i256 @ui_sat_inf() {
  %r = call i256 @llvm.fptoui.sat.i256.f32(float 0x7FF0000000000000)
  ret i256 %r
}

declare i256 @llvm.fptoui.sat.i256.f32(float)
