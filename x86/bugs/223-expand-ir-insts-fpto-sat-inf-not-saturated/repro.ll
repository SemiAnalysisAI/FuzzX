target triple = "x86_64-unknown-linux-gnu"
declare i256 @llvm.fptoui.sat.i256.f32(float)
define i256 @f() {
  %r = call i256 @llvm.fptoui.sat.i256.f32(float 0x7FF0000000000000)
  ret i256 %r
}
