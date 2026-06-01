define i64 @passthru_i64(i64 %x) {
  %v = call i64 asm "mov.u64 $0, $1;", "=r,r"(i64 %x)
  ret i64 %v
}

define double @passthru_f64(double %x) {
  %v = call double asm "mov.f64 $0, $1;", "=f,f"(double %x)
  ret double %v
}

define ptr @passthru_ptr(ptr %x) {
  %v = call ptr asm "mov.u64 $0, $1;", "=r,r"(ptr %x)
  ret ptr %v
}

; correct reference: using 'l' (B64) constraint
define i64 @passthru_i64_correct(i64 %x) {
  %v = call i64 asm "mov.u64 $0, $1;", "=l,l"(i64 %x)
  ret i64 %v
}
