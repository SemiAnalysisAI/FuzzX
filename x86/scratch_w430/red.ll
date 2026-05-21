target triple = "x86_64-unknown-linux-gnu"
define float @ord_fadd(float %acc, <4 x float> %v) {
  %r = call float @llvm.vector.reduce.fadd.v4f32(float %acc, <4 x float> %v)
  ret float %r
}
declare float @llvm.vector.reduce.fadd.v4f32(float, <4 x float>)
