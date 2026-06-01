define ptx_kernel i32 @assert_variant() "nvvm.maxntid"="65536,65536,1" {
  %1 = call i32 @llvm.nvvm.read.ptx.sreg.ntid.x()
  ret i32 %1
}
declare i32 @llvm.nvvm.read.ptx.sreg.ntid.x()
