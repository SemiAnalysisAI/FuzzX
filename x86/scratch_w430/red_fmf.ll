target triple = "x86_64-unknown-linux-gnu"

; Reassoc (unordered shuffle path):
define float @reassoc_fmul(float %acc, <4 x float> %v) {
  %r = call reassoc nsz nnan ninf float @llvm.vector.reduce.fmul.v4f32(float %acc, <4 x float> %v)
  ret float %r
}

; Ordered (no-reassoc) path:
define float @ord_fadd_fmf(float %acc, <4 x float> %v) {
  %r = call nnan ninf float @llvm.vector.reduce.fadd.v4f32(float %acc, <4 x float> %v)
  ret float %r
}

; max/min nnan needed:
define float @fmax_red(<4 x float> %v) {
  %r = call nnan float @llvm.vector.reduce.fmax.v4f32(<4 x float> %v)
  ret float %r
}

declare float @llvm.vector.reduce.fadd.v4f32(float, <4 x float>)
declare float @llvm.vector.reduce.fmul.v4f32(float, <4 x float>)
declare float @llvm.vector.reduce.fmax.v4f32(<4 x float>)
