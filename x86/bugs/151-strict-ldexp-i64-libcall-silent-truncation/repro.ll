target triple = "x86_64-unknown-linux-gnu"
declare double @llvm.experimental.constrained.ldexp.f64.i64(double, i64, metadata, metadata)
define double @strict_ldexp_i64(double %x, i64 %e) strictfp {
  %r = call double @llvm.experimental.constrained.ldexp.f64.i64(
        double %x, i64 %e,
        metadata !"round.tonearest", metadata !"fpexcept.strict")
  ret double %r
}
