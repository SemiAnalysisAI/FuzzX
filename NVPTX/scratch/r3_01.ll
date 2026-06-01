target triple = "nvptx64-nvidia-cuda"

declare i32 @llvm.nvvm.prmt(i32, i32, i32)

define i32 @prmt_neg_selector(i32 %a, i32 %b) {
  %r = call i32 @llvm.nvvm.prmt(i32 %a, i32 %b, i32 -1)
  ret i32 %r
}

define i32 @prmt_sel_80000000(i32 %a, i32 %b) {
  %r = call i32 @llvm.nvvm.prmt(i32 %a, i32 %b, i32 -2147483648)
  ret i32 %r
}

define i32 @prmt_sel_ffff0000(i32 %a, i32 %b) {
  %r = call i32 @llvm.nvvm.prmt(i32 %a, i32 %b, i32 -65536)
  ret i32 %r
}

define i32 @prmt_sel_7654(i32 %a, i32 %b) {
  %r = call i32 @llvm.nvvm.prmt(i32 %a, i32 %b, i32 30292)
  ret i32 %r
}
