declare double @llvm.exp2.f64(double)

define double @div_by_exp2(double %z, double %y) {
  %e = call double @llvm.exp2.f64(double %y)
  %r = fdiv reassoc arcp double %z, %e
  ret double %r
}
