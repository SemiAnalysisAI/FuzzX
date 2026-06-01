target triple = "x86_64-unknown-linux-gnu"
@g = global i32 42
@arr = global [2 x i64] [i64 ptrtoint (ptr @g to i64), i64 add (i64 ptrtoint (ptr @g to i64), i64 16)]
