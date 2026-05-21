target triple = "x86_64-pc-windows-msvc"
declare i32 @__CxxFrameHandler3(...)
declare void @throws()

define void @f() personality ptr @__CxxFrameHandler3 {
entry:
  invoke void @throws() to label %cont unwind label %cd
cont:
  ret void
cd:
  %cs = catchswitch within none [label %c] unwind to caller
c:
  %cp = catchpad within %cs [ptr null, i32 64, ptr null]
  catchret from %cp to label %cont
}

!llvm.module.flags = !{!0}
!0 = !{i32 8, !"cf-protection-branch", i32 1}
