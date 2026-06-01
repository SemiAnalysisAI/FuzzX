target triple = "nvptx64-nvidia-cuda"

; minimumnum(-0.0, +0.0) MUST be -0.0 (IEEE-754-2019 minimumNumber).
define float @minimumnum_f32(float %a, float %b) {
  %r = call float @llvm.minimumnum.f32(float %a, float %b)
  ret float %r
}

; maximumnum(-0.0, +0.0) MUST be +0.0.
define float @maximumnum_f32(float %a, float %b) {
  %r = call float @llvm.maximumnum.f32(float %a, float %b)
  ret float %r
}

define double @minimumnum_f64(double %a, double %b) {
  %r = call double @llvm.minimumnum.f64(double %a, double %b)
  ret double %r
}

; For contrast: llvm.minimum (signed-zero-aware) on f64. Should emit a fixup.
define double @minimum_f64(double %a, double %b) {
  %r = call double @llvm.minimum.f64(double %a, double %b)
  ret double %r
}

; Constant-folded reference: minimumnum(-0.0, +0.0) -> should be -0.0.
define float @minimumnum_const() {
  %r = call float @llvm.minimumnum.f32(float -0.0, float 0.0)
  ret float %r
}

declare float @llvm.minimumnum.f32(float, float)
declare float @llvm.maximumnum.f32(float, float)
declare double @llvm.minimumnum.f64(double, double)
declare double @llvm.minimum.f64(double, double)
