declare {float,float,float,float} @llvm.nvvm.mma.m16n8k8.row.col.bf16(i32, i32, i32, float, float, float, float)
define {float,float,float,float} @test_bf16(i32 %a0, i32 %a1, i32 %b0, float %c0, float %c1, float %c2, float %c3) {
  %r = call {float,float,float,float} @llvm.nvvm.mma.m16n8k8.row.col.bf16(i32 %a0, i32 %a1, i32 %b0, float %c0, float %c1, float %c2, float %c3)
  ret {float,float,float,float} %r
}

; tf32 variant (a=4 regs, b=2 regs):
declare {float,float,float,float} @llvm.nvvm.mma.m16n8k8.row.col.tf32(i32, i32, i32, i32, i32, i32, float, float, float, float)
define {float,float,float,float} @test_tf32(i32 %a0, i32 %a1, i32 %a2, i32 %a3, i32 %b0, i32 %b1, float %c0, float %c1, float %c2, float %c3) {
  %r = call {float,float,float,float} @llvm.nvvm.mma.m16n8k8.row.col.tf32(i32 %a0, i32 %a1, i32 %a2, i32 %a3, i32 %b0, i32 %b1, float %c0, float %c1, float %c2, float %c3)
  ret {float,float,float,float} %r
}
