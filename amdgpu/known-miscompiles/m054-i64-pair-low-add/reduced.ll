; RUN-LLVM-BUILD: build/llvm-fuzzer
; RUN-INPUTS: 0x0 0x1
target datalayout = "e-m:e-p:64:64-p1:64:64-p2:32:32-p3:32:32-p4:64:64-p5:32:32-p6:32:32-p7:160:256:256:32-p8:128:128:128:48-p9:192:256:256:32-i64:64-v16:16-v24:32-v32:32-v48:64-v96:128-v192:256-v256:256-v512:512-v1024:1024-n32:64-S32-A5-G1-ni:7:8:9"
target triple = "amdgcn-amd-amdhsa"

define amdgpu_kernel void @fuzz_kernel(ptr addrspace(1) %in, ptr addrspace(1) %out, i32 %n) #0 {
entry:
  %wi = call i32 @llvm.amdgcn.workitem.id.x()
  %n.bit = and i32 %n, 2
  %neg = sub i32 0, %n.bit
  %a24 = and i32 %neg, 16777214
  %a64 = zext i32 %a24 to i64
  %idx64 = zext i32 %wi to i64
  %prod = mul i64 %a64, %idx64
  %pair.hi = shl i64 %prod, 32
  %pair = or i64 %pair.hi, 65535
  %sum = add i64 %pair, %prod
  %hi = lshr i64 %sum, 32
  %fold64 = xor i64 %hi, %sum
  %fold = trunc i64 %fold64 to i32
  %out.ptr = getelementptr i32, ptr addrspace(1) %out, i64 %idx64
  store i32 %fold, ptr addrspace(1) %out.ptr, align 4
  ret void
}

declare i32 @llvm.amdgcn.workitem.id.x()

attributes #0 = { convergent nounwind "amdgpu-flat-work-group-size"="1,256" "target-cpu"="gfx950" "uniform-work-group-size"="true" }
