declare double @llvm.exp.f64(double)

define double @div_by_exp(double %z, double %y) {
  %e = call double @llvm.exp.f64(double %y)
  %r = fdiv reassoc arcp double %z, %e
  ret double %r
}
