target triple = "x86_64-unknown-linux-gnu"
@g = global i32 0
@s = global { i32, i32 } { i32 ptrtoint (ptr @g to i32), i32 305419896 }
