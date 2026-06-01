target triple = "nvptx64-nvidia-cuda"

define half @t_h_cta(ptr %p, half %v) {
  %r = call half @llvm.nvvm.atomic.add.gen.f.cta.f16.p0(ptr %p, half %v)
  ret half %r
}
define bfloat @t_bf_sys(ptr %p, bfloat %v) {
  %r = call bfloat @llvm.nvvm.atomic.add.gen.f.sys.bf16.p0(ptr %p, bfloat %v)
  ret bfloat %r
}
declare half @llvm.nvvm.atomic.add.gen.f.cta.f16.p0(ptr, half)
declare bfloat @llvm.nvvm.atomic.add.gen.f.sys.bf16.p0(ptr, bfloat)
