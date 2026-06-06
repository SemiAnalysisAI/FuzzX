define float @x_div_c_mul_k(float %x) {
  %d = fdiv reassoc float %x, 1.000000e+01
  %r = fmul reassoc float %d, 3.000000e+00
  ret float %r
}
