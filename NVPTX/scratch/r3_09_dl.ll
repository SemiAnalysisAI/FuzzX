target triple = "nvptx64-nvidia-cuda"

; Demonstrate the in-memory layout of <8 x i1>: store then load via i8 alias.
define void @layout(ptr %p, <8 x i1> %v) {
  store <8 x i1> %v, ptr %p
  ret void
}
