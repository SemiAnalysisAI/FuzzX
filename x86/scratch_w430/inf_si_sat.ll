target triple = "x86_64-unknown-linux-gnu"

define i256 @si_sat_inf() {
  %r = call i256 @llvm.fptosi.sat.i256.f32(float 0x7FF0000000000000)
  ret i256 %r
}

define i256 @si_sat_neg_inf() {
  %r = call i256 @llvm.fptosi.sat.i256.f32(float 0xFFF0000000000000)
  ret i256 %r
}

declare i256 @llvm.fptosi.sat.i256.f32(float)
