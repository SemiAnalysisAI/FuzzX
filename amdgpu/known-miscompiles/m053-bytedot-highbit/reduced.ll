; RUN-LLVM-BUILD: build/llvm-fuzzer
; RUN-INPUTS: 0x0 0x1
target datalayout = "e-m:e-p:64:64-p1:64:64-p2:32:32-p3:32:32-p4:64:64-p5:32:32-p6:32:32-p7:160:256:256:32-p8:128:128:128:48-p9:192:256:256:32-i64:64-v16:16-v24:32-v32:32-v48:64-v96:128-v192:256-v256:256-v512:512-v1024:1024-n32:64-S32-A5-G1-ni:7:8:9"
target triple = "amdgcn-amd-amdhsa"

define amdgpu_kernel void @fuzz_kernel(ptr addrspace(1) %in, ptr addrspace(1) %out, i32 %n) #0 {
entry:
  %wi = call i32 @llvm.amdgcn.workitem.id.x()
  %idx64 = zext i32 %wi to i64
  %in.ptr = getelementptr i32, ptr addrspace(1) %in, i64 %idx64
  %v = load i32, ptr addrspace(1) %in.ptr, align 4
  %salt = mul i32 %wi, -1640531527
  %mix = xor i32 %v, %salt
  %x = and i32 %mix, 15
  %width = add i32 15, 1
  %invwidth.raw = sub i32 32, %width
  %invwidth = and i32 %invwidth.raw, 31
  %mask = lshr i32 -1, %invwidth
  %shifted = lshr i32 %mix, %x
  %extracted = and i32 %shifted, %mask
  %rhs.mask = and i32 255, %mask
  %extract.xor = xor i32 %extracted, %rhs.mask
  %blend.mask = xor i32 %extract.xor, %mix
  %blend.left = and i32 255, %blend.mask
  %blend.not = xor i32 %blend.mask, -1
  %blend.right = and i32 %mix, %blend.not
  %blend = or i32 %blend.left, %blend.right
  %a0s = lshr i32 %mix, 8
  %a0t = trunc i32 %a0s to i8
  %a0 = zext i8 %a0t to i32
  %b0s = lshr i32 %mix, 24
  %b0t = trunc i32 %b0s to i8
  %b0 = zext i8 %b0t to i32
  %mul0 = mul i32 %a0, %b0
  %acc0 = sub i32 150, %mul0
  %a1t = trunc i32 %blend to i8
  %a1 = zext i8 %a1t to i32
  %b1t = trunc i32 %blend to i8
  %b1 = sext i8 %b1t to i32
  %mul1 = mul i32 %a1, %b1
  %mul1.byte = and i32 %mul1, 255
  %mul1.shift = shl i32 %mul1.byte, 4
  %acc1 = xor i32 %acc0, %mul1.shift
  %a2s = lshr i32 %mix, 16
  %a2t = trunc i32 %a2s to i8
  %a2 = sext i8 %a2t to i32
  %b2s = lshr i32 %blend, 16
  %b2t = trunc i32 %b2s to i8
  %b2 = zext i8 %b2t to i32
  %mul2 = mul i32 %a2, %b2
  %mul2.byte = and i32 %mul2, 255
  %mul2.shift = shl i32 %mul2.byte, 8
  %acc2 = xor i32 %acc1, %mul2.shift
  %a3s = lshr i32 %blend, 16
  %a3t = trunc i32 %a3s to i8
  %a3 = zext i8 %a3t to i32
  %b3s = lshr i32 %mix, 16
  %b3t = trunc i32 %b3s to i8
  %b3 = zext i8 %b3t to i32
  %mul3 = mul i32 %a3, %b3
  %mul3.byte = and i32 %mul3, 255
  %mul3.shift = shl i32 %mul3.byte, 12
  %acc3 = xor i32 %acc2, %mul3.shift
  %p0 = and i32 %mul0, 255
  %p1m = and i32 %mul1, 255
  %p1 = shl i32 %p1m, 8
  %or1 = or i32 %p0, %p1
  %p2m = and i32 %mul2, 255
  %p2 = shl i32 %p2m, 16
  %or2 = or i32 %or1, %p2
  %p3m = and i32 %mul3, 255
  %p3 = shl i32 %p3m, 24
  %packed = or i32 %or2, %p3
  %result = add i32 %acc3, %packed
  %high = and i32 %result, -2147483648
  %flipped = xor i32 %high, -2147483648
  %out.ptr = getelementptr i32, ptr addrspace(1) %out, i64 %idx64
  store i32 %flipped, ptr addrspace(1) %out.ptr, align 4
  ret void
}

declare i32 @llvm.amdgcn.workitem.id.x()

attributes #0 = { convergent nounwind "amdgpu-flat-work-group-size"="1,256" "target-cpu"="gfx950" "uniform-work-group-size"="true" }
