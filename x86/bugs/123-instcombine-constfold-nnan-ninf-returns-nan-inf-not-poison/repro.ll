define float @nnan_div_zero() {
  %r = fdiv nnan float 0.0, 0.0
  ret float %r
}
define float @nnan_mul_inf_zero() {
  %r = fmul nnan float 0x7FF0000000000000, 0.0
  ret float %r
}
define double @ninf_add_max_max() {
  %r = fadd ninf double 0x7FEFFFFFFFFFFFFF, 0x7FEFFFFFFFFFFFFF
  ret double %r
}
