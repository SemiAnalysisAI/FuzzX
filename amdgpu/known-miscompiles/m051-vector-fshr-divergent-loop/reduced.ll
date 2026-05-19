; RUN-INPUTS: 0x00000000 0x00000001
; RUN-LLVM-BUILD: build/llvm-fuzzer
target datalayout = "e-m:e-p:64:64-p1:64:64-p2:32:32-p3:32:32-p4:64:64-p5:32:32-p6:32:32-p7:160:256:256:32-p8:128:128:128:48-p9:192:256:256:32-i64:64-v16:16-v24:32-v32:32-v48:64-v96:128-v192:256-v256:256-v512:512-v1024:1024-v2048:2048-n32:64-S32-A5-G1-ni:7:8:9"
target triple = "amdgcn-amd-amdhsa"

define amdgpu_kernel void @fuzz_kernel(ptr addrspace(1) %in, ptr addrspace(1) %out, i32 %n) #0 {
entry:
  %wi = call i32 @llvm.amdgcn.workitem.id.x()
  %idx64 = zext i32 %wi to i64
  %in.ptr = getelementptr i32, ptr addrspace(1) %in, i64 %idx64
  %v = load i32, ptr addrspace(1) %in.ptr, align 4
  %salt = mul i32 %wi, -1640531527
  %mix = xor i32 %v, %salt
  %a2.shr = lshr i32 %mix, 16
  %a2.trunc = trunc i32 %a2.shr to i8
  %a2.zext = zext i8 %a2.trunc to i32
  %a3.shr = lshr i32 %mix, 24
  %a3.trunc = trunc i32 %a3.shr to i8
  %a3.zext = zext i8 %a3.trunc to i32
  %out.ptr = getelementptr i32, ptr addrspace(1) %out, i64 %idx64
  %trip.mask = and i32 %a2.zext, 3
  %trip = add i32 %trip.mask, 1
  br label %outer

outer:
  %outer.iv = phi i32 [ 0, %entry ], [ %outer.next, %inner.exit ]
  %outer.acc = phi i32 [ %a3.zext, %entry ], [ %inner.acc, %inner.exit ]
  %outer.cond = icmp ult i32 %outer.iv, %trip
  br i1 %outer.cond, label %outer.body, label %exit

outer.body:
  %inner.trip.mask = and i32 %outer.acc, 3
  %inner.trip = add i32 %inner.trip.mask, 1
  br label %inner

inner:
  %inner.iv = phi i32 [ 0, %outer.body ], [ %inner.next, %tail.exit ]
  %inner.acc = phi i32 [ %outer.acc, %outer.body ], [ %tail.mix, %tail.exit ]
  %inner.cond = icmp ult i32 %inner.iv, %inner.trip
  br i1 %inner.cond, label %tail.body, label %inner.exit

tail.body:
  %v0 = insertelement <2 x i32> zeroinitializer, i32 %inner.acc, i32 0
  %v2 = insertelement <2 x i32> zeroinitializer, i32 %inner.acc, i32 1
  %fshr = call <2 x i32> @llvm.fshr.v2i32(<2 x i32> %v0, <2 x i32> %v2, <2 x i32> <i32 2, i32 0>)
  %ext = extractelement <2 x i32> %fshr, i32 0
  %trunc = trunc i32 %ext to i8
  %mul = mul i8 %trunc, 85
  %sext = sext i8 %mul to i32
  %add = add i32 %sext, -2028201370
  %tail.value = or i32 %add, 2
  br label %tail.exit

tail.exit:
  %tail.mix = xor i32 %tail.value, %inner.iv
  %inner.next = add i32 %inner.iv, 1
  br label %inner

inner.exit:
  %outer.next = add i32 %outer.iv, 1
  br label %outer

exit:
  store i32 %outer.acc, ptr addrspace(1) %out.ptr, align 4
  ret void
}

declare i32 @llvm.amdgcn.workitem.id.x()
declare <2 x i32> @llvm.fshr.v2i32(<2 x i32>, <2 x i32>, <2 x i32>)
attributes #0 = { convergent nounwind "amdgpu-flat-work-group-size"="1,256" "target-cpu"="gfx950" "uniform-work-group-size"="true" }
