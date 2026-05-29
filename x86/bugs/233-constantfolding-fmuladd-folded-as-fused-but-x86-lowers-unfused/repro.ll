target triple = "x86_64-unknown-linux-gnu"
declare double @llvm.fmuladd.f64(double, double, double)
define double @const_fold() {
  %r = call double @llvm.fmuladd.f64(double 0x3FF0000000000001, double 0x3FF0000000000001, double 0xBFF0000000000002)
  ret double %r
}
