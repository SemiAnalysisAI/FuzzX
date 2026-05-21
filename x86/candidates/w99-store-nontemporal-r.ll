target triple = "x86_64-unknown-linux-gnu"

define void @test(ptr %p, i32 %v) {
entry:
  store i32 %v, ptr %p, align 4
  store i32 %v, ptr %p, align 4, !nontemporal !0
  ret void
}

!0 = !{i32 1}
