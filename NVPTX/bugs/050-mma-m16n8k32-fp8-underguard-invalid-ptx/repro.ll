declare {float,float,float,float} @llvm.nvvm.mma.m16n8k32.row.col.f32.e4m3.e4m3.f32(i32, i32, i32, i32, i32, i32, float, float, float, float)
define {float,float,float,float} @t(i32 %a0,i32 %a1,i32 %a2,i32 %a3,i32 %b0,i32 %b1, float %c0, float %c1, float %c2, float %c3) {
  %r = call {float,float,float,float} @llvm.nvvm.mma.m16n8k32.row.col.f32.e4m3.e4m3.f32(i32 %a0,i32 %a1,i32 %a2,i32 %a3,i32 %b0,i32 %b1, float %c0, float %c1, float %c2, float %c3)
  ret {float,float,float,float} %r
}
; e5m2 variant: replace e4m3 with e5m2 in the intrinsic name (same signature).
