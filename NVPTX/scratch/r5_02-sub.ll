target triple = "nvptx64-nvidia-cuda"
@g = addrspace(1) global i32 42
@arr = addrspace(1) global [2 x i64] [i64 ptrtoint (ptr addrspace(1) @g to i64), i64 sub (i64 ptrtoint (ptr addrspace(1) @g to i64), i64 8)]
