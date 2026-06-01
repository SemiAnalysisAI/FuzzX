; Branching prmt chain: each prmt feeds BOTH operands of the next, so the
; un-incremented Depth in computeKnownBitsForPRMT causes 2^N re-derivation.
; n=24 already takes seconds; a long *linear* chain (~60k) stack-overflows.
target triple = "nvptx64-nvidia-cuda"
declare i32 @llvm.nvvm.prmt(i32, i32, i32)
define i32 @c(i32 %x) {
  %v0 = call i32 @llvm.nvvm.prmt(i32 %x, i32 %x, i32 17)
  %v1 = call i32 @llvm.nvvm.prmt(i32 %v0, i32 %v0, i32 17)
  %v2 = call i32 @llvm.nvvm.prmt(i32 %v1, i32 %v1, i32 17)
  %v3 = call i32 @llvm.nvvm.prmt(i32 %v2, i32 %v2, i32 17)
  %v4 = call i32 @llvm.nvvm.prmt(i32 %v3, i32 %v3, i32 17)
  %v5 = call i32 @llvm.nvvm.prmt(i32 %v4, i32 %v4, i32 17)
  %v6 = call i32 @llvm.nvvm.prmt(i32 %v5, i32 %v5, i32 17)
  %v7 = call i32 @llvm.nvvm.prmt(i32 %v6, i32 %v6, i32 17)
  %v8 = call i32 @llvm.nvvm.prmt(i32 %v7, i32 %v7, i32 17)
  %v9 = call i32 @llvm.nvvm.prmt(i32 %v8, i32 %v8, i32 17)
  %v10 = call i32 @llvm.nvvm.prmt(i32 %v9, i32 %v9, i32 17)
  %v11 = call i32 @llvm.nvvm.prmt(i32 %v10, i32 %v10, i32 17)
  %v12 = call i32 @llvm.nvvm.prmt(i32 %v11, i32 %v11, i32 17)
  %v13 = call i32 @llvm.nvvm.prmt(i32 %v12, i32 %v12, i32 17)
  %v14 = call i32 @llvm.nvvm.prmt(i32 %v13, i32 %v13, i32 17)
  %v15 = call i32 @llvm.nvvm.prmt(i32 %v14, i32 %v14, i32 17)
  %v16 = call i32 @llvm.nvvm.prmt(i32 %v15, i32 %v15, i32 17)
  %v17 = call i32 @llvm.nvvm.prmt(i32 %v16, i32 %v16, i32 17)
  %v18 = call i32 @llvm.nvvm.prmt(i32 %v17, i32 %v17, i32 17)
  %v19 = call i32 @llvm.nvvm.prmt(i32 %v18, i32 %v18, i32 17)
  %v20 = call i32 @llvm.nvvm.prmt(i32 %v19, i32 %v19, i32 17)
  %v21 = call i32 @llvm.nvvm.prmt(i32 %v20, i32 %v20, i32 17)
  %v22 = call i32 @llvm.nvvm.prmt(i32 %v21, i32 %v21, i32 17)
  %v23 = call i32 @llvm.nvvm.prmt(i32 %v22, i32 %v22, i32 17)
  %a = and i32 %v23, 255
  ret i32 %a
}
