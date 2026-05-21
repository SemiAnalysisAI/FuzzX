target triple = "x86_64-unknown-linux-gnu"
define i32 @ref() {
  %r = call i32 @llvm.fptoui.sat.i32.f32(float 0x7FF0000000000000)
  ret i32 %r
}
declare i32 @llvm.fptoui.sat.i32.f32(float)
