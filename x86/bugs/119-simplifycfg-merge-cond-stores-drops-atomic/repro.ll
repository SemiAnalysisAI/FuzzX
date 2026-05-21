target triple = "x86_64-unknown-linux-gnu"
define void @merge_cond_stores_atomic(i1 %c1, i1 %c2, ptr %p) {
entry:
  br i1 %c1, label %if.then, label %if.else
if.then:
  store atomic i32 1, ptr %p unordered, align 4
  br label %merge
if.else:
  br label %merge
merge:
  br i1 %c2, label %if.then2, label %if.end
if.then2:
  store atomic i32 2, ptr %p unordered, align 4
  br label %if.end
if.end:
  ret void
}
