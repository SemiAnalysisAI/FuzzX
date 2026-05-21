target triple = "x86_64-unknown-linux-gnu"
define void @f(i1 %c1, i1 %c2, ptr %p) {
entry: br i1 %c1, label %if.then, label %if.else
if.then:
  store i32 1, ptr %p, align 4, !nontemporal !1
  br label %merge
if.else: br label %merge
merge: br i1 %c2, label %if.then2, label %if.end
if.then2:
  store i32 2, ptr %p, align 4
  br label %if.end
if.end: ret void
}
!1 = !{i32 1}
