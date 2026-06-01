target triple = "nvptx64-unknown-cuda"

declare { float, float, float, float } @llvm.nvvm.tex.unified.1d.v4f32.s32(i64, i32)
declare i64 @llvm.nvvm.texsurf.handle.internal.p1(ptr addrspace(1))

@tex0 = internal addrspace(1) global i64 0, align 8

define ptx_kernel void @sel(ptr %red, i32 %idx, i1 %c) {
entry:
  %h0 = tail call i64 @llvm.nvvm.texsurf.handle.internal.p1(ptr addrspace(1) @tex0)
  %val = tail call { float, float, float, float } @llvm.nvvm.tex.unified.1d.v4f32.s32(i64 %h0, i32 %idx)
  %ret = extractvalue { float, float, float, float } %val, 0
  store float %ret, ptr %red
  ret void
}
