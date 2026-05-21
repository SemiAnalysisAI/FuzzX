target triple = "x86_64-unknown-linux-gnu"

define i32 @test_te(i32* %p) {
  %v = load i32, i32* %p
  %z = icmp eq i32 %v, 0
  br i1 %z, label %t, label %f
t:
  ret i32 1
f:
  ret i32 0
}
