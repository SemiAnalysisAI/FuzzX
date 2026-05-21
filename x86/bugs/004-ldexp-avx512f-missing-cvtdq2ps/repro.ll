declare <4 x float> @llvm.ldexp.v4f32.v4i32(<4 x float>, <4 x i32>)

define <4 x float> @ldexp_v4f32(<4 x float> %x, <4 x i32> %exp) {
  %r = call <4 x float> @llvm.ldexp.v4f32.v4i32(<4 x float> %x, <4 x i32> %exp)
  ret <4 x float> %r
}
