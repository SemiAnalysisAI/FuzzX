target triple = "nvptx64-nvidia-cuda"

declare void @llvm.nvvm.cp.async.bulk.tensor.reduce.add.tile.1d(ptr addrspace(3), ptr, i32, i64, i1 immarg)
declare void @llvm.nvvm.cp.async.bulk.tensor.reduce.and.im2col.3d(ptr addrspace(3), ptr, i32, i32, i32, i64, i1 immarg)

define void @red1d(ptr addrspace(3) %src, ptr %tmap, i32 %d0) {
  call void @llvm.nvvm.cp.async.bulk.tensor.reduce.add.tile.1d(ptr addrspace(3) %src, ptr %tmap, i32 %d0, i64 0, i1 0)
  ret void
}

define void @red_im2col(ptr addrspace(3) %src, ptr %tmap, i32 %d0, i32 %d1, i32 %d2) {
  call void @llvm.nvvm.cp.async.bulk.tensor.reduce.and.im2col.3d(ptr addrspace(3) %src, ptr %tmap, i32 %d0, i32 %d1, i32 %d2, i64 0, i1 0)
  ret void
}
