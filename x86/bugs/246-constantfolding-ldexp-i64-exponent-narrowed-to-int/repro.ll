target triple = "x86_64-unknown-linux-gnu"
declare double @llvm.ldexp.f64.i64(double, i64)
define double @f() {
  %r = call double @llvm.ldexp.f64.i64(double 1.0, i64 4294967330)
  ret double %r
}
