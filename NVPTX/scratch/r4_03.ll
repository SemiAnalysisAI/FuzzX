target triple = "nvptx64-nvidia-cuda"

declare half @llvm.nvvm.fmin.f16(half, half)

define half @t(half %a, half %b) #0 {
  %r = call half @llvm.nvvm.fmin.f16(half %a, half %b)
  ret half %r
}

attributes #0 = { denormal_fpenv(float: preservesign) }
