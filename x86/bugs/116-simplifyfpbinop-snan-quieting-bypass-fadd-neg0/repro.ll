define float @fadd_neg0(float %x) {
  %r = fadd float %x, -0.0
  ret float %r
}
define float @fsub_pos0(float %x) {
  %r = fsub float %x, 0.0
  ret float %r
}
