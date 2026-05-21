; llvm.ldexp.<fty>.<ity> allows any integer width for the exponent. On x86_64
; the C libcall is `double ldexp(double, int)` — i.e. the exponent is 32-bit.
; LegalizeDAG's FLDEXP path doesn't truncate/sign-extend or error like FPOWI;
; it just emits `jmp ldexp@PLT`. For an i64 exponent the upper 32 bits are
; silently dropped.

declare double @llvm.ldexp.f64.i64(double, i64)

define double @t(double %a, i64 %e) {
  %r = call double @llvm.ldexp.f64.i64(double %a, i64 %e)
  ret double %r
}
