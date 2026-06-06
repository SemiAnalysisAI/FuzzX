declare double @llvm.exp2.f64(double)
declare double @llvm.log2.f64(double)

define double @log2_exp2(double %x) {
  %e = call double @llvm.exp2.f64(double %x)
  %r = call reassoc double @llvm.log2.f64(double %e)
  ret double %r
}
