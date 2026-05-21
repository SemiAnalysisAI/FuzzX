declare double @llvm.minimumnum.f64(double, double)
declare float  @llvm.minimumnum.f32(float, float)

; IEEE-754 minimumNumber(x, y):
;   - if exactly one input is NaN, return the other
;   - if BOTH inputs are NaN, return a quiet NaN (the sNaN payload must be quieted)
;
; Here %x is an arbitrary double; the second operand is a quiet NaN constant.
; DAGCombiner.cpp:visitFMinMax constant-folds this to `return X` unconditionally
; (no nnan check), which is wrong when %x is itself NaN — and in particular when
; %x is a signaling NaN, the result must be a qNaN but the lowered code returns
; the raw sNaN.

define i64 @minimumnum_x_qnan(double %x) {
  %r = call double @llvm.minimumnum.f64(double %x, double 0x7FF8000000000000)
  %i = bitcast double %r to i64
  ret i64 %i
}

define i32 @minimumnum_f32_x_qnan(float %x) {
  %r = call float @llvm.minimumnum.f32(float %x, float 0x7FF8000000000000)
  %i = bitcast float %r to i32
  ret i32 %i
}
