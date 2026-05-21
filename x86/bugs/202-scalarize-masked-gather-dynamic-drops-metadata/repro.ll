target triple = "x86_64-unknown-linux-gnu"
define i1 @gather_dyn_range_fold(<4 x ptr> %ptrs, <4 x i1> %m, <4 x i32> %src) {
  %v = call <4 x i32> @llvm.masked.gather.v4i32.v4p0(<4 x ptr> %ptrs, i32 4, <4 x i1> %m, <4 x i32> <i32 0, i32 0, i32 0, i32 0>), !range !0
  %e = extractelement <4 x i32> %v, i32 0
  %c = icmp uge i32 %e, 2
  ret i1 %c
}
declare <4 x i32> @llvm.masked.gather.v4i32.v4p0(<4 x ptr>, i32, <4 x i1>, <4 x i32>)
!0 = !{i32 0, i32 2}
