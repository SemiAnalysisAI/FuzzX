target datalayout = "e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-i128:128-f80:128-n8:16:32:64-S128"
target triple = "x86_64-unknown-linux-gnu"

define i64 @pr64669_2(i1 %cmp) {
  %mul = select i1 %cmp, i64 undef, i64 1
  %conv3 = zext i1 %cmp to i64
  %add = add i64 %mul, %conv3
  ret i64 %add
}
