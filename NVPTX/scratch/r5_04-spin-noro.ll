target triple = "nvptx64-unknown-cuda"

define ptx_kernel void @spin(ptr noalias %flag) {
entry:
  %fg = addrspacecast ptr %flag to ptr addrspace(1)
  br label %loop
loop:
  %v = load volatile i32, ptr addrspace(1) %fg, align 4
  %done = icmp ne i32 %v, 0
  br i1 %done, label %exit, label %loop
exit:
  ret void
}
