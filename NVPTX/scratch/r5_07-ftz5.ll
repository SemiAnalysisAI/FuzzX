target triple = "nvptx64-nvidia-cuda"
define float @fadd_ftz(float %a, float %b) {
  %r = fadd float %a, %b
  ret float %r
}
!llvm.module.flags = !{!0}
!0 = !{i32 4, !"nvvm-reflect-ftz", i32 1}
