; RUN-LLVM-BUILD: build/llvm-fuzzer
; RUN-INPUTS: 0
; ModuleID = '<bc file>'
source_filename = "fuzzx_amdgpu_ir_diff"
target datalayout = "e-m:e-p:64:64-p1:64:64-p2:32:32-p3:32:32-p4:64:64-p5:32:32-p6:32:32-p7:160:256:256:32-p8:128:128:128:48-p9:192:256:256:32-i64:64-v16:16-v24:32-v32:32-v48:64-v96:128-v192:256-v256:256-v512:512-v1024:1024-v2048:2048-n32:64-S32-A5-G1-ni:7:8:9"
target triple = "amdgcn-amd-amdhsa"

; Function Attrs: convergent nounwind
define amdgpu_kernel void @fuzz_kernel(ptr addrspace(1) %in, ptr addrspace(1) %out) #0 {
entry:
  %wi = call i32 @llvm.amdgcn.workitem.id.x()
  %idx64 = zext nneg i32 %wi to i64
  %in.ptr = getelementptr [4 x i8], ptr addrspace(1) %in, i64 %idx64
  %v = load i32, ptr addrspace(1) %in.ptr, align 4
  %fuzz.halfdiffminmax.idiom.a.lo.zext = and i32 %v, 65535
  %fuzz.halfdiffminmax.idiom.prefix.minmax.umax = call i32 @llvm.umax.i32(i32 %fuzz.halfdiffminmax.idiom.a.lo.zext, i32 1542)
  %fuzz.halfdiffminmax.idiom.pack.shift32 = shl i32 %fuzz.halfdiffminmax.idiom.prefix.minmax.umax, 24
  %fuzz.halfdiffminmax.idiom.prefix.xor34 = or disjoint i32 %fuzz.halfdiffminmax.idiom.pack.shift32, 1798
  br label %fuzz.loop.header

fuzz.loop.header:                                 ; preds = %fuzz.loop.body, %entry
  %fuzz.loop.iv = phi i32 [ 0, %entry ], [ %fuzz.loop.next, %fuzz.loop.body ]
  %fuzz.loop.acc = phi i32 [ %fuzz.halfdiffminmax.idiom.prefix.xor34, %entry ], [ %fuzz.cfg.veci16zextmul.idiom.a.xor, %fuzz.loop.body ]
  %fuzz.loop.cond = icmp samesign ult i32 %fuzz.loop.iv, 12
  br i1 %fuzz.loop.cond, label %fuzz.loop.body, label %fuzz.loop.exit

fuzz.loop.body:                                   ; preds = %fuzz.loop.header
  %fuzz.cfg.veci16zextmul.idiom.half.trunc = trunc i32 %fuzz.loop.acc to i16
  %fuzz.cfg.veci16zextmul.idiom.half.shr = lshr i32 %fuzz.loop.acc, 16
  %fuzz.cfg.veci16zextmul.idiom.half.trunc5 = trunc nuw i32 %fuzz.cfg.veci16zextmul.idiom.half.shr to i16
  %0 = insertelement <4 x i16> <i16 poison, i16 -21013, i16 poison, i16 -31491>, i16 %fuzz.cfg.veci16zextmul.idiom.half.trunc, i64 0
  %fuzz.vec.ins14 = insertelement <4 x i16> %0, i16 %fuzz.cfg.veci16zextmul.idiom.half.trunc5, i64 2
  %fuzz.cfg.veci16zextmul.idiom.other.trunc22 = or i16 %fuzz.cfg.veci16zextmul.idiom.half.trunc5, 1
  %fuzz.vec.ins30 = insertelement <4 x i16> <i16 -21013, i16 -21013, i16 poison, i16 poison>, i16 %fuzz.cfg.veci16zextmul.idiom.other.trunc22, i64 2
  %fuzz.vec.ins31 = insertelement <4 x i16> %fuzz.vec.ins30, i16 %fuzz.cfg.veci16zextmul.idiom.other.trunc22, i64 3
  %fuzz.cfg.veci16zextmul.idiom.v.zext = zext <4 x i16> %fuzz.vec.ins14 to <4 x i32>
  %fuzz.cfg.veci16zextmul.idiom.w.zext = zext <4 x i16> %fuzz.vec.ins31 to <4 x i32>
  %fuzz.cfg.veci16zextmul.idiom.prod = mul nuw <4 x i32> %fuzz.cfg.veci16zextmul.idiom.v.zext, %fuzz.cfg.veci16zextmul.idiom.w.zext
  %fuzz.cfg.veci16zextmul.idiom.prod.rev = shufflevector <4 x i32> %fuzz.cfg.veci16zextmul.idiom.prod, <4 x i32> zeroinitializer, <4 x i32> <i32 poison, i32 2, i32 poison, i32 0>
  %fuzz.cfg.veci16zextmul.idiom.sum.rev = xor <4 x i32> %fuzz.cfg.veci16zextmul.idiom.prod, %fuzz.cfg.veci16zextmul.idiom.prod.rev
  %fuzz.cfg.veci16zextmul.idiom.reduce.lane32 = extractelement <4 x i32> %fuzz.cfg.veci16zextmul.idiom.sum.rev, i64 1
  %fuzz.cfg.veci16zextmul.idiom.reduce.lane35 = extractelement <4 x i32> %fuzz.cfg.veci16zextmul.idiom.sum.rev, i64 3
  %fuzz.cfg.veci16zextmul.idiom.reduce.sminmax.smax37 = call i32 @llvm.smax.i32(i32 %fuzz.cfg.veci16zextmul.idiom.reduce.lane32, i32 %fuzz.cfg.veci16zextmul.idiom.reduce.lane35)
  %fuzz.cfg.veci16zextmul.idiom.a.xor = xor i32 %fuzz.cfg.veci16zextmul.idiom.reduce.sminmax.smax37, %fuzz.loop.acc
  %fuzz.loop.next = add nuw nsw i32 %fuzz.loop.iv, 1
  br label %fuzz.loop.header

fuzz.loop.exit:                                   ; preds = %fuzz.loop.header
  %out.ptr = getelementptr [4 x i8], ptr addrspace(1) %out, i64 %idx64
  %fuzz.umaxbitop3cascade.idiom.not.a = xor i32 %fuzz.loop.acc, -1
  %fuzz.umaxbitop3cascade.idiom.lane.xor = and i32 %fuzz.loop.acc, 25552825
  %fuzz.umaxbitop3cascade.idiom.acc.xor5 = xor i32 %fuzz.umaxbitop3cascade.idiom.lane.xor, 12776413
  %fuzz.umaxbitop3cascade.idiom.acc.next6 = add nuw nsw i32 %fuzz.umaxbitop3cascade.idiom.acc.xor5, 59
  %fuzz.umaxbitop3cascade.idiom.acc.xor14 = xor i32 %fuzz.umaxbitop3cascade.idiom.acc.next6, -28662607
  %fuzz.umaxbitop3cascade.idiom.acc.next15 = add nuw nsw i32 %fuzz.umaxbitop3cascade.idiom.acc.xor14, 117
  %fuzz.umaxbitop3cascade.idiom.mix = lshr i32 %fuzz.loop.acc, 14
  %fuzz.umaxbitop3cascade.idiom.mix.shr = xor i32 %fuzz.umaxbitop3cascade.idiom.mix, 1559
  %fuzz.umaxbitop3cascade.idiom.a.or.mix = or i32 %fuzz.loop.acc, %fuzz.umaxbitop3cascade.idiom.mix.shr
  %fuzz.umaxbitop3cascade.idiom.min.and.max = and i32 %fuzz.umaxbitop3cascade.idiom.mix.shr, %fuzz.umaxbitop3cascade.idiom.not.a
  %fuzz.umaxbitop3cascade.idiom.x.xor.y = xor i32 %fuzz.umaxbitop3cascade.idiom.a.or.mix, %fuzz.umaxbitop3cascade.idiom.not.a
  %fuzz.umaxbitop3cascade.idiom.lane.or = or i32 %fuzz.umaxbitop3cascade.idiom.min.and.max, %fuzz.umaxbitop3cascade.idiom.x.xor.y
  %fuzz.umaxbitop3cascade.idiom.acc.xor20 = xor i32 %fuzz.umaxbitop3cascade.idiom.acc.next15, %fuzz.umaxbitop3cascade.idiom.lane.or
  %fuzz.umaxbitop3cascade.idiom.acc.next21 = add i32 %fuzz.umaxbitop3cascade.idiom.acc.xor20, 175
  %fuzz.umaxbitop3cascade.idiom.a.add = add i32 %fuzz.umaxbitop3cascade.idiom.acc.next21, %fuzz.loop.acc
  store i32 %fuzz.umaxbitop3cascade.idiom.a.add, ptr addrspace(1) %out.ptr, align 4
  ret void

; uselistorder directives
  uselistorder i32 %fuzz.loop.acc, { 2, 3, 0, 1, 4, 5, 6, 7 }
  uselistorder <4 x i32> %fuzz.cfg.veci16zextmul.idiom.prod, { 1, 0 }
  uselistorder i32 %fuzz.umaxbitop3cascade.idiom.not.a, { 1, 0 }
}

; Function Attrs: nocallback nofree nosync nounwind speculatable willreturn memory(none)
declare noundef i32 @llvm.amdgcn.workgroup.id.x() #1

; Function Attrs: nocallback nofree nosync nounwind speculatable willreturn memory(none)
declare noundef range(i32 0, 1024) i32 @llvm.amdgcn.workitem.id.x() #1

; Function Attrs: nocallback nocreateundeforpoison nofree nosync nounwind speculatable willreturn memory(none)
declare i32 @llvm.umax.i32(i32, i32) #2

; Function Attrs: nocallback nocreateundeforpoison nofree nosync nounwind speculatable willreturn memory(none)
declare i32 @llvm.smax.i32(i32, i32) #2

attributes #0 = { convergent nounwind "amdgpu-flat-work-group-size"="1,256" "target-cpu"="gfx950" "uniform-work-group-size" }
attributes #1 = { nocallback nofree nosync nounwind speculatable willreturn memory(none) }
attributes #2 = { nocallback nocreateundeforpoison nofree nosync nounwind speculatable willreturn memory(none) }
