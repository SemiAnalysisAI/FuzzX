target triple = "x86_64-unknown-linux-gnu"
define <2 x i1> @f(<2 x i32> %a, <2 x i32> %b) {
  %c = icmp samesign slt <2 x i32> %a, %b
  %r = shufflevector <2 x i1> %c, <2 x i1> poison, <2 x i32> <i32 1, i32 0>
  ret <2 x i1> %r
}
