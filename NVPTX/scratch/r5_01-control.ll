define ptx_kernel i32 @control() "nvvm.maxntid"="1000,1000" {
  %1 = call i32 @llvm.nvvm.read.ptx.sreg.tid.x()
  %2 = call i32 @llvm.nvvm.read.ptx.sreg.ntid.x()
  %3 = call i32 @llvm.nvvm.read.ptx.sreg.tid.y()
  %4 = call i32 @llvm.nvvm.read.ptx.sreg.ntid.y()
  %5 = add i32 %1, %2
  %6 = add i32 %5, %3
  %7 = add i32 %6, %4
  ret i32 %7
}
declare i32 @llvm.nvvm.read.ptx.sreg.tid.x()
declare i32 @llvm.nvvm.read.ptx.sreg.ntid.x()
declare i32 @llvm.nvvm.read.ptx.sreg.tid.y()
declare i32 @llvm.nvvm.read.ptx.sreg.ntid.y()
