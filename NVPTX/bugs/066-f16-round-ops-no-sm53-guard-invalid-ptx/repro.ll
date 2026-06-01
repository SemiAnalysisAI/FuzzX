target triple = "nvptx64-nvidia-cuda"

define half @ceil_f16(half %a) {
  %r = call half @llvm.ceil.f16(half %a)
  ret half %r
}
define half @trunc_f16(half %a) {
  %r = call half @llvm.trunc.f16(half %a)
  ret half %r
}
define half @floor_f16(half %a) {
  %r = call half @llvm.floor.f16(half %a)
  ret half %r
}
define half @rint_f16(half %a) {
  %r = call half @llvm.rint.f16(half %a)
  ret half %r
}
define half @roundeven_f16(half %a) {
  %r = call half @llvm.roundeven.f16(half %a)
  ret half %r
}
declare half @llvm.ceil.f16(half)
declare half @llvm.trunc.f16(half)
declare half @llvm.floor.f16(half)
declare half @llvm.rint.f16(half)
declare half @llvm.roundeven.f16(half)
