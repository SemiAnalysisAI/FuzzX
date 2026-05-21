target triple = "x86_64-unknown-linux-gnu"
define <2 x i64> @f(ptr %p) {
  %r = load atomic <2 x i64>, ptr %p seq_cst, align 16
  ret <2 x i64> %r
}
