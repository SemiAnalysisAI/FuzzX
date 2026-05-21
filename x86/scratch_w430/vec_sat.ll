target triple = "x86_64-unknown-linux-gnu"
define <2 x i256> @vec_sat(<2 x float> %x) {
  %r = call <2 x i256> @llvm.fptoui.sat.v2i256.v2f32(<2 x float> %x)
  ret <2 x i256> %r
}
declare <2 x i256> @llvm.fptoui.sat.v2i256.v2f32(<2 x float>)
