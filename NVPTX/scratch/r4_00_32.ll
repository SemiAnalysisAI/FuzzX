target triple = "nvptx-nvidia-cuda"
@g = addrspace(1) global i32 0
@s = addrspace(1) global { i32, i32 } { i32 ptrtoint (ptr addrspace(1) @g to i32), i32 305419896 }
