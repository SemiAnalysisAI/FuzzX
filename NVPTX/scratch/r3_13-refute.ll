target triple = "nvptx64-nvidia-cuda"

define float @ext_f16(half %a) #0 {
  %r = fpext half %a to float
  ret float %r
}

attributes #0 = { denormal_fpenv(float: preservesign) }
