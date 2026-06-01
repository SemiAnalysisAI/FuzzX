target triple = "nvptx64-nvidia-cuda"
@g = addrspace(1) global [4 x i32] zeroinitializer
@arr = addrspace(1) global [2 x i64] [i64 ptrtoint (ptr addrspace(1) @g to i64), i64 ptrtoint (ptr addrspace(1) getelementptr inbounds ([4 x i32], ptr addrspace(1) @g, i32 0, i32 2) to i64)]
