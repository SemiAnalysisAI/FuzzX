; RUN-INPUTS: 0x00000000*256
; RUN-LLVM-BUILD: build/llvm-fuzzer
; ModuleID = '/tmp/fuzzx-reduce-m046-1779158295/reduced.bc'
source_filename = "/tmp/fuzzx-amdgpu-orenamd@semianalysis.com/head-pr198373-196418-198412-198419-m045-20260519-023242/corpus/directed-gpu/shared/.seed-3284378.ll"
target datalayout = "e-m:e-p:64:64-p1:64:64-p2:32:32-p3:32:32-p4:64:64-p5:32:32-p6:32:32-p7:160:256:256:32-p8:128:128:128:48-p9:192:256:256:32-i64:64-v16:16-v24:32-v32:32-v48:64-v96:128-v192:256-v256:256-v512:512-v1024:1024-v2048:2048-n32:64-S32-A5-G1-ni:7:8:9"
target triple = "amdgcn-amd-amdhsa"

; Function Attrs: convergent nounwind
define amdgpu_kernel void @fuzz_kernel(ptr addrspace(1) %in, ptr addrspace(1) %out, i32 %n) #0 {
entry:
  %wg = call i32 @llvm.amdgcn.workgroup.id.x()
  %wi = call i32 @llvm.amdgcn.workitem.id.x()
  %block.base = mul i32 %wg, 256
  %idx = add i32 %block.base, %wi
  %ok = icmp ult i32 %idx, %n
  %idx64 = zext i32 %idx to i64
  %in.ptr = getelementptr i32, ptr addrspace(1) %in, i64 %idx64
  %v = load i32, ptr addrspace(1) %in.ptr, align 4
  %salt = mul i32 %idx, -1640531527
  %mix = xor i32 %v, %salt
  %out.ptr = getelementptr i32, ptr addrspace(1) %out, i64 %idx64
  %fuzz.nz = or i32 %block.base, 1
  %fuzz.udiv = udiv i32 %mix, %fuzz.nz
  %fuzz.maskshift.shift.mask = and i32 255, 31
  %fuzz.maskshift.signed = ashr i32 %fuzz.udiv, %fuzz.maskshift.shift.mask
  %fuzz.maskshift.unsigned = lshr i32 %fuzz.udiv, %fuzz.maskshift.shift.mask
  %fuzz.maskshift.sign = icmp slt i32 %fuzz.udiv, 0
  %fuzz.maskshift.select = select i1 %fuzz.maskshift.sign, i32 %fuzz.maskshift.signed, i32 %fuzz.maskshift.unsigned
  %fuzz.zext.i64 = zext i32 %fuzz.maskshift.select to i64
  %fuzz.zext.i641 = zext i32 %fuzz.nz to i64
  %fuzz.i64.usub_sat = call i64 @llvm.usub.sat.i64(i64 %fuzz.zext.i64, i64 %fuzz.zext.i641)
  %fuzz.trunc.i64 = trunc i64 %fuzz.i64.usub_sat to i32
  %fuzz.loop.nest.cond = icmp ult i32 0, 2
  br label %fuzz.loop.nest.header2

fuzz.loop.nest.header2:                           ; preds = %fuzz.nested.loop.exit9, %entry
  %fuzz.loop.nest.iv4 = phi i32 [ 0, %entry ], [ %fuzz.loop.nest.next42, %fuzz.nested.loop.exit9 ]
  %fuzz.loop.nest.acc5 = phi i32 [ %fuzz.trunc.i64, %entry ], [ %fuzz.loop.nest.acc.add, %fuzz.nested.loop.exit9 ]
  %fuzz.loop.nest.cond6 = icmp ult i32 %fuzz.loop.nest.iv4, 4
  br i1 %fuzz.loop.nest.cond6, label %fuzz.loop.nest.body3, label %fuzz.loop.nest.exit1

fuzz.loop.nest.body3:                             ; preds = %fuzz.loop.nest.header2
  %fuzz.loop.trip.inner.mask = and i32 %fuzz.loop.nest.acc5, 3
  %fuzz.loop.trip.inner = add i32 %fuzz.loop.trip.inner.mask, 1
  br label %fuzz.nested.loop.header7

fuzz.nested.loop.header7:                         ; preds = %fuzz.nested.loop.body8, %fuzz.loop.nest.body3
  %fuzz.loop.iv.inner10 = phi i32 [ 0, %fuzz.loop.nest.body3 ], [ %fuzz.loop.next.inner41, %fuzz.nested.loop.body8 ]
  %fuzz.loop.acc.inner11 = phi i32 [ %fuzz.loop.nest.acc5, %fuzz.loop.nest.body3 ], [ %fuzz.loop.acc.inner.mix, %fuzz.nested.loop.body8 ]
  %fuzz.loop.cond.inner12 = icmp ult i32 %fuzz.loop.iv.inner10, %fuzz.loop.trip.inner
  br i1 %fuzz.loop.cond.inner12, label %fuzz.nested.loop.body8, label %fuzz.nested.loop.exit9

fuzz.nested.loop.body8:                           ; preds = %fuzz.nested.loop.header7
  %fuzz.vec.narrow.trunc = trunc i32 %fuzz.loop.acc.inner11 to i16
  %fuzz.vec.narrow.trunc13 = trunc i32 2147483647 to i16
  %fuzz.vec.narrow.trunc14 = trunc i32 2147483647 to i16
  %fuzz.vec.narrow.trunc15 = trunc i32 -1 to i16
  %fuzz.vec.narrow.trunc16 = trunc i32 %fuzz.loop.acc.inner11 to i16
  %fuzz.vec.narrow.trunc17 = trunc i32 2147483647 to i16
  %fuzz.vec.narrow.trunc18 = trunc i32 2147483647 to i16
  %fuzz.vec.narrow.trunc19 = trunc i32 %fuzz.loop.acc.inner11 to i16
  %fuzz.vec.ins = insertelement <4 x i16> zeroinitializer, i16 %fuzz.vec.narrow.trunc, i32 0
  %fuzz.vec.ins20 = insertelement <4 x i16> %fuzz.vec.ins, i16 %fuzz.vec.narrow.trunc14, i32 1
  %fuzz.vec.ins21 = insertelement <4 x i16> %fuzz.vec.ins20, i16 %fuzz.vec.narrow.trunc16, i32 2
  %fuzz.vec.ins22 = insertelement <4 x i16> %fuzz.vec.ins21, i16 %fuzz.vec.narrow.trunc18, i32 3
  %fuzz.vec.ins23 = insertelement <4 x i16> zeroinitializer, i16 %fuzz.vec.narrow.trunc13, i32 0
  %fuzz.vec.ins24 = insertelement <4 x i16> %fuzz.vec.ins23, i16 %fuzz.vec.narrow.trunc15, i32 1
  %fuzz.vec.ins25 = insertelement <4 x i16> %fuzz.vec.ins24, i16 %fuzz.vec.narrow.trunc17, i32 2
  %fuzz.vec.ins26 = insertelement <4 x i16> %fuzz.vec.ins25, i16 %fuzz.vec.narrow.trunc19, i32 3
  %fuzz.vec.narrow.cttz = call <4 x i16> @llvm.cttz.v4i16(<4 x i16> %fuzz.vec.ins22, i1 false)
  %fuzz.vec.narrow.ext = extractelement <4 x i16> %fuzz.vec.narrow.cttz, i32 1
  %fuzz.vec.narrow.ext27 = extractelement <4 x i16> %fuzz.vec.narrow.cttz, i32 1
  %fuzz.vec.narrow.zext = zext i16 %fuzz.vec.narrow.ext to i32
  %fuzz.vec.narrow.sext = sext i16 %fuzz.vec.narrow.ext27 to i32
  %fuzz.vec.narrow.reduce.xor = xor i32 %fuzz.vec.narrow.zext, %fuzz.vec.narrow.sext
  %fuzz.cfg.select.idiom.smin.cmp = icmp slt i32 %fuzz.vec.narrow.reduce.xor, 941078264
  %fuzz.cfg.select.idiom.smin = select i1 %fuzz.cfg.select.idiom.smin.cmp, i32 %fuzz.vec.narrow.reduce.xor, i32 941078264
  %fuzz.vec.ins28 = insertelement <4 x i32> zeroinitializer, i32 %fuzz.cfg.select.idiom.smin, i32 0
  %fuzz.vec.ins29 = insertelement <4 x i32> %fuzz.vec.ins28, i32 -1, i32 1
  %fuzz.vec.ins30 = insertelement <4 x i32> %fuzz.vec.ins29, i32 %fuzz.cfg.select.idiom.smin, i32 2
  %fuzz.vec.ins31 = insertelement <4 x i32> %fuzz.vec.ins30, i32 -1, i32 3
  %fuzz.vec.ins32 = insertelement <4 x i32> zeroinitializer, i32 -1, i32 0
  %fuzz.vec.ins33 = insertelement <4 x i32> %fuzz.vec.ins32, i32 %fuzz.cfg.select.idiom.smin, i32 1
  %fuzz.vec.ins34 = insertelement <4 x i32> %fuzz.vec.ins33, i32 -1, i32 2
  %fuzz.vec.ins35 = insertelement <4 x i32> %fuzz.vec.ins34, i32 %fuzz.cfg.select.idiom.smin, i32 3
  %fuzz.vec.add = add <4 x i32> %fuzz.vec.ins31, %fuzz.vec.ins35
  %fuzz.vec.ext = extractelement <4 x i32> %fuzz.vec.add, i32 0
  %fuzz.vec.ext36 = extractelement <4 x i32> %fuzz.vec.add, i32 1
  %fuzz.vec.reduce.or = or i32 %fuzz.vec.ext, %fuzz.vec.ext36
  %fuzz.cfg.funnel.idiom.shift = and i32 522634553, 31
  %fuzz.cfg.funnel.idiom.inv.raw = sub i32 32, %fuzz.cfg.funnel.idiom.shift
  %fuzz.cfg.funnel.idiom.inv = and i32 %fuzz.cfg.funnel.idiom.inv.raw, 31
  %fuzz.cfg.funnel.idiom.zero = icmp eq i32 %fuzz.cfg.funnel.idiom.shift, 0
  %fuzz.cfg.funnel.idiom.left = shl i32 %fuzz.vec.reduce.or, %fuzz.cfg.funnel.idiom.inv
  %fuzz.cfg.funnel.idiom.right = lshr i32 -1, %fuzz.cfg.funnel.idiom.shift
  %fuzz.cfg.funnel.idiom.fshr.raw = or i32 %fuzz.cfg.funnel.idiom.left, %fuzz.cfg.funnel.idiom.right
  %fuzz.cfg.funnel.idiom.fshr = select i1 %fuzz.cfg.funnel.idiom.zero, i32 -1, i32 %fuzz.cfg.funnel.idiom.fshr.raw
  %fuzz.cfg.bool.cmp0 = icmp ne i32 %fuzz.cfg.funnel.idiom.fshr, -1
  %fuzz.cfg.bool.cmp1 = icmp ult i32 -1, 2
  %fuzz.cfg.bool.select = select i1 %fuzz.cfg.bool.cmp0, i1 %fuzz.cfg.bool.cmp1, i1 false
  %fuzz.cfg.bool.zext = zext i1 %fuzz.cfg.bool.select to i32
  %fuzz.cfg.bool.xor.i32 = xor i32 %fuzz.cfg.funnel.idiom.fshr, %fuzz.cfg.bool.zext
  %fuzz.cfg.bitcount.idiom.pop.a37 = call i32 @llvm.ctpop.i32(i32 %fuzz.cfg.bool.xor.i32)
  %fuzz.cfg.bitcount.idiom.pop.b38 = call i32 @llvm.ctpop.i32(i32 1)
  %fuzz.cfg.bitcount.idiom.pop.cmp39 = icmp ugt i32 %fuzz.cfg.bitcount.idiom.pop.a37, %fuzz.cfg.bitcount.idiom.pop.b38
  %fuzz.cfg.bitcount.idiom.pop.select40 = select i1 %fuzz.cfg.bitcount.idiom.pop.cmp39, i32 %fuzz.cfg.bool.xor.i32, i32 1
  %fuzz.loop.acc.inner.mix = xor i32 %fuzz.cfg.bitcount.idiom.pop.select40, %fuzz.loop.iv.inner10
  %fuzz.loop.next.inner41 = add i32 %fuzz.loop.iv.inner10, 1
  br label %fuzz.nested.loop.header7

fuzz.nested.loop.exit9:                           ; preds = %fuzz.nested.loop.header7
  %fuzz.loop.nest.acc.add = add i32 %fuzz.loop.acc.inner11, -1
  %fuzz.loop.nest.next42 = add i32 %fuzz.loop.nest.iv4, 1
  br label %fuzz.loop.nest.header2

fuzz.loop.nest.exit1:                             ; preds = %fuzz.loop.nest.header2
  store i32 %fuzz.loop.nest.acc5, ptr addrspace(1) %out.ptr, align 4
  ret void
}

; Function Attrs: nocallback nofree nosync nounwind speculatable willreturn memory(none)
declare noundef i32 @llvm.amdgcn.workgroup.id.x() #1

; Function Attrs: nocallback nofree nosync nounwind speculatable willreturn memory(none)
declare noundef range(i32 0, 1024) i32 @llvm.amdgcn.workitem.id.x() #1

; Function Attrs: nocallback nofree nosync nounwind speculatable willreturn memory(none)
declare i64 @llvm.usub.sat.i64(i64, i64) #2

; Function Attrs: nocallback nofree nosync nounwind speculatable willreturn memory(none)
declare i32 @llvm.cttz.i32(i32, i1 immarg) #1

; Function Attrs: nocallback nofree nosync nounwind speculatable willreturn memory(none)
declare i32 @llvm.ctpop.i32(i32) #2

; Function Attrs: nocallback nofree nosync nounwind speculatable willreturn memory(none)
declare i32 @llvm.usub.sat.i32(i32, i32) #2

; Function Attrs: nocallback nofree nosync nounwind speculatable willreturn memory(none)
declare <4 x i16> @llvm.cttz.v4i16(<4 x i16>, i1 immarg) #1

attributes #0 = { convergent nounwind "amdgpu-flat-work-group-size"="1,256" "target-cpu"="gfx950" "uniform-work-group-size" }
attributes #1 = { nocallback nofree nosync nounwind speculatable willreturn memory(none) }
attributes #2 = { nocallback nofree nosync nounwind speculatable willreturn memory(none) }
