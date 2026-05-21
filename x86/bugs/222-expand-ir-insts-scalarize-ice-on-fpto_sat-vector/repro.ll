target triple = "x86_64-unknown-linux-gnu"
declare <2 x i256> @llvm.fptoui.sat.v2i256.v2f32(<2 x float>)
define <2 x i256> @f(<2 x float> %x) {
  %r = call <2 x i256> @llvm.fptoui.sat.v2i256.v2f32(<2 x float> %x)
  ret <2 x i256> %r
}
