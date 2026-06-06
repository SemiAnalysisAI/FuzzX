define <4 x i1> @alm_wrap() {
  %m = call <4 x i1> @llvm.get.active.lane.mask.v4i1.i64(i64 18446744073709551614, i64 2)
  ret <4 x i1> %m
}

declare <4 x i1> @llvm.get.active.lane.mask.v4i1.i64(i64, i64)
