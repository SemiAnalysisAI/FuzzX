target triple = "x86_64-unknown-linux-gnu"

define i256 @ui_sat_f(float %x) {
  %r = call i256 @llvm.fptoui.sat.i256.f32(float %x)
  ret i256 %r
}

define i256 @si_sat_f(float %x) {
  %r = call i256 @llvm.fptosi.sat.i256.f32(float %x)
  ret i256 %r
}

declare i256 @llvm.fptoui.sat.i256.f32(float)
declare i256 @llvm.fptosi.sat.i256.f32(float)
