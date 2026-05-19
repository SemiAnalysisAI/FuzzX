; RUN-INPUTS: 0x0,0x1
; RUN-LLVM-BUILD: build/llvm-fuzzer
source_filename = "known-miscompiles/m059-srem-loop-branch/reduced.ll"
target datalayout = "e-m:e-p:64:64-p1:64:64-p2:32:32-p3:32:32-p4:64:64-p5:32:32-p6:32:32-p7:160:256:256:32-p8:128:128:128:48-p9:192:256:256:32-i64:64-v16:16-v24:32-v32:32-v48:64-v96:128-v192:256-v256:256-v512:512-v1024:1024-v2048:2048-n32:64-S32-A5-G1-ni:7:8:9"
target triple = "amdgcn-amd-amdhsa"

; Function Attrs: convergent nounwind
define amdgpu_kernel void @fuzz_kernel(ptr addrspace(1) %in, ptr addrspace(1) %out, i32 %n) #0 {
entry:
  %wi = call i32 @llvm.amdgcn.workitem.id.x()
  br label %body

body:                                             ; preds = %entry
  %idx64 = zext i32 %wi to i64
  %salt = mul i32 %wi, -1640531527
  %out.ptr = getelementptr i32, ptr addrspace(1) %out, i64 %idx64
  br label %fuzz.loop.multi.header

fuzz.loop.multi.header:                           ; preds = %fuzz.loop.multi.header, %body
  %fuzz.loop.acc.multi = phi i32 [ %salt, %body ], [ 0, %fuzz.loop.multi.header ]
  %fuzz.cfg.bitslice.idiom.xor.ab = xor i32 %fuzz.loop.acc.multi, 735267362
  %fuzz.cfg.bitslice.idiom.lo = and i32 %fuzz.cfg.bitslice.idiom.xor.ab, 2147483647
  %fuzz.cfg.bitslice.idiom.not.mask = xor i32 2147483647, 0
  %fuzz.cfg.bitslice.idiom.hi = and i32 %fuzz.cfg.bitslice.idiom.xor.ab, %fuzz.cfg.bitslice.idiom.not.mask
  %fuzz.cfg.bitslice.idiom.mask.select = or i32 %fuzz.cfg.bitslice.idiom.lo, %fuzz.cfg.bitslice.idiom.hi
  %fuzz.cfg.halfcmp.idiom.hi.a.shr = lshr i32 %fuzz.cfg.bitslice.idiom.mask.select, 1
  %fuzz.cfg.halfcmp.idiom.hi.a.trunc = trunc i32 %fuzz.cfg.halfcmp.idiom.hi.a.shr to i16
  %fuzz.cfg.halfcmp.idiom.hi.a.sext = sext i16 %fuzz.cfg.halfcmp.idiom.hi.a.trunc to i32
  %fuzz.cfg.halfcmp.idiom.pack.hi.mask = and i32 %fuzz.cfg.halfcmp.idiom.hi.a.sext, 65535
  %fuzz.cfg.halfcmp.idiom.pack.hi.shift = shl i32 %fuzz.cfg.halfcmp.idiom.pack.hi.mask, 16
  %fuzz.cfg.srem.num.mask = and i32 %fuzz.cfg.halfcmp.idiom.pack.hi.shift, 8388607
  %fuzz.cfg.srem.op = srem i32 %fuzz.cfg.srem.num.mask, 35
  %fuzz.loop.multi.exit.key = and i32 %fuzz.cfg.srem.op, 1
  switch i32 %fuzz.loop.multi.exit.key, label %fuzz.loop.multi.header [
    i32 0, label %common.ret
    i32 1, label %fuzz.loop.multi.break.b
  ]

common.ret:                                       ; preds = %fuzz.loop.multi.header, %fuzz.loop.multi.break.b
  ret void

fuzz.loop.multi.break.b:                          ; preds = %fuzz.loop.multi.header
  store i32 1, ptr addrspace(1) %out.ptr, align 4
  br label %common.ret
}

; Function Attrs: nocallback nofree nosync nounwind speculatable willreturn memory(none)
declare noundef range(i32 0, 1024) i32 @llvm.amdgcn.workitem.id.x() #1

attributes #0 = { convergent nounwind "amdgpu-flat-work-group-size"="1,256" "target-cpu"="gfx950" "uniform-work-group-size"="true" }
attributes #1 = { nocallback nofree nosync nounwind speculatable willreturn memory(none) }
