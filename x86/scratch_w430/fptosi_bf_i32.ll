target triple = "x86_64-unknown-linux-gnu"

define i32 @si_bf(bfloat %x) {
  %r = fptosi bfloat %x to i32
  ret i32 %r
}
