target triple = "nvptx64-nvidia-cuda"
@g = addrspace(1) global i32 42
@x = addrspace(1) global i64 add (i64 ptrtoint (ptr addrspace(1) @g to i64), i64 16)
