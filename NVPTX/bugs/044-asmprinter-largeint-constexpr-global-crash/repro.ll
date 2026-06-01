target triple = "nvptx64-nvidia-cuda"
@g = global i96 bitcast (<3 x i32> <i32 1, i32 2, i32 3> to i96)
; also crashes: @g2 = global i128 bitcast (<2 x i64> <i64 1, i64 2> to i128)
