target triple = "nvptx64-nvidia-cuda"

; Does store <8 x i1> pack into 1 byte?
define void @storevec(ptr %p, <8 x i1> %a) {
  store <8 x i1> %a, ptr %p
  ret void
}

; What does a load + extractelement from memory look like?
define i32 @loadextract(ptr %p) {
  %v = load <8 x i1>, ptr %p
  %e7 = extractelement <8 x i1> %v, i32 7
  %z = zext i1 %e7 to i32
  ret i32 %z
}

@g = global <8 x i1> <i1 0, i1 0, i1 0, i1 0, i1 0, i1 0, i1 0, i1 1>
