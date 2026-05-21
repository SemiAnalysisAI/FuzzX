target triple = "x86_64-unknown-linux-gnu"
target datalayout = "e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-i128:128-f80:128-n8:16:32:64-S128"
define i32 @vec_extract_atomic(ptr %p) {
  %v  = load atomic <4 x i32>, ptr %p unordered, align 16
  %e0 = extractelement <4 x i32> %v, i32 0
  %e1 = extractelement <4 x i32> %v, i32 1
  %s  = add i32 %e0, %e1
  ret i32 %s
}
