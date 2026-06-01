target triple = "nvptx64-nvidia-cuda"

declare void @sink(<8 x i1> %a)

define void @caller(<8 x i1> %a) {
  call void @sink(<8 x i1> %a)
  ret void
}

define i32 @callee(<8 x i1> %a) {
  %e7 = extractelement <8 x i1> %a, i32 7
  %z = zext i1 %e7 to i32
  ret i32 %z
}
