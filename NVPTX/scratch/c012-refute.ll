target datalayout = "e-m:e-i64:64-f80:128-n8:16:32:64-S128"
target triple = "nvptx64-unknown-unknown"

define void @copy_overlap(ptr %p) {
entry:
  %dst = getelementptr inbounds i8, ptr %p, i64 8
  %v = load [128 x i8], ptr %p, align 1
  store [128 x i8] %v, ptr %dst, align 1
  ret void
}
