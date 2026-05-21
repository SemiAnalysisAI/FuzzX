target triple = "x86_64-unknown-linux-gnu"
%S = type { i32, i32 }

define i32 @atomic_load_from_partial(ptr %src) {
  %a = alloca %S, align 8
  %ld = load i64, ptr %src, align 8
  store i64 %ld, ptr %a, align 8
  %p1 = getelementptr inbounds %S, ptr %a, i32 0, i32 1
  %r = load atomic i32, ptr %p1 seq_cst, align 4
  ret i32 %r
}

define void @atomic_store_to_partial(ptr %dst, i32 %x) {
  %a = alloca %S, align 8
  store atomic i32 %x, ptr %a seq_cst, align 4
  %p1 = getelementptr inbounds %S, ptr %a, i32 0, i32 1
  store i32 0, ptr %p1, align 4
  %v = load i64, ptr %a, align 8
  store i64 %v, ptr %dst, align 8
  ret void
}
