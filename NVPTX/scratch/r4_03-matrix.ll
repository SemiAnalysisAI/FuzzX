target triple = "nvptx64-nvidia-cuda"

declare half @llvm.nvvm.fmin.f16(half, half)
declare half @llvm.nvvm.fmin.ftz.f16(half, half)
declare half @llvm.minimumnum.f16(half, half)

; nvvm.fmin.f16 (non-ftz intrinsic) with float-only ftz attr
define half @nvvm_fmin_f16_floatftz(half %a, half %b) #0 {
  %r = call half @llvm.nvvm.fmin.f16(half %a, half %b)
  ret half %r
}
; nvvm.fmin.f16 (non-ftz intrinsic) with default (half) ftz attr
define half @nvvm_fmin_f16_defaultftz(half %a, half %b) #1 {
  %r = call half @llvm.nvvm.fmin.f16(half %a, half %b)
  ret half %r
}
; nvvm.fmin.ftz.f16 (ftz intrinsic) with no attr
define half @nvvm_fmin_ftz_f16_noattr(half %a, half %b) {
  %r = call half @llvm.nvvm.fmin.ftz.f16(half %a, half %b)
  ret half %r
}
; generic minimumnum.f16 with float-only ftz attr
define half @minimumnum_f16_floatftz(half %a, half %b) #0 {
  %r = call half @llvm.minimumnum.f16(half %a, half %b)
  ret half %r
}
; generic minimumnum.f16 with default (half) ftz attr
define half @minimumnum_f16_defaultftz(half %a, half %b) #1 {
  %r = call half @llvm.minimumnum.f16(half %a, half %b)
  ret half %r
}
; generic minimumnum.f16 with NO attr
define half @minimumnum_f16_noattr(half %a, half %b) {
  %r = call half @llvm.minimumnum.f16(half %a, half %b)
  ret half %r
}

attributes #0 = { denormal_fpenv(float: preservesign) }
attributes #1 = { denormal_fpenv(preservesign) }
