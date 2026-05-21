define ptr @load_ig_asc(ptr %p) {
  %v = load ptr, ptr %p, align 8, !invariant.group !0
  %c = bitcast ptr %v to ptr
  ret ptr %c
}
!0 = !{!"vt"}
