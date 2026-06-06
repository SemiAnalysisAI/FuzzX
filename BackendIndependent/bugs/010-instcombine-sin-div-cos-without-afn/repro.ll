declare double @llvm.sin.f64(double)
declare double @llvm.cos.f64(double)

define double @sin_div_cos(double %x) {
  %s = call double @llvm.sin.f64(double %x)
  %c = call double @llvm.cos.f64(double %x)
  %r = fdiv reassoc double %s, %c
  ret double %r
}
