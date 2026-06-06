declare double @llvm.log2.f64(double)
declare double @llvm.exp2.f64(double)

define double @exp2_log2(double %x) {
  %l = call double @llvm.log2.f64(double %x)
  %r = call reassoc double @llvm.exp2.f64(double %l)
  ret double %r
}
