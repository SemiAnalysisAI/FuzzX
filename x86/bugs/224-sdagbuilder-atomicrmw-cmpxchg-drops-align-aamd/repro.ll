target triple = "x86_64-unknown-linux-gnu"
define i32 @f(ptr %p) {
  %r = atomicrmw add ptr %p, i32 1 seq_cst, align 32
  ret i32 %r
}
