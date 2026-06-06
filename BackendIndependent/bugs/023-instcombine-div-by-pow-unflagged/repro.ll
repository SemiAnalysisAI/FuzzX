declare double @llvm.pow.f64(double, double)

define double @div_by_pow(double %z, double %x, double %y) {
  %p = call double @llvm.pow.f64(double %x, double %y)
  %r = fdiv reassoc arcp double %z, %p
  ret double %r
}
