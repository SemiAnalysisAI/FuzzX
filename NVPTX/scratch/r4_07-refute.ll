target triple = "nvptx64-nvidia-cuda"

@g = addrspace(1) global i32 0

; cluster.ctaid.x legitimately returns 0 at runtime when there is no cluster.
; cluster_dim="0,0,0" is the AsmPrinter's recognized "no explicit cluster" form.
define ptx_kernel void @t() "nvvm.cluster_dim"="0,0,0" {
entry:
  %ctaid.x = call i32 @llvm.nvvm.read.ptx.sreg.cluster.ctaid.x()
  %is_zero = icmp eq i32 %ctaid.x, 0
  br i1 %is_zero, label %zero, label %nonzero

zero:
  store i32 111, ptr addrspace(1) @g
  br label %exit

nonzero:
  store i32 222, ptr addrspace(1) @g
  br label %exit

exit:
  ret void
}

declare i32 @llvm.nvvm.read.ptx.sreg.cluster.ctaid.x()
