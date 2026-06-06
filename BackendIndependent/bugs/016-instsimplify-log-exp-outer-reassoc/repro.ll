declare double @llvm.exp.f64(double)
declare double @llvm.log.f64(double)

define double @log_exp(double %x) {
  %e = call double @llvm.exp.f64(double %x)
  %r = call reassoc double @llvm.log.f64(double %e)
  ret double %r
}
