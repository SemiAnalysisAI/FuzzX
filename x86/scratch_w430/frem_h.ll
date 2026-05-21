target triple = "x86_64-unknown-linux-gnu"

define half @frem_h(half %a, half %b) {
  %r = frem half %a, %b
  ret half %r
}
