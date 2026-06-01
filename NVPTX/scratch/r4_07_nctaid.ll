target triple = "nvptx64-nvidia-cuda"
@g = addrspace(1) global i32 0
define ptx_kernel i32 @t() "nvvm.cluster_dim"="0,0,0" {
  %x = call i32 @llvm.nvvm.read.ptx.sreg.cluster.nctaid.x()
  store i32 %x, ptr addrspace(1) @g
  ret i32 %x
}
declare i32 @llvm.nvvm.read.ptx.sreg.cluster.nctaid.x()
