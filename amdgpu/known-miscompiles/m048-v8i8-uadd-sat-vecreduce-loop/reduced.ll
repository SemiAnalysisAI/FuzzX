; RUN-INPUTS: 0x00000000*256
; RUN-LLVM-BUILD: build/llvm-fuzzer
; ModuleID = '/tmp/fuzzx-reduce-m048-1779160354/reduced.bc'
source_filename = "/tmp/fuzzx-amdgpu-orenamd@semianalysis.com/head-pr198373-196418-198412-198419-m047-20260519-030439/corpus/directed-gpu/shared/.seed-3313078.ll"
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
  %fuzz.clamppack.idiom.u8.src.trunc = trunc i32 %mix to i8
  %fuzz.clamppack.idiom.u8.src.zext = zext i8 %fuzz.clamppack.idiom.u8.src.trunc to i32
  %fuzz.clamppack.idiom.u8.clamp.below = icmp ult i32 %fuzz.clamppack.idiom.u8.src.zext, 0
  %fuzz.clamppack.idiom.u8.clamp.atleast = select i1 %fuzz.clamppack.idiom.u8.clamp.below, i32 0, i32 %fuzz.clamppack.idiom.u8.src.zext
  %fuzz.clamppack.idiom.u8.clamp.above = icmp ugt i32 %fuzz.clamppack.idiom.u8.clamp.atleast, 31
  %fuzz.clamppack.idiom.u8.clamp.clamp = select i1 %fuzz.clamppack.idiom.u8.clamp.above, i32 31, i32 %fuzz.clamppack.idiom.u8.clamp.atleast
  %fuzz.clamppack.idiom.u8.src.shr = lshr i32 %mix, 8
  %fuzz.clamppack.idiom.u8.src.trunc1 = trunc i32 %fuzz.clamppack.idiom.u8.src.shr to i8
  %fuzz.clamppack.idiom.u8.src.zext2 = zext i8 %fuzz.clamppack.idiom.u8.src.trunc1 to i32
  %fuzz.clamppack.idiom.u8.clamp.below3 = icmp ult i32 %fuzz.clamppack.idiom.u8.src.zext2, 0
  %fuzz.clamppack.idiom.u8.clamp.atleast4 = select i1 %fuzz.clamppack.idiom.u8.clamp.below3, i32 0, i32 %fuzz.clamppack.idiom.u8.src.zext2
  %fuzz.clamppack.idiom.u8.clamp.above5 = icmp ugt i32 %fuzz.clamppack.idiom.u8.clamp.atleast4, 127
  %fuzz.clamppack.idiom.u8.clamp.clamp6 = select i1 %fuzz.clamppack.idiom.u8.clamp.above5, i32 1431655765, i32 %fuzz.clamppack.idiom.u8.clamp.atleast4
  %fuzz.clamppack.idiom.u8.src.shr7 = lshr i32 1431655765, 16
  %fuzz.clamppack.idiom.u8.src.trunc8 = trunc i32 %fuzz.clamppack.idiom.u8.src.shr7 to i8
  %fuzz.clamppack.idiom.u8.src.zext9 = zext i8 %fuzz.clamppack.idiom.u8.src.trunc8 to i32
  %fuzz.clamppack.idiom.u8.clamp.below10 = icmp ult i32 %fuzz.clamppack.idiom.u8.src.zext9, 0
  %fuzz.clamppack.idiom.u8.clamp.atleast11 = select i1 %fuzz.clamppack.idiom.u8.clamp.below10, i32 0, i32 %fuzz.clamppack.idiom.u8.src.zext9
  %fuzz.clamppack.idiom.u8.clamp.above12 = icmp ugt i32 %fuzz.clamppack.idiom.u8.clamp.atleast11, 255
  %fuzz.clamppack.idiom.u8.clamp.clamp13 = select i1 %fuzz.clamppack.idiom.u8.clamp.above12, i32 255, i32 %fuzz.clamppack.idiom.u8.clamp.atleast11
  %fuzz.clamppack.idiom.u8.src.shr14 = lshr i32 1431655765, 24
  %fuzz.clamppack.idiom.u8.src.trunc15 = trunc i32 %fuzz.clamppack.idiom.u8.src.shr14 to i8
  %fuzz.clamppack.idiom.u8.src.zext16 = zext i8 %fuzz.clamppack.idiom.u8.src.trunc15 to i32
  %fuzz.clamppack.idiom.u8.clamp.below17 = icmp ult i32 %fuzz.clamppack.idiom.u8.src.zext16, 0
  %fuzz.clamppack.idiom.u8.clamp.atleast18 = select i1 %fuzz.clamppack.idiom.u8.clamp.below17, i32 0, i32 %fuzz.clamppack.idiom.u8.src.zext16
  %fuzz.clamppack.idiom.u8.clamp.above19 = icmp ugt i32 %fuzz.clamppack.idiom.u8.clamp.atleast18, 15
  %fuzz.clamppack.idiom.u8.clamp.clamp20 = select i1 %fuzz.clamppack.idiom.u8.clamp.above19, i32 15, i32 %fuzz.clamppack.idiom.u8.clamp.atleast18
  %fuzz.clamppack.idiom.u8.pack.mask = and i32 %fuzz.clamppack.idiom.u8.clamp.clamp, 255
  %fuzz.clamppack.idiom.u8.pack.add = add i32 0, %fuzz.clamppack.idiom.u8.pack.mask
  %fuzz.clamppack.idiom.u8.pack.mask21 = and i32 %fuzz.clamppack.idiom.u8.clamp.clamp6, 255
  %fuzz.clamppack.idiom.u8.pack.shift = shl i32 %fuzz.clamppack.idiom.u8.pack.mask21, 8
  %fuzz.clamppack.idiom.u8.pack.add22 = add i32 %fuzz.clamppack.idiom.u8.pack.add, %fuzz.clamppack.idiom.u8.pack.shift
  %fuzz.clamppack.idiom.u8.pack.mask23 = and i32 %fuzz.clamppack.idiom.u8.clamp.clamp13, 255
  %fuzz.clamppack.idiom.u8.pack.shift24 = shl i32 %fuzz.clamppack.idiom.u8.pack.mask23, 16
  %fuzz.clamppack.idiom.u8.pack.add25 = add i32 %fuzz.clamppack.idiom.u8.pack.add22, %fuzz.clamppack.idiom.u8.pack.shift24
  %fuzz.clamppack.idiom.u8.pack.mask26 = and i32 %fuzz.clamppack.idiom.u8.clamp.clamp20, 255
  %fuzz.clamppack.idiom.u8.pack.shift27 = shl i32 %fuzz.clamppack.idiom.u8.pack.mask26, 24
  %fuzz.clamppack.idiom.u8.pack.add28 = add i32 %fuzz.clamppack.idiom.u8.pack.add25, %fuzz.clamppack.idiom.u8.pack.shift27
  %fuzz.packunpack.idiom.a0.trunc = trunc i32 %fuzz.clamppack.idiom.u8.pack.add28 to i8
  %fuzz.packunpack.idiom.a0.zext = zext i8 %fuzz.packunpack.idiom.a0.trunc to i32
  %fuzz.packunpack.idiom.a2.shr = lshr i32 %fuzz.clamppack.idiom.u8.pack.add28, 16
  %fuzz.packunpack.idiom.a2.trunc = trunc i32 %fuzz.packunpack.idiom.a2.shr to i8
  %fuzz.packunpack.idiom.a2.zext = zext i8 %fuzz.packunpack.idiom.a2.trunc to i32
  %fuzz.packunpack.idiom.b1.shr = lshr i32 %fuzz.clamppack.idiom.u8.src.shr, 8
  %fuzz.packunpack.idiom.b1.trunc = trunc i32 %fuzz.packunpack.idiom.b1.shr to i8
  %fuzz.packunpack.idiom.b1.zext = zext i8 %fuzz.packunpack.idiom.b1.trunc to i32
  %fuzz.packunpack.idiom.b3.shr = lshr i32 %fuzz.clamppack.idiom.u8.src.shr, 24
  %fuzz.packunpack.idiom.b3.trunc = trunc i32 %fuzz.packunpack.idiom.b3.shr to i8
  %fuzz.packunpack.idiom.b3.zext = zext i8 %fuzz.packunpack.idiom.b3.trunc to i32
  %fuzz.packunpack.idiom.pack.mask = and i32 %fuzz.packunpack.idiom.b3.zext, 255
  %fuzz.packunpack.idiom.pack.or = or i32 0, %fuzz.packunpack.idiom.pack.mask
  %fuzz.packunpack.idiom.pack.mask1 = and i32 %fuzz.packunpack.idiom.a0.zext, 255
  %fuzz.packunpack.idiom.pack.shift = shl i32 %fuzz.packunpack.idiom.pack.mask1, 8
  %fuzz.packunpack.idiom.pack.or2 = or i32 %fuzz.packunpack.idiom.pack.or, %fuzz.packunpack.idiom.pack.shift
  %fuzz.packunpack.idiom.pack.mask3 = and i32 %fuzz.packunpack.idiom.b1.zext, 255
  %fuzz.packunpack.idiom.pack.shift4 = shl i32 %fuzz.packunpack.idiom.pack.mask3, 16
  %fuzz.packunpack.idiom.pack.or5 = or i32 %fuzz.packunpack.idiom.pack.or2, %fuzz.packunpack.idiom.pack.shift4
  %fuzz.packunpack.idiom.pack.mask6 = and i32 %fuzz.packunpack.idiom.a2.zext, 1
  %fuzz.packunpack.idiom.pack.shift7 = shl i32 %fuzz.packunpack.idiom.pack.mask6, 24
  %fuzz.packunpack.idiom.pack.or8 = or i32 %fuzz.packunpack.idiom.pack.or5, %fuzz.packunpack.idiom.pack.shift7
  %fuzz.packunpack.idiom.half.shr = lshr i32 %fuzz.packunpack.idiom.pack.or8, 16
  %fuzz.packunpack.idiom.half.trunc = trunc i32 %fuzz.packunpack.idiom.half.shr to i16
  %fuzz.packunpack.idiom.half.zext = zext i16 %fuzz.packunpack.idiom.half.trunc to i32
  %fuzz.packunpack.idiom.half.xor = xor i32 %fuzz.packunpack.idiom.half.zext, %fuzz.clamppack.idiom.u8.pack.add28
  %fuzz.bitfield.idiom.shift = and i32 -2147483648, 15
  %fuzz.bitfield.idiom.width.m1 = and i32 -2147483648, 15
  %fuzz.bitfield.idiom.width = add i32 %fuzz.bitfield.idiom.width.m1, 1
  %fuzz.bitfield.idiom.invwidth.raw = sub i32 32, %fuzz.bitfield.idiom.width
  %fuzz.bitfield.idiom.invwidth = and i32 %fuzz.bitfield.idiom.invwidth.raw, 31
  %fuzz.bitfield.idiom.mask = lshr i32 -1, %fuzz.bitfield.idiom.invwidth
  %fuzz.bitfield.idiom.shifted = lshr i32 %fuzz.packunpack.idiom.half.xor, %fuzz.bitfield.idiom.shift
  %fuzz.bitfield.idiom.extracted = and i32 %fuzz.bitfield.idiom.shifted, %fuzz.bitfield.idiom.mask
  %fuzz.bitfield.idiom.fieldmask = shl i32 %fuzz.bitfield.idiom.mask, %fuzz.bitfield.idiom.shift
  %fuzz.bitfield.idiom.notfieldmask = xor i32 %fuzz.bitfield.idiom.fieldmask, -1
  %fuzz.bitfield.idiom.clear = and i32 %fuzz.packunpack.idiom.half.xor, %fuzz.bitfield.idiom.notfieldmask
  %fuzz.bitfield.idiom.payload.masked = and i32 -2147483648, %fuzz.bitfield.idiom.mask
  %fuzz.bitfield.idiom.payload.shifted = shl i32 %fuzz.bitfield.idiom.payload.masked, %fuzz.bitfield.idiom.shift
  %fuzz.bitfield.idiom.insert = or i32 %fuzz.bitfield.idiom.clear, %fuzz.bitfield.idiom.payload.shifted
  br label %fuzz.loop.nest.header

fuzz.loop.nest.header:                            ; preds = %fuzz.nested.loop.exit, %entry
  %fuzz.loop.nest.iv = phi i32 [ 0, %entry ], [ %fuzz.loop.nest.next, %fuzz.nested.loop.exit ]
  %fuzz.loop.nest.acc = phi i32 [ %fuzz.bitfield.idiom.insert, %entry ], [ %fuzz.loop.acc.inner, %fuzz.nested.loop.exit ]
  %fuzz.loop.nest.cond = icmp ult i32 %fuzz.loop.nest.iv, 2
  br i1 %fuzz.loop.nest.cond, label %fuzz.loop.nest.body, label %fuzz.loop.nest.exit

fuzz.loop.nest.body:                              ; preds = %fuzz.loop.nest.header
  %fuzz.loop.trip.inner.mask = and i32 %fuzz.loop.nest.acc, 3
  %fuzz.loop.trip.inner = add i32 %fuzz.loop.trip.inner.mask, 1
  br label %fuzz.nested.loop.header

fuzz.nested.loop.header:                          ; preds = %fuzz.nested.loop.body, %fuzz.loop.nest.body
  %fuzz.loop.iv.inner = phi i32 [ 0, %fuzz.loop.nest.body ], [ %fuzz.loop.next.inner, %fuzz.nested.loop.body ]
  %fuzz.loop.acc.inner = phi i32 [ %fuzz.loop.nest.acc, %fuzz.loop.nest.body ], [ %fuzz.loop.acc.inner.mix, %fuzz.nested.loop.body ]
  %fuzz.loop.cond.inner = icmp ult i32 %fuzz.loop.iv.inner, %fuzz.loop.trip.inner
  br i1 %fuzz.loop.cond.inner, label %fuzz.nested.loop.body, label %fuzz.nested.loop.exit

fuzz.nested.loop.body:                            ; preds = %fuzz.nested.loop.header
  %fuzz.vec.narrow.trunc = trunc i32 %fuzz.loop.acc.inner to i8
  %fuzz.vec.narrow.trunc1 = trunc i32 -2147483648 to i8
  %fuzz.vec.narrow.trunc2 = trunc i32 -2147483648 to i8
  %fuzz.vec.narrow.trunc3 = trunc i32 %fuzz.loop.acc.inner to i8
  %fuzz.vec.narrow.trunc4 = trunc i32 %fuzz.loop.acc.inner to i8
  %fuzz.vec.narrow.trunc5 = trunc i32 2 to i8
  %fuzz.vec.narrow.trunc6 = trunc i32 -2147483648 to i8
  %fuzz.vec.narrow.trunc7 = trunc i32 1107317678 to i8
  %fuzz.vec.narrow.trunc8 = trunc i32 %fuzz.loop.acc.inner to i8
  %fuzz.vec.narrow.trunc9 = trunc i32 -2147483648 to i8
  %fuzz.vec.narrow.trunc10 = trunc i32 -2147483648 to i8
  %fuzz.vec.narrow.trunc11 = trunc i32 %fuzz.loop.acc.inner to i8
  %fuzz.vec.narrow.trunc12 = trunc i32 %fuzz.loop.acc.inner to i8
  %fuzz.vec.narrow.trunc13 = trunc i32 -2147483648 to i8
  %fuzz.vec.narrow.trunc14 = trunc i32 -2147483648 to i8
  %fuzz.vec.narrow.trunc15 = trunc i32 %fuzz.loop.acc.inner to i8
  %fuzz.vec.ins = insertelement <8 x i8> zeroinitializer, i8 %fuzz.vec.narrow.trunc, i32 0
  %fuzz.vec.ins16 = insertelement <8 x i8> %fuzz.vec.ins, i8 %fuzz.vec.narrow.trunc2, i32 1
  %fuzz.vec.ins17 = insertelement <8 x i8> %fuzz.vec.ins16, i8 %fuzz.vec.narrow.trunc4, i32 2
  %fuzz.vec.ins18 = insertelement <8 x i8> %fuzz.vec.ins17, i8 %fuzz.vec.narrow.trunc6, i32 3
  %fuzz.vec.ins19 = insertelement <8 x i8> %fuzz.vec.ins18, i8 %fuzz.vec.narrow.trunc8, i32 4
  %fuzz.vec.ins20 = insertelement <8 x i8> %fuzz.vec.ins19, i8 %fuzz.vec.narrow.trunc10, i32 5
  %fuzz.vec.ins21 = insertelement <8 x i8> %fuzz.vec.ins20, i8 %fuzz.vec.narrow.trunc12, i32 6
  %fuzz.vec.ins22 = insertelement <8 x i8> %fuzz.vec.ins21, i8 %fuzz.vec.narrow.trunc14, i32 7
  %fuzz.vec.ins23 = insertelement <8 x i8> zeroinitializer, i8 %fuzz.vec.narrow.trunc1, i32 0
  %fuzz.vec.ins24 = insertelement <8 x i8> %fuzz.vec.ins23, i8 %fuzz.vec.narrow.trunc3, i32 1
  %fuzz.vec.ins25 = insertelement <8 x i8> %fuzz.vec.ins24, i8 %fuzz.vec.narrow.trunc5, i32 2
  %fuzz.vec.ins26 = insertelement <8 x i8> %fuzz.vec.ins25, i8 %fuzz.vec.narrow.trunc7, i32 3
  %fuzz.vec.ins27 = insertelement <8 x i8> %fuzz.vec.ins26, i8 %fuzz.vec.narrow.trunc9, i32 4
  %fuzz.vec.ins28 = insertelement <8 x i8> %fuzz.vec.ins27, i8 %fuzz.vec.narrow.trunc11, i32 5
  %fuzz.vec.ins29 = insertelement <8 x i8> %fuzz.vec.ins28, i8 %fuzz.vec.narrow.trunc13, i32 6
  %fuzz.vec.ins30 = insertelement <8 x i8> %fuzz.vec.ins29, i8 %fuzz.vec.narrow.trunc15, i32 7
  %fuzz.vec.narrow.binary = call <8 x i8> @llvm.uadd.sat.v8i8(<8 x i8> %fuzz.vec.ins22, <8 x i8> %fuzz.vec.ins30)
  %fuzz.vec.narrow.ext = extractelement <8 x i8> %fuzz.vec.narrow.binary, i32 4
  %fuzz.vec.narrow.ext31 = extractelement <8 x i8> %fuzz.vec.narrow.binary, i32 3
  %fuzz.vec.narrow.zext = zext i8 %fuzz.vec.narrow.ext to i32
  %fuzz.vec.narrow.sext = sext i8 %fuzz.vec.narrow.ext31 to i32
  %fuzz.vec.narrow.reduce.or = or i32 %fuzz.vec.narrow.zext, %fuzz.vec.narrow.sext
  %fuzz.cfg.usub_sat = call i32 @llvm.usub.sat.i32(i32 %fuzz.vec.narrow.reduce.or, i32 7)
  %fuzz.cfg.bitcount.idiom.smear1.shr = lshr i32 %fuzz.cfg.usub_sat, 1
  %fuzz.cfg.bitcount.idiom.smear1 = or i32 %fuzz.cfg.usub_sat, %fuzz.cfg.bitcount.idiom.smear1.shr
  %fuzz.cfg.bitcount.idiom.smear2.shr = lshr i32 %fuzz.cfg.bitcount.idiom.smear1, 2
  %fuzz.cfg.bitcount.idiom.smear2 = or i32 %fuzz.cfg.bitcount.idiom.smear1, %fuzz.cfg.bitcount.idiom.smear2.shr
  %fuzz.cfg.bitcount.idiom.smear4.shr = lshr i32 %fuzz.cfg.bitcount.idiom.smear2, 4
  %fuzz.cfg.bitcount.idiom.smear4 = or i32 %fuzz.cfg.bitcount.idiom.smear2, %fuzz.cfg.bitcount.idiom.smear4.shr
  %fuzz.cfg.bitcount.idiom.smear8.shr = lshr i32 %fuzz.cfg.bitcount.idiom.smear4, 8
  %fuzz.cfg.bitcount.idiom.smear8 = or i32 %fuzz.cfg.bitcount.idiom.smear4, %fuzz.cfg.bitcount.idiom.smear8.shr
  %fuzz.cfg.bitcount.idiom.smear16.shr = lshr i32 %fuzz.cfg.bitcount.idiom.smear8, 16
  %fuzz.cfg.bitcount.idiom.smear16 = or i32 %fuzz.cfg.bitcount.idiom.smear8, %fuzz.cfg.bitcount.idiom.smear16.shr
  %fuzz.cfg.bitcount.idiom.smear.pop = call i32 @llvm.ctpop.i32(i32 %fuzz.cfg.bitcount.idiom.smear16)
  %fuzz.cfg.bitcount.idiom.smear.mix = sub i32 %fuzz.cfg.bitcount.idiom.smear16, %fuzz.cfg.bitcount.idiom.smear.pop
  %fuzz.cfg.vecreduce.idiom.a.xor = xor i32 %fuzz.cfg.bitcount.idiom.smear.mix, -910110681
  %fuzz.cfg.vecreduce.idiom.a.xor32 = xor i32 -910110681, %fuzz.cfg.bitcount.idiom.smear.mix
  %fuzz.vec.ins33 = insertelement <2 x i32> zeroinitializer, i32 %fuzz.cfg.vecreduce.idiom.a.xor, i32 0
  %fuzz.vec.ins34 = insertelement <2 x i32> %fuzz.vec.ins33, i32 %fuzz.cfg.vecreduce.idiom.a.xor32, i32 1
  %fuzz.vec.ins35 = insertelement <2 x i32> zeroinitializer, i32 -2147483648, i32 0
  %fuzz.vec.ins36 = insertelement <2 x i32> %fuzz.vec.ins35, i32 1, i32 1
  %fuzz.cfg.vecreduce.idiom.rot = shufflevector <2 x i32> %fuzz.vec.ins34, <2 x i32> %fuzz.vec.ins36, <2 x i32> <i32 1, i32 0>
  %fuzz.cfg.vecreduce.idiom.rev = shufflevector <2 x i32> %fuzz.vec.ins36, <2 x i32> %fuzz.vec.ins34, <2 x i32> <i32 1, i32 0>
  %fuzz.cfg.vecreduce.idiom.vxor = xor <2 x i32> %fuzz.vec.ins34, %fuzz.cfg.vecreduce.idiom.rev
  %fuzz.cfg.vecreduce.idiom.reduce.lane = extractelement <2 x i32> %fuzz.cfg.vecreduce.idiom.vxor, i32 0
  %fuzz.cfg.vecreduce.idiom.reduce.lane37 = extractelement <2 x i32> %fuzz.cfg.vecreduce.idiom.vxor, i32 1
  %fuzz.cfg.vecreduce.idiom.reduce.and = and i32 %fuzz.cfg.vecreduce.idiom.reduce.lane, %fuzz.cfg.vecreduce.idiom.reduce.lane37
  %fuzz.loop.acc.inner.mix = xor i32 %fuzz.cfg.vecreduce.idiom.reduce.and, %fuzz.loop.iv.inner
  %fuzz.loop.next.inner = add i32 %fuzz.loop.iv.inner, 1
  br label %fuzz.nested.loop.header

fuzz.nested.loop.exit:                            ; preds = %fuzz.nested.loop.header
  %fuzz.loop.nest.next = add i32 %fuzz.loop.nest.iv, 1
  br label %fuzz.loop.nest.header

fuzz.loop.nest.exit:                              ; preds = %fuzz.loop.nest.header
  store i32 %fuzz.loop.nest.acc, ptr addrspace(1) %out.ptr, align 4
  ret void
}

; Function Attrs: nocallback nofree nosync nounwind speculatable willreturn memory(none)
declare noundef i32 @llvm.amdgcn.workgroup.id.x() #1

; Function Attrs: nocallback nofree nosync nounwind speculatable willreturn memory(none)
declare noundef range(i32 0, 1024) i32 @llvm.amdgcn.workitem.id.x() #1

; Function Attrs: nocallback nofree nosync nounwind speculatable willreturn memory(none)
declare <8 x i8> @llvm.uadd.sat.v8i8(<8 x i8>, <8 x i8>) #2

; Function Attrs: nocallback nofree nosync nounwind speculatable willreturn memory(none)
declare i32 @llvm.usub.sat.i32(i32, i32) #2

; Function Attrs: nocallback nofree nosync nounwind speculatable willreturn memory(none)
declare i32 @llvm.ctpop.i32(i32) #2

attributes #0 = { convergent nounwind "amdgpu-flat-work-group-size"="1,256" "target-cpu"="gfx950" "uniform-work-group-size" }
attributes #1 = { nocallback nofree nosync nounwind speculatable willreturn memory(none) }
attributes #2 = { nocallback nofree nosync nounwind speculatable willreturn memory(none) }
