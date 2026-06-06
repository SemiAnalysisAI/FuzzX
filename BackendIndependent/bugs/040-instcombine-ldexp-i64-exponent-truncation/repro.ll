target triple = "x86_64-unknown-linux-gnu"

declare double @llvm.ldexp.f64.i64(double, i64)

define double @g(double %x) {
  %r = call double @llvm.ldexp.f64.i64(double %x, i64 4294967330)
  ret double %r
}
