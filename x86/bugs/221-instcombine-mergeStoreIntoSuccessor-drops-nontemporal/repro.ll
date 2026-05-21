target triple = "x86_64-unknown-linux-gnu"
define void @f(i1 %c, ptr %p, i32 %x, i32 %y) {
entry:
  br i1 %c, label %t, label %f
t:
  store i32 %x, ptr %p, align 4, !nontemporal !0
  br label %end
f:
  store i32 %y, ptr %p, align 4, !nontemporal !0
  br label %end
end:
  ret void
}
!0 = !{i32 1}
