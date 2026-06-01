target triple = "nvptx64-nvidia-cuda"

define half @t_h(ptr %p, half %v) {
  %r = atomicrmw fadd ptr %p, half %v seq_cst
  ret half %r
}
define bfloat @t_bf(ptr %p, bfloat %v) {
  %r = atomicrmw fadd ptr %p, bfloat %v seq_cst
  ret bfloat %r
}
