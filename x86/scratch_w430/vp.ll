target triple = "x86_64-unknown-linux-gnu"

define <4 x float> @vp_fadd(<4 x float> %a, <4 x float> %b, <4 x i1> %m, i32 %vl) {
  %r = call <4 x float> @llvm.vp.fadd.v4f32(<4 x float> %a, <4 x float> %b, <4 x i1> %m, i32 %vl)
  ret <4 x float> %r
}

declare <4 x float> @llvm.vp.fadd.v4f32(<4 x float>, <4 x float>, <4 x i1>, i32)
