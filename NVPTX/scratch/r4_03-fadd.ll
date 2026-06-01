target triple = "nvptx64-nvidia-cuda"
define half @fadd_f16_floatftz(half %a, half %b) #0 {
  %r = fadd half %a, %b
  ret half %r
}
define float @fadd_f32_floatftz(float %a, float %b) #0 {
  %r = fadd float %a, %b
  ret float %r
}
define half @fadd_f16_defftz(half %a, half %b) #1 {
  %r = fadd half %a, %b
  ret half %r
}
attributes #0 = { denormal_fpenv(float: preservesign) }
attributes #1 = { denormal_fpenv(preservesign) }
