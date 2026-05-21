target triple = "x86_64-unknown-linux-gnu"
target datalayout = "e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-i128:128-f80:128-n8:16:32:64-S128"
declare <4 x float> @llvm.matrix.column.major.load.v4f32.i64(ptr nocapture, i64, i1 immarg, i32 immarg, i32 immarg)
declare void @llvm.matrix.column.major.store.v1f32.i64(<1 x float>, ptr nocapture, i64, i1 immarg, i32 immarg, i32 immarg)
declare <1 x float> @llvm.matrix.multiply.v1f32.v4f32.v4f32(<4 x float>, <4 x float>, i32 immarg, i32 immarg, i32 immarg)
define void @matmul_fuse(ptr %a, ptr %b, ptr %c) {
  %A = call <4 x float> @llvm.matrix.column.major.load.v4f32.i64(ptr %a, i64 1, i1 true,  i32 1, i32 4)
  %B = call <4 x float> @llvm.matrix.column.major.load.v4f32.i64(ptr %b, i64 4, i1 false, i32 4, i32 1)
  %res = call reassoc <1 x float> @llvm.matrix.multiply.v1f32.v4f32.v4f32(<4 x float> %A, <4 x float> %B, i32 1, i32 4, i32 1)
  call void @llvm.matrix.column.major.store.v1f32.i64(<1 x float> %res, ptr %c, i64 1, i1 false, i32 1, i32 1)
  ret void
}
