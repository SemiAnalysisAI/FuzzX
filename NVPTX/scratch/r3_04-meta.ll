target triple = "nvptx64-nvidia-cuda"

define internal void @ik(ptr byval(i32) %p) {
  ret void
}

!nvvm.annotations = !{!0}
!0 = !{ptr @ik, !"kernel", i32 1}
