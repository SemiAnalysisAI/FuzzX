declare double @llvm.log.f64(double)
declare double @llvm.exp.f64(double)

define double @exp_log(double %x) {
  %l = call double @llvm.log.f64(double %x)
  %r = call reassoc double @llvm.exp.f64(double %l)
  ret double %r
}
