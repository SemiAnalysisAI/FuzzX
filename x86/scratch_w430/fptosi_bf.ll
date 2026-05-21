target triple = "x86_64-unknown-linux-gnu"

define i256 @si_bf(bfloat %x) {
  %r = fptosi bfloat %x to i256
  ret i256 %r
}
