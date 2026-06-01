target triple = "nvptx64-nvidia-cuda"
%s = type { <3 x i32>, i32 }
@g = global %s { <3 x i32> <i32 -1, i32 -1, i32 -1>, i32 -2 }

; also: @g_arr = global [2 x <3 x i32>] [<3 x i32> <i32 -1,i32 -1,i32 -1>, <3 x i32> <i32 -2,i32 -2,i32 -2>]
; also: %s16 = type { <3 x i16>, i16 } ; @g16 = global %s16 { <3 x i16> <i16 -1,i16 -1,i16 -1>, i16 -256 }
