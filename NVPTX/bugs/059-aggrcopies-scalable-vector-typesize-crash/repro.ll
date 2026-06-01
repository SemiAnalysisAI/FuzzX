target triple = "nvptx64-nvidia-cuda"

define void @copy_sv(ptr %dst, ptr %src) {
  %v = load <vscale x 4 x i32>, ptr %src
  store <vscale x 4 x i32> %v, ptr %dst
  ret void
}
