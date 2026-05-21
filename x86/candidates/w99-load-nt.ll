target triple = "x86_64-unknown-linux-gnu"

define <4 x i32> @test(ptr %p) {
entry:
  %a = load <4 x i32>, ptr %p, align 16
  %b = load <4 x i32>, ptr %p, align 16, !nontemporal !0
  %r = add <4 x i32> %a, %b
  ret <4 x i32> %r
}

!0 = !{i32 1}
