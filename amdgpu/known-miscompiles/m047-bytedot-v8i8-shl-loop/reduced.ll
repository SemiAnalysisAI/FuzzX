; RUN-INPUTS: 0x00000000*256
; RUN-LLVM-BUILD: build/llvm-fuzzer
; ModuleID = '/tmp/fuzzx-reduce-m047-1779158307/reduced.bc'
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
  %fuzz.loop.multi.trip.mask = and i32 %idx, 7
  %fuzz.loop.multi.trip = add i32 %fuzz.loop.multi.trip.mask, 1
  %fuzz.loop.multi.cond = icmp ult i32 0, %fuzz.loop.multi.trip
  %fuzz.loop.nest.trip.mask = and i32 %mix, 3
  %fuzz.loop.nest.trip = add i32 %fuzz.loop.nest.trip.mask, 1
  %fuzz.loop.nest.cond = icmp ult i32 0, %fuzz.loop.nest.trip
  %fuzz.avgdiff.idiom.a64 = zext i32 %mix to i64
  %fuzz.avgdiff.idiom.b64 = zext i32 %v to i64
  %fuzz.avgdiff.idiom.sum64 = add i64 %fuzz.avgdiff.idiom.a64, %fuzz.avgdiff.idiom.b64
  %fuzz.avgdiff.idiom.sum64.round = add i64 %fuzz.avgdiff.idiom.sum64, 1
  %fuzz.avgdiff.idiom.avg64 = lshr i64 %fuzz.avgdiff.idiom.sum64.round, 1
  %fuzz.avgdiff.idiom.avg.round = trunc i64 %fuzz.avgdiff.idiom.avg64 to i32
  br label %fuzz.loop.nest.header2

fuzz.loop.nest.header2:                           ; preds = %fuzz.nested.loop.exit9, %entry
  %fuzz.loop.nest.iv4 = phi i32 [ 0, %entry ], [ %fuzz.loop.nest.next67, %fuzz.nested.loop.exit9 ]
  %fuzz.loop.nest.acc5 = phi i32 [ %fuzz.avgdiff.idiom.avg.round, %entry ], [ %fuzz.loop.acc.inner11, %fuzz.nested.loop.exit9 ]
  %fuzz.loop.nest.cond6 = icmp ult i32 %fuzz.loop.nest.iv4, 1
  br i1 %fuzz.loop.nest.cond6, label %fuzz.loop.nest.body3, label %fuzz.loop.nest.exit1

fuzz.loop.nest.body3:                             ; preds = %fuzz.loop.nest.header2
  %fuzz.loop.trip.inner.mask = and i32 %fuzz.loop.nest.acc5, 3
  %fuzz.loop.trip.inner = add i32 %fuzz.loop.trip.inner.mask, 1
  br label %fuzz.nested.loop.header7

fuzz.nested.loop.header7:                         ; preds = %fuzz.nested.loop.body8, %fuzz.loop.nest.body3
  %fuzz.loop.iv.inner10 = phi i32 [ 0, %fuzz.loop.nest.body3 ], [ %fuzz.loop.next.inner66, %fuzz.nested.loop.body8 ]
  %fuzz.loop.acc.inner11 = phi i32 [ %fuzz.loop.nest.acc5, %fuzz.loop.nest.body3 ], [ %fuzz.loop.acc.inner.mix65, %fuzz.nested.loop.body8 ]
  %fuzz.loop.cond.inner12 = icmp ult i32 %fuzz.loop.iv.inner10, %fuzz.loop.trip.inner
  br i1 %fuzz.loop.cond.inner12, label %fuzz.nested.loop.body8, label %fuzz.nested.loop.exit9

fuzz.nested.loop.body8:                           ; preds = %fuzz.nested.loop.header7
  %fuzz.cfg.bytedot.idiom.lhs.ubyte.shr = lshr i32 %fuzz.loop.acc.inner11, 24
  %fuzz.cfg.bytedot.idiom.lhs.ubyte.trunc = trunc i32 %fuzz.cfg.bytedot.idiom.lhs.ubyte.shr to i8
  %fuzz.cfg.bytedot.idiom.lhs.ubyte.zext = zext i8 %fuzz.cfg.bytedot.idiom.lhs.ubyte.trunc to i32
  %fuzz.cfg.bytedot.idiom.rhs.ubyte.shr = lshr i32 %wg, 24
  %fuzz.cfg.bytedot.idiom.rhs.ubyte.trunc = trunc i32 %fuzz.cfg.bytedot.idiom.rhs.ubyte.shr to i8
  %fuzz.cfg.bytedot.idiom.rhs.ubyte.zext = zext i8 %fuzz.cfg.bytedot.idiom.rhs.ubyte.trunc to i32
  %fuzz.cfg.bytedot.idiom.mul = mul i32 %fuzz.cfg.bytedot.idiom.lhs.ubyte.zext, %fuzz.cfg.bytedot.idiom.rhs.ubyte.zext
  %fuzz.cfg.bytedot.idiom.acc.xor = xor i32 15, %fuzz.cfg.bytedot.idiom.mul
  %fuzz.cfg.bytedot.idiom.lhs.ubyte.trunc13 = trunc i32 %fuzz.loop.acc.inner11 to i8
  %fuzz.cfg.bytedot.idiom.lhs.ubyte.zext14 = zext i8 %fuzz.cfg.bytedot.idiom.lhs.ubyte.trunc13 to i32
  %fuzz.cfg.bytedot.idiom.rhs.sbyte.shr = lshr i32 %fuzz.loop.acc.inner11, 16
  %fuzz.cfg.bytedot.idiom.rhs.sbyte.trunc = trunc i32 %fuzz.cfg.bytedot.idiom.rhs.sbyte.shr to i8
  %fuzz.cfg.bytedot.idiom.rhs.sbyte.sext = sext i8 %fuzz.cfg.bytedot.idiom.rhs.sbyte.trunc to i32
  %fuzz.cfg.bytedot.idiom.mul15 = mul i32 %fuzz.cfg.bytedot.idiom.lhs.ubyte.zext14, %fuzz.cfg.bytedot.idiom.rhs.sbyte.sext
  %fuzz.cfg.bytedot.idiom.mul.low = and i32 %fuzz.cfg.bytedot.idiom.mul15, 65535
  %fuzz.cfg.bytedot.idiom.acc.add.low = add i32 %fuzz.cfg.bytedot.idiom.acc.xor, %fuzz.cfg.bytedot.idiom.mul.low
  %fuzz.cfg.bytedot.idiom.lhs.ubyte.shr16 = lshr i32 %wg, 16
  %fuzz.cfg.bytedot.idiom.lhs.ubyte.trunc17 = trunc i32 %fuzz.cfg.bytedot.idiom.lhs.ubyte.shr16 to i8
  %fuzz.cfg.bytedot.idiom.lhs.ubyte.zext18 = zext i8 %fuzz.cfg.bytedot.idiom.lhs.ubyte.trunc17 to i32
  %fuzz.cfg.bytedot.idiom.rhs.ubyte.trunc19 = trunc i32 %wg to i8
  %fuzz.cfg.bytedot.idiom.rhs.ubyte.zext20 = zext i8 %fuzz.cfg.bytedot.idiom.rhs.ubyte.trunc19 to i32
  %fuzz.cfg.bytedot.idiom.mul21 = mul i32 %fuzz.cfg.bytedot.idiom.lhs.ubyte.zext18, %fuzz.cfg.bytedot.idiom.rhs.ubyte.zext20
  %fuzz.cfg.bytedot.idiom.acc.add = add i32 %fuzz.cfg.bytedot.idiom.acc.add.low, %fuzz.cfg.bytedot.idiom.mul21
  %fuzz.cfg.bytedot.idiom.lhs.sbyte.shr = lshr i32 %wg, 16
  %fuzz.cfg.bytedot.idiom.lhs.sbyte.trunc = trunc i32 %fuzz.cfg.bytedot.idiom.lhs.sbyte.shr to i8
  %fuzz.cfg.bytedot.idiom.lhs.sbyte.sext = sext i8 %fuzz.cfg.bytedot.idiom.lhs.sbyte.trunc to i32
  %fuzz.cfg.bytedot.idiom.rhs.ubyte.shr22 = lshr i32 %wg, 16
  %fuzz.cfg.bytedot.idiom.rhs.ubyte.trunc23 = trunc i32 %fuzz.cfg.bytedot.idiom.rhs.ubyte.shr22 to i8
  %fuzz.cfg.bytedot.idiom.rhs.ubyte.zext24 = zext i8 %fuzz.cfg.bytedot.idiom.rhs.ubyte.trunc23 to i32
  %fuzz.cfg.bytedot.idiom.mul25 = mul i32 %fuzz.cfg.bytedot.idiom.lhs.sbyte.sext, %fuzz.cfg.bytedot.idiom.rhs.ubyte.zext24
  %fuzz.cfg.bytedot.idiom.acc.sub = sub i32 %fuzz.cfg.bytedot.idiom.acc.add, %fuzz.cfg.bytedot.idiom.mul25
  %fuzz.cfg.bytedot.idiom.pack.products.mask = and i32 %fuzz.cfg.bytedot.idiom.mul, 255
  %fuzz.cfg.bytedot.idiom.pack.products.add = add i32 0, %fuzz.cfg.bytedot.idiom.pack.products.mask
  %fuzz.cfg.bytedot.idiom.pack.products.mask26 = and i32 %fuzz.cfg.bytedot.idiom.mul15, 255
  %fuzz.cfg.bytedot.idiom.pack.products.shift = shl i32 %fuzz.cfg.bytedot.idiom.pack.products.mask26, 8
  %fuzz.cfg.bytedot.idiom.pack.products.add27 = add i32 %fuzz.cfg.bytedot.idiom.pack.products.add, %fuzz.cfg.bytedot.idiom.pack.products.shift
  %fuzz.cfg.bytedot.idiom.pack.products.mask28 = and i32 %fuzz.cfg.bytedot.idiom.mul21, 255
  %fuzz.cfg.bytedot.idiom.pack.products.shift29 = shl i32 %fuzz.cfg.bytedot.idiom.pack.products.mask28, 16
  %fuzz.cfg.bytedot.idiom.pack.products.add30 = add i32 %fuzz.cfg.bytedot.idiom.pack.products.add27, %fuzz.cfg.bytedot.idiom.pack.products.shift29
  %fuzz.cfg.bytedot.idiom.pack.products.mask31 = and i32 %fuzz.cfg.bytedot.idiom.mul25, 255
  %fuzz.cfg.bytedot.idiom.pack.products.shift32 = shl i32 %fuzz.cfg.bytedot.idiom.pack.products.mask31, 24
  %fuzz.cfg.bytedot.idiom.pack.products.add33 = add i32 %fuzz.cfg.bytedot.idiom.pack.products.add30, %fuzz.cfg.bytedot.idiom.pack.products.shift32
  %fuzz.cfg.bytedot.idiom.result.add = add i32 %fuzz.cfg.bytedot.idiom.acc.sub, %fuzz.cfg.bytedot.idiom.pack.products.add33
  %fuzz.cfg.narrow.trunc.a = trunc i32 %fuzz.cfg.bytedot.idiom.result.add to i16
  %fuzz.cfg.narrow.or = or i16 %fuzz.cfg.narrow.trunc.a, -1
  %fuzz.cfg.narrow.zext = zext i16 %fuzz.cfg.narrow.or to i32
  %fuzz.cfg.narrow.xor.i32 = xor i32 %fuzz.cfg.narrow.zext, %fuzz.cfg.bytedot.idiom.result.add
  %fuzz.vec.narrow.trunc = trunc i32 %fuzz.cfg.narrow.xor.i32 to i8
  %fuzz.vec.narrow.trunc34 = trunc i32 %wg to i8
  %fuzz.vec.narrow.trunc35 = trunc i32 %wg to i8
  %fuzz.vec.narrow.trunc36 = trunc i32 65535 to i8
  %fuzz.vec.narrow.trunc37 = trunc i32 %fuzz.cfg.narrow.xor.i32 to i8
  %fuzz.vec.narrow.trunc38 = trunc i32 %wg to i8
  %fuzz.vec.narrow.trunc39 = trunc i32 %wg to i8
  %fuzz.vec.narrow.trunc40 = trunc i32 1431655765 to i8
  %fuzz.vec.narrow.trunc41 = trunc i32 %fuzz.cfg.narrow.xor.i32 to i8
  %fuzz.vec.narrow.trunc42 = trunc i32 %wg to i8
  %fuzz.vec.narrow.trunc43 = trunc i32 %wg to i8
  %fuzz.vec.narrow.trunc44 = trunc i32 2 to i8
  %fuzz.vec.narrow.trunc45 = trunc i32 %fuzz.cfg.narrow.xor.i32 to i8
  %fuzz.vec.narrow.trunc46 = trunc i32 %wg to i8
  %fuzz.vec.narrow.trunc47 = trunc i32 %wg to i8
  %fuzz.vec.narrow.trunc48 = trunc i32 %fuzz.cfg.narrow.xor.i32 to i8
  %fuzz.vec.ins = insertelement <8 x i8> zeroinitializer, i8 %fuzz.vec.narrow.trunc, i32 0
  %fuzz.vec.ins49 = insertelement <8 x i8> %fuzz.vec.ins, i8 %fuzz.vec.narrow.trunc35, i32 1
  %fuzz.vec.ins50 = insertelement <8 x i8> %fuzz.vec.ins49, i8 %fuzz.vec.narrow.trunc37, i32 2
  %fuzz.vec.ins51 = insertelement <8 x i8> %fuzz.vec.ins50, i8 %fuzz.vec.narrow.trunc39, i32 3
  %fuzz.vec.ins52 = insertelement <8 x i8> %fuzz.vec.ins51, i8 %fuzz.vec.narrow.trunc41, i32 4
  %fuzz.vec.ins53 = insertelement <8 x i8> %fuzz.vec.ins52, i8 %fuzz.vec.narrow.trunc43, i32 5
  %fuzz.vec.ins54 = insertelement <8 x i8> %fuzz.vec.ins53, i8 %fuzz.vec.narrow.trunc45, i32 6
  %fuzz.vec.ins55 = insertelement <8 x i8> %fuzz.vec.ins54, i8 %fuzz.vec.narrow.trunc47, i32 7
  %fuzz.vec.ins56 = insertelement <8 x i8> zeroinitializer, i8 %fuzz.vec.narrow.trunc34, i32 0
  %fuzz.vec.ins57 = insertelement <8 x i8> %fuzz.vec.ins56, i8 %fuzz.vec.narrow.trunc36, i32 1
  %fuzz.vec.ins58 = insertelement <8 x i8> %fuzz.vec.ins57, i8 %fuzz.vec.narrow.trunc38, i32 2
  %fuzz.vec.ins59 = insertelement <8 x i8> %fuzz.vec.ins58, i8 %fuzz.vec.narrow.trunc40, i32 3
  %fuzz.vec.ins60 = insertelement <8 x i8> %fuzz.vec.ins59, i8 %fuzz.vec.narrow.trunc42, i32 4
  %fuzz.vec.ins61 = insertelement <8 x i8> %fuzz.vec.ins60, i8 %fuzz.vec.narrow.trunc44, i32 5
  %fuzz.vec.ins62 = insertelement <8 x i8> %fuzz.vec.ins61, i8 %fuzz.vec.narrow.trunc46, i32 6
  %fuzz.vec.ins63 = insertelement <8 x i8> %fuzz.vec.ins62, i8 %fuzz.vec.narrow.trunc48, i32 7
  %fuzz.vec.narrow.shl = shl <8 x i8> %fuzz.vec.ins55, <i8 3, i8 5, i8 2, i8 5, i8 0, i8 0, i8 0, i8 5>
  %fuzz.vec.narrow.ext = extractelement <8 x i8> %fuzz.vec.narrow.shl, i32 1
  %fuzz.vec.narrow.ext64 = extractelement <8 x i8> %fuzz.vec.narrow.shl, i32 3
  %fuzz.vec.narrow.sext = sext i8 %fuzz.vec.narrow.ext to i32
  %fuzz.vec.narrow.zext = zext i8 %fuzz.vec.narrow.ext64 to i32
  %fuzz.vec.narrow.reduce.xor = xor i32 %fuzz.vec.narrow.sext, %fuzz.vec.narrow.zext
  %fuzz.loop.acc.inner.mix65 = xor i32 %fuzz.vec.narrow.reduce.xor, %fuzz.loop.iv.inner10
  %fuzz.loop.next.inner66 = add i32 %fuzz.loop.iv.inner10, 1
  br label %fuzz.nested.loop.header7

fuzz.nested.loop.exit9:                           ; preds = %fuzz.nested.loop.header7
  %fuzz.loop.nest.next67 = add i32 %fuzz.loop.nest.iv4, 1
  br label %fuzz.loop.nest.header2

fuzz.loop.nest.exit1:                             ; preds = %fuzz.loop.nest.header2
  %fuzz.soverflow.idiom.add.sel.sadd = add i32 %fuzz.loop.nest.acc5, 1
  %fuzz.soverflow.idiom.add.sel.sadd.abxor = xor i32 %fuzz.loop.nest.acc5, 1
  %fuzz.soverflow.idiom.add.sel.sadd.same = xor i32 %fuzz.soverflow.idiom.add.sel.sadd.abxor, -1
  %fuzz.soverflow.idiom.add.sel.sadd.flip = xor i32 %fuzz.loop.nest.acc5, %fuzz.soverflow.idiom.add.sel.sadd
  %fuzz.soverflow.idiom.add.sel.sadd.ovbits = and i32 %fuzz.soverflow.idiom.add.sel.sadd.same, %fuzz.soverflow.idiom.add.sel.sadd.flip
  %fuzz.soverflow.idiom.add.sel.sadd.ov = icmp slt i32 %fuzz.soverflow.idiom.add.sel.sadd.ovbits, 0
  %fuzz.soverflow.idiom.add.sel.sadd.neg = icmp slt i32 %fuzz.loop.nest.acc5, 0
  %fuzz.soverflow.idiom.sadd.select = select i1 %fuzz.soverflow.idiom.add.sel.sadd.ov, i32 1, i32 %fuzz.soverflow.idiom.add.sel.sadd
  store i32 %fuzz.soverflow.idiom.sadd.select, ptr addrspace(1) %out.ptr, align 4
  ret void
}

; Function Attrs: nocallback nofree nosync nounwind speculatable willreturn memory(none)
declare noundef i32 @llvm.amdgcn.workgroup.id.x() #1

; Function Attrs: nocallback nofree nosync nounwind speculatable willreturn memory(none)
declare noundef range(i32 0, 1024) i32 @llvm.amdgcn.workitem.id.x() #1

; Function Attrs: nocallback nofree nosync nounwind speculatable willreturn memory(none)
declare i32 @llvm.uadd.sat.i32(i32, i32) #2

; Function Attrs: nocallback nofree nosync nounwind speculatable willreturn memory(none)
declare { i32, i1 } @llvm.umul.with.overflow.i32(i32, i32) #2

; Function Attrs: nocallback nofree nosync nounwind speculatable willreturn memory(none)
declare { i32, i1 } @llvm.smul.with.overflow.i32(i32, i32) #2

attributes #0 = { convergent nounwind "amdgpu-flat-work-group-size"="1,256" "target-cpu"="gfx950" "uniform-work-group-size" }
attributes #1 = { nocallback nofree nosync nounwind speculatable willreturn memory(none) }
attributes #2 = { nocallback nofree nosync nounwind speculatable willreturn memory(none) }
