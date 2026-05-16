; RUN-INPUTS: 0x0*129
; RUN-REPEAT: 100
; RUN-LLVM-BUILD: build/llvm-fuzzer
source_filename = "m013-private-memory-fshl"
target datalayout = "e-m:e-p:64:64-p1:64:64-p2:32:32-p3:32:32-p4:64:64-p5:32:32-p6:32:32-p7:160:256:256:32-p8:128:128:128:48-p9:192:256:256:32-i64:64-v16:16-v24:32-v32:32-v48:64-v96:128-v192:256-v256:256-v512:512-v1024:1024-v2048:2048-n32:64-S32-A5-G1-ni:7:8:9"
target triple = "amdgcn-amd-amdhsa"

define protected amdgpu_kernel void @fuzz_kernel(ptr addrspace(1) %in, ptr addrspace(1) %out, i32 %n) #0 {
entry:
  %wg = call i32 @llvm.amdgcn.workgroup.id.x()
  %wi = call i32 @llvm.amdgcn.workitem.id.x()
  %base = mul i32 %wg, 256
  %idx = add i32 %base, %wi
  %ok = icmp ult i32 %idx, %n
  br i1 %ok, label %body, label %exit

body:
  %idx64 = zext i32 %idx to i64
  %inptr = getelementptr i32, ptr addrspace(1) %in, i64 %idx64
  %x = load i32, ptr addrspace(1) %inptr, align 4
  %b0_slot = alloca [2 x i32], align 4, addrspace(5)
  %b0_val = xor i32 %x, -283988598
  %b0_p0 = getelementptr [2 x i32], ptr addrspace(5) %b0_slot, i32 0, i32 0
  store i32 %b0_val, ptr addrspace(5) %b0_p0, align 4
  %b0_p1 = getelementptr [2 x i32], ptr addrspace(5) %b0_slot, i32 0, i32 1
  store i32 0, ptr addrspace(5) %b0_p1, align 4
  %b0_loaded = load i32, ptr addrspace(5) %b0_p0, align 4
  %b0_sx = xor i32 %b0_loaded, -283988598
  %b0_shift = and i32 %b0_sx, 31
  %b0_f = call i32 @llvm.fshl.i32(i32 %b0_loaded, i32 -283988598, i32 %b0_shift)
  %b0_pop = call i32 @llvm.ctpop.i32(i32 %b0_f)
  %b1_slot = alloca [2 x i32], align 4, addrspace(5)
  %b1_val = xor i32 %b0_pop, -283988598
  %b1_p0 = getelementptr [2 x i32], ptr addrspace(5) %b1_slot, i32 0, i32 0
  store i32 %b1_val, ptr addrspace(5) %b1_p0, align 4
  %b1_p1 = getelementptr [2 x i32], ptr addrspace(5) %b1_slot, i32 0, i32 1
  store i32 0, ptr addrspace(5) %b1_p1, align 4
  %b1_loaded = load i32, ptr addrspace(5) %b1_p0, align 4
  %b1_sx = xor i32 %b1_loaded, -283988598
  %b1_shift = and i32 %b1_sx, 31
  %b1_f = call i32 @llvm.fshl.i32(i32 %b1_loaded, i32 -283988598, i32 %b1_shift)
  %b1_pop = call i32 @llvm.ctpop.i32(i32 %b1_f)
  %b2_slot = alloca [2 x i32], align 4, addrspace(5)
  %b2_val = xor i32 %b1_pop, -283988598
  %b2_p0 = getelementptr [2 x i32], ptr addrspace(5) %b2_slot, i32 0, i32 0
  store i32 %b2_val, ptr addrspace(5) %b2_p0, align 4
  %b2_p1 = getelementptr [2 x i32], ptr addrspace(5) %b2_slot, i32 0, i32 1
  store i32 0, ptr addrspace(5) %b2_p1, align 4
  %b2_loaded = load i32, ptr addrspace(5) %b2_p0, align 4
  %b2_sx = xor i32 %b2_loaded, -283988598
  %b2_shift = and i32 %b2_sx, 31
  %b2_f = call i32 @llvm.fshl.i32(i32 %b2_loaded, i32 -283988598, i32 %b2_shift)
  %b2_pop = call i32 @llvm.ctpop.i32(i32 %b2_f)
  %b3_slot = alloca [2 x i32], align 4, addrspace(5)
  %b3_val = xor i32 %b2_pop, -283988598
  %b3_p0 = getelementptr [2 x i32], ptr addrspace(5) %b3_slot, i32 0, i32 0
  store i32 %b3_val, ptr addrspace(5) %b3_p0, align 4
  %b3_p1 = getelementptr [2 x i32], ptr addrspace(5) %b3_slot, i32 0, i32 1
  store i32 0, ptr addrspace(5) %b3_p1, align 4
  %b3_loaded = load i32, ptr addrspace(5) %b3_p0, align 4
  %b3_sx = xor i32 %b3_loaded, -283988598
  %b3_shift = and i32 %b3_sx, 31
  %b3_f = call i32 @llvm.fshl.i32(i32 %b3_loaded, i32 -283988598, i32 %b3_shift)
  %b3_pop = call i32 @llvm.ctpop.i32(i32 %b3_f)
  %b4_slot = alloca [2 x i32], align 4, addrspace(5)
  %b4_val = xor i32 %b3_pop, -283988598
  %b4_p0 = getelementptr [2 x i32], ptr addrspace(5) %b4_slot, i32 0, i32 0
  store i32 %b4_val, ptr addrspace(5) %b4_p0, align 4
  %b4_p1 = getelementptr [2 x i32], ptr addrspace(5) %b4_slot, i32 0, i32 1
  store i32 0, ptr addrspace(5) %b4_p1, align 4
  %b4_loaded = load i32, ptr addrspace(5) %b4_p0, align 4
  %b4_sx = xor i32 %b4_loaded, -283988598
  %b4_shift = and i32 %b4_sx, 31
  %b4_f = call i32 @llvm.fshl.i32(i32 %b4_loaded, i32 -283988598, i32 %b4_shift)
  %outptr = getelementptr i32, ptr addrspace(1) %out, i64 %idx64
  store i32 %b4_f, ptr addrspace(1) %outptr, align 4
  br label %exit

exit:
  ret void
}

declare i32 @llvm.amdgcn.workgroup.id.x()
declare i32 @llvm.amdgcn.workitem.id.x()
declare i32 @llvm.fshl.i32(i32, i32, i32)
declare i32 @llvm.ctpop.i32(i32)

attributes #0 = { convergent nounwind "amdgpu-flat-work-group-size"="1,256" "target-cpu"="gfx950" "uniform-work-group-size"="true" }
