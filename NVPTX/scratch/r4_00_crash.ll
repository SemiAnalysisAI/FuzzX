target triple = "nvptx64-nvidia-cuda"
@g = addrspace(1) global i32 0
@h = addrspace(1) global i32 0
@s2 = addrspace(1) global <{ i32, ptr addrspace(1) }> <{ i32 ptrtoint (ptr addrspace(1) @g to i32), ptr addrspace(1) @h }>
