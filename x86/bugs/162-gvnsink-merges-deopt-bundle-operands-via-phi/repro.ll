target triple = "x86_64-unknown-linux-gnu"
declare void @callee(i32) #0
declare void @use(i32)
define void @test(i1 %c, i32 %x, i32 %a, i32 %b) gc "statepoint-example" {
entry: br i1 %c, label %ba, label %bb
ba:
  call void @callee(i32 %x) [ "deopt"(i32 %a) ]
  call void @use(i32 %a)
  br label %end
bb:
  call void @callee(i32 %x) [ "deopt"(i32 %b) ]
  call void @use(i32 %b)
  br label %end
end: ret void
}
attributes #0 = { nounwind }
