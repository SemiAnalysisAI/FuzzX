define i8 @partial_aggregate_store_lane_poison(ptr %p, <2 x i8> %u) {
  %e0 = extractelement <2 x i8> %u, i32 0
  %agg = insertvalue [2 x i8] undef, i8 %e0, 0
  store [2 x i8] %agg, ptr %p, align 1
  %p1 = getelementptr i8, ptr %p, i64 1
  %v = load i8, ptr %p1, align 1
  ret i8 %v
}
