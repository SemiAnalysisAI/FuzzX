; RUN-LLVM-BUILD: build/llvm-fuzzer
; RUN-INPUTS: 0
; ModuleID = '<bc file>'
source_filename = "/tmp/fuzzx-amdgpu-orenamd@semianalysis.com/head-pr198373-196418-198412-198419-gen13-20260519-085306/corpus/directed-gpu/shared/.seed-3996662.ll"
target datalayout = "e-m:e-p:64:64-p1:64:64-p2:32:32-p3:32:32-p4:64:64-p5:32:32-p6:32:32-p7:160:256:256:32-p8:128:128:128:48-p9:192:256:256:32-i64:64-v16:16-v24:32-v32:32-v48:64-v96:128-v192:256-v256:256-v512:512-v1024:1024-v2048:2048-n32:64-S32-A5-G1-ni:7:8:9"
target triple = "amdgcn-amd-amdhsa"

; Function Attrs: convergent nounwind
define amdgpu_kernel void @fuzz_kernel(ptr addrspace(1) %in, ptr addrspace(1) %out, i32 %n) #0 {
entry:
  %wi = call i32 @llvm.amdgcn.workitem.id.x()
  %ok = icmp ult i32 0, 1
  br i1 true, label %body, label %exit

body:                                             ; preds = %entry
  %salt = mul i32 %wi, 0
  %fuzz.umin = call i32 @llvm.umin.i32(i32 %salt, i32 0)
  br label %fuzz.loop.multi.header

fuzz.loop.multi.header:                           ; preds = %fuzz.loop.multi.continue, %body
  %fuzz.loop.acc.multi = phi i32 [ %fuzz.umin, %body ], [ 0, %fuzz.loop.multi.continue ]
  %fuzz.cfg.i64cmppack.idiom.a.lo = and i32 %fuzz.loop.acc.multi, 0
  %fuzz.cfg.i64cmppack.idiom.a.lo64 = zext i32 %fuzz.cfg.i64cmppack.idiom.a.lo to i64
  %fuzz.cfg.i64cmppack.idiom.sgt = icmp sgt i64 %fuzz.cfg.i64cmppack.idiom.a.lo64, 0
  %fuzz.cfg.i64cmppack.idiom.select32 = select i1 %fuzz.cfg.i64cmppack.idiom.sgt, i32 0, i32 0
  %fuzz.loop.multi.exit.key = and i32 %fuzz.cfg.i64cmppack.idiom.select32, 0
  switch i32 %fuzz.loop.multi.exit.key, label %fuzz.loop.multi.continue [
    i32 0, label %fuzz.loop.multi.break.a
    i32 1, label %fuzz.loop.multi.break.b
  ]

fuzz.loop.multi.break.a:                          ; preds = %fuzz.loop.multi.header
  br label %fuzz.loop.multi.exit

fuzz.loop.multi.break.b:                          ; preds = %fuzz.loop.multi.header
  br label %fuzz.loop.multi.exit

fuzz.loop.multi.continue:                         ; preds = %fuzz.loop.multi.header
  br label %fuzz.loop.multi.header

fuzz.loop.multi.exit:                             ; preds = %fuzz.loop.multi.break.b, %fuzz.loop.multi.break.a
  %fuzz.limb.idiom.signed.bias = and i32 3, 255
  %fuzz.bytecarry.idiom.x.byte.trunc = trunc i32 %fuzz.limb.idiom.signed.bias to i8
  %fuzz.bytecarry.idiom.x.byte.zext = zext i8 %fuzz.bytecarry.idiom.x.byte.trunc to i32
  %fuzz.bytecarry.idiom.sum.byte = and i32 %fuzz.bytecarry.idiom.x.byte.zext, 255
  %fuzz.bytecarry.idiom.y.byte.trunc20 = trunc i32 255 to i8
  %fuzz.bytecarry.idiom.y.byte.zext21 = zext i8 %fuzz.bytecarry.idiom.y.byte.trunc20 to i32
  %fuzz.bytecarry.idiom.sum.byte25 = and i32 %fuzz.bytecarry.idiom.y.byte.zext21, 255
  %fuzz.bytecarry.idiom.fold.xor31 = xor i32 %fuzz.bytecarry.idiom.sum.byte, %fuzz.bytecarry.idiom.sum.byte25
  %fuzz.bytecarry.idiom.fold.sub = sub i32 %fuzz.bytecarry.idiom.fold.xor31, %fuzz.bytecarry.idiom.sum.byte
  %fuzz.vecbytegather.idiom.b1.trunc = trunc i32 0 to i8
  %fuzz.vecbytegather.idiom.b1.zext = zext i8 0 to i32
  %fuzz.vecbytegather.idiom.b1.shl = shl i32 0, 0
  %fuzz.vecbytegather.idiom.b1.trunc11 = trunc i32 %fuzz.bytecarry.idiom.fold.sub to i8
  %fuzz.vecbytegather.idiom.b1.zext12 = zext i8 %fuzz.vecbytegather.idiom.b1.trunc11 to i32
  %fuzz.vecbytegather.idiom.b1.shl13 = shl i32 %fuzz.vecbytegather.idiom.b1.zext12, 8
  %fuzz.vecbytegather.idiom.elt15 = add i32 %fuzz.vecbytegather.idiom.b1.shl13, 514
  %fuzz.vec.ins = insertelement <4 x i32> zeroinitializer, i32 0, i32 0
  %fuzz.vec.ins25 = insertelement <4 x i32> zeroinitializer, i32 0, i32 0
  %fuzz.vec.ins26 = insertelement <4 x i32> zeroinitializer, i32 %fuzz.vecbytegather.idiom.elt15, i32 2
  %fuzz.vec.ins27 = insertelement <4 x i32> %fuzz.vec.ins26, i32 0, i32 0
  %fuzz.vecbytegather.idiom.rot = shufflevector <4 x i32> %fuzz.vec.ins27, <4 x i32> zeroinitializer, <4 x i32> <i32 1, i32 2, i32 3, i32 0>
  %fuzz.vecbytegather.idiom.xor = xor <4 x i32> %fuzz.vecbytegather.idiom.rot, splat (i32 1)
  %fuzz.vecbytegather.idiom.lane = extractelement <4 x i32> zeroinitializer, i32 0
  %fuzz.vecbytegather.idiom.lane31 = extractelement <4 x i32> %fuzz.vecbytegather.idiom.xor, i32 1
  %fuzz.vecbytegather.idiom.fold33 = xor i32 0, %fuzz.vecbytegather.idiom.lane31
  %fuzz.vecbytegather.idiom.lane34 = extractelement <4 x i32> zeroinitializer, i32 0
  %fuzz.vecbytegather.idiom.lane.byte35 = lshr i32 0, 0
  %fuzz.vecbytegather.idiom.fold36 = xor i32 %fuzz.vecbytegather.idiom.fold33, 0
  %fuzz.vecbytegather.idiom.pack.mask42 = and i32 0, 0
  %fuzz.vecbytegather.idiom.pack.shift43 = shl i32 0, 0
  %fuzz.vecbytegather.idiom.xor.fold = xor i32 0, %fuzz.vecbytegather.idiom.fold36
  %fuzz.bytecondsel.idiom.key.next15 = add i32 0, 20
  %fuzz.bytecondsel.idiom.key.xor29 = xor i32 %fuzz.bytecondsel.idiom.key.next15, 58
  %fuzz.bytecondsel.idiom.key.next30 = add i32 %fuzz.bytecondsel.idiom.key.xor29, 21
  %fuzz.bytecondsel.idiom.a.byte.shr31 = lshr i32 %fuzz.vecbytegather.idiom.xor.fold, 0
  %fuzz.bytecondsel.idiom.a.byte.trunc32 = trunc i32 %fuzz.bytecondsel.idiom.a.byte.shr31 to i8
  %fuzz.bytecondsel.idiom.a.byte.zext33 = zext i8 %fuzz.bytecondsel.idiom.a.byte.trunc32 to i32
  %fuzz.bytecondsel.idiom.c.byte.shr36 = lshr i32 %fuzz.vecbytegather.idiom.xor.fold, 8
  %fuzz.bytecondsel.idiom.c.byte.trunc37 = trunc i32 %fuzz.bytecondsel.idiom.c.byte.shr36 to i8
  %fuzz.bytecondsel.idiom.c.byte.zext38 = zext i8 %fuzz.bytecondsel.idiom.c.byte.trunc37 to i32
  %fuzz.bytecondsel.idiom.cmp.first39 = icmp ult i32 %fuzz.bytecondsel.idiom.a.byte.zext33, 0
  %fuzz.bytecondsel.idiom.comb.mask = and i1 %fuzz.bytecondsel.idiom.cmp.first39, %fuzz.bytecondsel.idiom.cmp.first39
  %fuzz.bytecondsel.idiom.sel.three42 = select i1 %fuzz.bytecondsel.idiom.comb.mask, i32 %fuzz.bytecondsel.idiom.c.byte.zext38, i32 0
  %fuzz.bytecondsel.idiom.key.xor44 = xor i32 %fuzz.bytecondsel.idiom.key.next30, %fuzz.bytecondsel.idiom.sel.three42
  %fuzz.bytecondsel.idiom.key.next45 = add i32 %fuzz.bytecondsel.idiom.key.xor44, 22
  store i32 %fuzz.bytecondsel.idiom.key.next45, ptr addrspace(1) %out, align 4
  ret void

exit:                                             ; preds = %entry
  ret void

; uselistorder directives
  uselistorder i1 %fuzz.bytecondsel.idiom.cmp.first39, { 1, 0 }
}

; Function Attrs: nocallback nofree nosync nounwind speculatable willreturn memory(none)
declare noundef i32 @llvm.amdgcn.workgroup.id.x() #1

; Function Attrs: nocallback nofree nosync nounwind speculatable willreturn memory(none)
declare noundef range(i32 0, 1024) i32 @llvm.amdgcn.workitem.id.x() #1

; Function Attrs: nocallback nocreateundeforpoison nofree nosync nounwind speculatable willreturn memory(none)
declare i32 @llvm.umin.i32(i32, i32) #2

attributes #0 = { convergent nounwind "amdgpu-flat-work-group-size"="1,256" "target-cpu"="gfx950" "uniform-work-group-size" }
attributes #1 = { nocallback nofree nosync nounwind speculatable willreturn memory(none) }
attributes #2 = { nocallback nocreateundeforpoison nofree nosync nounwind speculatable willreturn memory(none) }
