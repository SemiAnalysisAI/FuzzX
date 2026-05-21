target triple = "x86_64-unknown-linux-gnu"
define i1 @f() {
  %b = bitcast <2 x i16> <i16 0, i16 poison> to i32
  %c = icmp eq i32 %b, 0
  ret i1 %c
}
