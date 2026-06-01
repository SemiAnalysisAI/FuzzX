target triple = "nvptx64-nvidia-cuda"
declare half @llvm.nvvm.fmin.f16(half, half)
; default-mode preservesign (half ftz ON): fold should NOT fire (FTZ_MustBeOff needs ftz off)
define half @t_defftz(half %a, half %b) #1 {
  %r = call half @llvm.nvvm.fmin.f16(half %a, half %b)
  ret half %r
}
attributes #1 = { denormal_fpenv(preservesign) }
