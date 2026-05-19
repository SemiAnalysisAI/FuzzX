; RUN-INPUTS: 0x0,0x1,0x7fffffff,0x0*253
; RUN-LLVM-BUILD: build/llvm-fuzzer
source_filename = "known-miscompiles/m056-halfdot-lowbit-branch/reduced.ll"
target datalayout = "e-m:e-p:64:64-p1:64:64-p2:32:32-p3:32:32-p4:64:64-p5:32:32-p6:32:32-p7:160:256:256:32-p8:128:128:128:48-p9:192:256:256:32-i64:64-v16:16-v24:32-v32:32-v48:64-v96:128-v192:256-v256:256-v512:512-v1024:1024-v2048:2048-n32:64-S32-A5-G1-ni:7:8:9"
target triple = "amdgcn-amd-amdhsa"

; Function Attrs: convergent nounwind
define amdgpu_kernel void @fuzz_kernel(ptr addrspace(1) %in, ptr addrspace(1) %out) #0 {
entry:
  %wg = call i32 @llvm.amdgcn.workgroup.id.x()
  %wi = call i32 @llvm.amdgcn.workitem.id.x()
  %block.base = mul i32 %wg, 256
  %idx = add i32 %block.base, %wi
  %idx64 = zext i32 %idx to i64
  %in.ptr = getelementptr i32, ptr addrspace(1) %in, i64 %idx64
  %v = load i32, ptr addrspace(1) %in.ptr, align 4
  %salt = mul i32 %idx, -1640531527
  %mix = xor i32 %v, %salt
  %out.ptr = getelementptr i32, ptr addrspace(1) %out, i64 %idx64
  %fuzz.soverflow.idiom.sub.ssub = sub i32 %mix, %idx
  %fuzz.soverflow.idiom.sub.ssub.abxor = xor i32 %mix, %idx
  %fuzz.soverflow.idiom.sub.ssub.flip = xor i32 %mix, %fuzz.soverflow.idiom.sub.ssub
  %fuzz.soverflow.idiom.sub.ssub.ovbits = and i32 %fuzz.soverflow.idiom.sub.ssub.abxor, %fuzz.soverflow.idiom.sub.ssub.flip
  %fuzz.soverflow.idiom.sub.ssub.ov = icmp slt i32 %fuzz.soverflow.idiom.sub.ssub.ovbits, 0
  %fuzz.soverflow.idiom.sub.ssub.neg = icmp slt i32 %mix, 0
  %fuzz.soverflow.idiom.ssub.sat.value = select i1 %fuzz.soverflow.idiom.sub.ssub.neg, i32 -2147483648, i32 2147483647
  %fuzz.soverflow.idiom.ssub.sat = select i1 %fuzz.soverflow.idiom.sub.ssub.ov, i32 %fuzz.soverflow.idiom.ssub.sat.value, i32 %fuzz.soverflow.idiom.sub.ssub
  %fuzz.ssub_sat = call i32 @llvm.ssub.sat.i32(i32 %fuzz.soverflow.idiom.ssub.sat, i32 %block.base)
  %fuzz.loop.multi.cond = icmp ult i32 0, 5
  %fuzz.cfg.ternary.idiom.not.b = xor i32 %fuzz.soverflow.idiom.sub.ssub.abxor, -1
  %fuzz.cfg.ternary.idiom.ab = xor i32 %fuzz.ssub_sat, %fuzz.soverflow.idiom.sub.ssub.abxor
  %fuzz.cfg.ternary.idiom.bc = or i32 %fuzz.soverflow.idiom.sub.ssub.abxor, 1687688493
  %fuzz.cfg.ternary.idiom.ca = and i32 1687688493, %fuzz.ssub_sat
  %fuzz.cfg.ternary.idiom.mix0 = xor i32 %fuzz.cfg.ternary.idiom.ab, %fuzz.cfg.ternary.idiom.bc
  %fuzz.cfg.ternary.idiom.mix = add i32 %fuzz.cfg.ternary.idiom.mix0, %fuzz.cfg.ternary.idiom.ca
  %fuzz.loop.multi.break.a.val = xor i32 %fuzz.cfg.ternary.idiom.mix, 0
  %fuzz.bool.cmp0 = icmp eq i32 %fuzz.loop.multi.break.a.val, %wi
  %fuzz.bool.cmp1 = icmp sle i32 %wi, 2
  %fuzz.bool.not = xor i1 %fuzz.bool.cmp0, true
  %fuzz.bool.zext = zext i1 %fuzz.bool.not to i32
  %fuzz.loop.multi.cond9 = icmp ult i32 0, 1
  %fuzz.cfg.ternary.idiom.not.a = xor i32 %fuzz.bool.zext, -1
  %fuzz.cfg.ternary.idiom.not.b10 = xor i32 %v, -1
  %fuzz.cfg.ternary.idiom.not.c = xor i32 65535, -1
  %fuzz.cfg.ternary.idiom.blend.mask = xor i32 %fuzz.bool.zext, %v
  %fuzz.cfg.ternary.idiom.blend.left = and i32 65535, %fuzz.cfg.ternary.idiom.blend.mask
  %fuzz.cfg.ternary.idiom.blend.not = xor i32 %fuzz.cfg.ternary.idiom.blend.mask, -1
  %fuzz.cfg.ternary.idiom.blend.right = and i32 %v, %fuzz.cfg.ternary.idiom.blend.not
  %fuzz.cfg.ternary.idiom.blend = or i32 %fuzz.cfg.ternary.idiom.blend.left, %fuzz.cfg.ternary.idiom.blend.right
  %fuzz.cfg.limb.idiom.a0 = and i32 %fuzz.cfg.ternary.idiom.blend, 65535
  %fuzz.cfg.limb.idiom.a1 = lshr i32 %fuzz.cfg.ternary.idiom.blend, 16
  %fuzz.cfg.limb.idiom.b0 = and i32 2147483647, 65535
  %fuzz.cfg.limb.idiom.b1 = lshr i32 2147483647, 16
  %fuzz.cfg.limb.idiom.a0.wide = zext i32 %fuzz.cfg.limb.idiom.a0 to i64
  %fuzz.cfg.limb.idiom.a1.wide = zext i32 %fuzz.cfg.limb.idiom.a1 to i64
  %fuzz.cfg.limb.idiom.b0.wide = zext i32 %fuzz.cfg.limb.idiom.b0 to i64
  %fuzz.cfg.limb.idiom.b1.wide = zext i32 %fuzz.cfg.limb.idiom.b1 to i64
  %fuzz.cfg.limb.idiom.cross0 = mul i64 %fuzz.cfg.limb.idiom.a0.wide, %fuzz.cfg.limb.idiom.b1.wide
  %fuzz.cfg.limb.idiom.cross1 = mul i64 %fuzz.cfg.limb.idiom.a1.wide, %fuzz.cfg.limb.idiom.b0.wide
  %fuzz.cfg.limb.idiom.cross = add i64 %fuzz.cfg.limb.idiom.cross0, %fuzz.cfg.limb.idiom.cross1
  %fuzz.cfg.limb.idiom.cross.lo32 = trunc i64 %fuzz.cfg.limb.idiom.cross to i32
  %fuzz.cfg.limb.idiom.cross.shr32 = lshr i64 %fuzz.cfg.limb.idiom.cross, 32
  %fuzz.cfg.limb.idiom.cross.hi32 = trunc i64 %fuzz.cfg.limb.idiom.cross.shr32 to i32
  %fuzz.cfg.limb.idiom.cross.fold = xor i32 %fuzz.cfg.limb.idiom.cross.lo32, %fuzz.cfg.limb.idiom.cross.hi32
  %fuzz.cfg.vecreduce.idiom.a.shr = lshr i32 %fuzz.cfg.limb.idiom.cross.fold, 0
  %fuzz.cfg.vecreduce.idiom.b.shl = shl i32 2147483647, 1
  %fuzz.cfg.vecreduce.idiom.a.add = add i32 2147483647, 2
  %fuzz.cfg.vecreduce.idiom.b.xor = xor i32 %fuzz.cfg.limb.idiom.cross.fold, 538976288
  %fuzz.vec.ins = insertelement <2 x i32> zeroinitializer, i32 %fuzz.cfg.vecreduce.idiom.a.shr, i32 0
  %fuzz.vec.ins11 = insertelement <2 x i32> %fuzz.vec.ins, i32 %fuzz.cfg.vecreduce.idiom.a.add, i32 1
  %fuzz.vec.ins12 = insertelement <2 x i32> zeroinitializer, i32 %fuzz.cfg.vecreduce.idiom.b.shl, i32 0
  %fuzz.vec.ins13 = insertelement <2 x i32> %fuzz.vec.ins12, i32 %fuzz.cfg.vecreduce.idiom.b.xor, i32 1
  %fuzz.cfg.vecreduce.idiom.rot = shufflevector <2 x i32> %fuzz.vec.ins11, <2 x i32> %fuzz.vec.ins13, <2 x i32> <i32 0, i32 1>
  %fuzz.cfg.vecreduce.idiom.rev = shufflevector <2 x i32> %fuzz.vec.ins13, <2 x i32> %fuzz.vec.ins11, <2 x i32> <i32 1, i32 0>
  %fuzz.cfg.vecreduce.idiom.vsub = sub <2 x i32> %fuzz.cfg.vecreduce.idiom.rot, %fuzz.vec.ins11
  %fuzz.cfg.vecreduce.idiom.reduce.lane = extractelement <2 x i32> %fuzz.cfg.vecreduce.idiom.vsub, i32 0
  %fuzz.cfg.vecreduce.idiom.reduce.lane14 = extractelement <2 x i32> %fuzz.cfg.vecreduce.idiom.vsub, i32 1
  %fuzz.cfg.vecreduce.idiom.reduce.sub = sub i32 %fuzz.cfg.vecreduce.idiom.reduce.lane, %fuzz.cfg.vecreduce.idiom.reduce.lane14
  %fuzz.cfg.vecreduce.idiom.xor = xor i32 %fuzz.cfg.vecreduce.idiom.reduce.sub, %fuzz.cfg.limb.idiom.cross.fold
  %fuzz.cfg.halfdot.idiom.a.half.trunc = trunc i32 %v to i16
  %fuzz.cfg.halfdot.idiom.a.half.zext = zext i16 %fuzz.cfg.halfdot.idiom.a.half.trunc to i32
  %fuzz.cfg.halfdot.idiom.b.half.shr = lshr i32 %fuzz.cfg.vecreduce.idiom.xor, 16
  %fuzz.cfg.halfdot.idiom.b.half.trunc = trunc i32 %fuzz.cfg.halfdot.idiom.b.half.shr to i16
  %fuzz.cfg.halfdot.idiom.b.half.zext = zext i16 %fuzz.cfg.halfdot.idiom.b.half.trunc to i32
  %fuzz.cfg.halfdot.idiom.a.wide.u = zext i32 %fuzz.cfg.halfdot.idiom.a.half.zext to i64
  %fuzz.cfg.halfdot.idiom.b.wide.u = zext i32 %fuzz.cfg.halfdot.idiom.b.half.zext to i64
  %fuzz.cfg.halfdot.idiom.mul = mul i64 %fuzz.cfg.halfdot.idiom.a.wide.u, %fuzz.cfg.halfdot.idiom.b.wide.u
  %fuzz.cfg.halfdot.idiom.acc.sub = sub i64 33598, %fuzz.cfg.halfdot.idiom.mul
  %fuzz.cfg.halfdot.idiom.mul.i32 = trunc i64 %fuzz.cfg.halfdot.idiom.mul to i32
  %fuzz.cfg.halfdot.idiom.mul.byte = lshr i32 %fuzz.cfg.halfdot.idiom.mul.i32, 0
  %fuzz.cfg.halfdot.idiom.a.half.shr = lshr i32 %fuzz.cfg.vecreduce.idiom.xor, 16
  %fuzz.cfg.halfdot.idiom.a.half.trunc15 = trunc i32 %fuzz.cfg.halfdot.idiom.a.half.shr to i16
  %fuzz.cfg.halfdot.idiom.a.half.zext16 = zext i16 %fuzz.cfg.halfdot.idiom.a.half.trunc15 to i32
  %fuzz.cfg.halfdot.idiom.b.half.trunc17 = trunc i32 %v to i16
  %fuzz.cfg.halfdot.idiom.b.half.zext18 = zext i16 %fuzz.cfg.halfdot.idiom.b.half.trunc17 to i32
  %fuzz.cfg.halfdot.idiom.a.wide.u19 = zext i32 %fuzz.cfg.halfdot.idiom.a.half.zext16 to i64
  %fuzz.cfg.halfdot.idiom.b.wide.u20 = zext i32 %fuzz.cfg.halfdot.idiom.b.half.zext18 to i64
  %fuzz.cfg.halfdot.idiom.mul21 = mul i64 %fuzz.cfg.halfdot.idiom.a.wide.u19, %fuzz.cfg.halfdot.idiom.b.wide.u20
  %fuzz.cfg.halfdot.idiom.mul.lo = and i64 %fuzz.cfg.halfdot.idiom.mul21, 65535
  %fuzz.cfg.halfdot.idiom.acc.add.lo = add i64 %fuzz.cfg.halfdot.idiom.acc.sub, %fuzz.cfg.halfdot.idiom.mul.lo
  %fuzz.cfg.halfdot.idiom.mul.i3222 = trunc i64 %fuzz.cfg.halfdot.idiom.mul21 to i32
  %fuzz.cfg.halfdot.idiom.mul.byte23 = lshr i32 %fuzz.cfg.halfdot.idiom.mul.i3222, 8
  %fuzz.cfg.halfdot.idiom.a.half.trunc24 = trunc i32 %v to i16
  %fuzz.cfg.halfdot.idiom.a.half.zext25 = zext i16 %fuzz.cfg.halfdot.idiom.a.half.trunc24 to i32
  %fuzz.cfg.halfdot.idiom.b.half.shr26 = lshr i32 %fuzz.cfg.vecreduce.idiom.xor, 16
  %fuzz.cfg.halfdot.idiom.b.half.trunc27 = trunc i32 %fuzz.cfg.halfdot.idiom.b.half.shr26 to i16
  %fuzz.cfg.halfdot.idiom.b.half.zext28 = zext i16 %fuzz.cfg.halfdot.idiom.b.half.trunc27 to i32
  %fuzz.cfg.halfdot.idiom.a.wide.u29 = zext i32 %fuzz.cfg.halfdot.idiom.a.half.zext25 to i64
  %fuzz.cfg.halfdot.idiom.b.wide.u30 = zext i32 %fuzz.cfg.halfdot.idiom.b.half.zext28 to i64
  %fuzz.cfg.halfdot.idiom.mul31 = mul i64 %fuzz.cfg.halfdot.idiom.a.wide.u29, %fuzz.cfg.halfdot.idiom.b.wide.u30
  %fuzz.cfg.halfdot.idiom.acc.sub32 = sub i64 %fuzz.cfg.halfdot.idiom.acc.add.lo, %fuzz.cfg.halfdot.idiom.mul31
  %fuzz.cfg.halfdot.idiom.mul.i3233 = trunc i64 %fuzz.cfg.halfdot.idiom.mul31 to i32
  %fuzz.cfg.halfdot.idiom.mul.byte34 = lshr i32 %fuzz.cfg.halfdot.idiom.mul.i3233, 0
  %fuzz.cfg.halfdot.idiom.a.half.shr35 = lshr i32 %fuzz.cfg.vecreduce.idiom.xor, 16
  %fuzz.cfg.halfdot.idiom.a.half.trunc36 = trunc i32 %fuzz.cfg.halfdot.idiom.a.half.shr35 to i16
  %fuzz.cfg.halfdot.idiom.a.half.zext37 = zext i16 %fuzz.cfg.halfdot.idiom.a.half.trunc36 to i32
  %fuzz.cfg.halfdot.idiom.b.half.trunc38 = trunc i32 %v to i16
  %fuzz.cfg.halfdot.idiom.b.half.sext = sext i16 %fuzz.cfg.halfdot.idiom.b.half.trunc38 to i32
  %fuzz.cfg.halfdot.idiom.a.wide.u39 = zext i32 %fuzz.cfg.halfdot.idiom.a.half.zext37 to i64
  %fuzz.cfg.halfdot.idiom.b.wide.s = sext i32 %fuzz.cfg.halfdot.idiom.b.half.sext to i64
  %fuzz.cfg.halfdot.idiom.mul40 = mul i64 %fuzz.cfg.halfdot.idiom.a.wide.u39, %fuzz.cfg.halfdot.idiom.b.wide.s
  %fuzz.cfg.halfdot.idiom.acc.add = add i64 %fuzz.cfg.halfdot.idiom.acc.sub32, %fuzz.cfg.halfdot.idiom.mul40
  %fuzz.cfg.halfdot.idiom.mul.i3241 = trunc i64 %fuzz.cfg.halfdot.idiom.mul40 to i32
  %fuzz.cfg.halfdot.idiom.mul.byte42 = lshr i32 %fuzz.cfg.halfdot.idiom.mul.i3241, 8
  %fuzz.cfg.halfdot.idiom.lo = trunc i64 %fuzz.cfg.halfdot.idiom.acc.add to i32
  %fuzz.cfg.halfdot.idiom.hi.shr = lshr i64 %fuzz.cfg.halfdot.idiom.acc.add, 32
  %fuzz.cfg.halfdot.idiom.hi = trunc i64 %fuzz.cfg.halfdot.idiom.hi.shr to i32
  %fuzz.cfg.halfdot.idiom.pack.mask = and i32 %fuzz.cfg.halfdot.idiom.mul.byte, 255
  %fuzz.cfg.halfdot.idiom.pack.add = add i32 0, %fuzz.cfg.halfdot.idiom.pack.mask
  %fuzz.cfg.halfdot.idiom.pack.mask43 = and i32 %fuzz.cfg.halfdot.idiom.mul.byte23, 255
  %fuzz.cfg.halfdot.idiom.pack.shift = shl i32 %fuzz.cfg.halfdot.idiom.pack.mask43, 8
  %fuzz.cfg.halfdot.idiom.pack.add44 = add i32 %fuzz.cfg.halfdot.idiom.pack.add, %fuzz.cfg.halfdot.idiom.pack.shift
  %fuzz.cfg.halfdot.idiom.pack.mask45 = and i32 %fuzz.cfg.halfdot.idiom.mul.byte34, 255
  %fuzz.cfg.halfdot.idiom.pack.shift46 = shl i32 %fuzz.cfg.halfdot.idiom.pack.mask45, 16
  %fuzz.cfg.halfdot.idiom.pack.add47 = add i32 %fuzz.cfg.halfdot.idiom.pack.add44, %fuzz.cfg.halfdot.idiom.pack.shift46
  %fuzz.cfg.halfdot.idiom.pack.mask48 = and i32 %fuzz.cfg.halfdot.idiom.mul.byte42, 255
  %fuzz.cfg.halfdot.idiom.pack.shift49 = shl i32 %fuzz.cfg.halfdot.idiom.pack.mask48, 24
  %fuzz.cfg.halfdot.idiom.pack.add50 = add i32 %fuzz.cfg.halfdot.idiom.pack.add47, %fuzz.cfg.halfdot.idiom.pack.shift49
  %fuzz.cfg.halfdot.idiom.pack.xor = xor i32 %fuzz.cfg.halfdot.idiom.pack.add50, %fuzz.cfg.halfdot.idiom.hi
  %fuzz.loop.multi.exit.key51 = and i32 %fuzz.cfg.halfdot.idiom.pack.xor, 3
  %cond = icmp eq i32 %fuzz.loop.multi.exit.key51, 1
  br i1 %cond, label %fuzz.loop.multi.break.b5, label %fuzz.loop.multi.break.a4

fuzz.loop.multi.break.a4:                         ; preds = %entry
  %fuzz.loop.multi.break.a.val52 = xor i32 %fuzz.cfg.halfdot.idiom.pack.xor, 0
  store i32 0, ptr addrspace(1) %out.ptr, align 4
  ret void

fuzz.loop.multi.break.b5:                         ; preds = %entry
  %fuzz.loop.multi.break.b.val53 = add i32 %fuzz.cfg.halfdot.idiom.pack.xor, %v
  store i32 %fuzz.loop.multi.break.b.val53, ptr addrspace(1) %out.ptr, align 4
  ret void
}

; Function Attrs: nocallback nofree nosync nounwind speculatable willreturn memory(none)
declare noundef i32 @llvm.amdgcn.workgroup.id.x() #1

; Function Attrs: nocallback nofree nosync nounwind speculatable willreturn memory(none)
declare noundef range(i32 0, 1024) i32 @llvm.amdgcn.workitem.id.x() #1

; Function Attrs: nocallback nofree nosync nounwind speculatable willreturn memory(none)
declare i32 @llvm.ssub.sat.i32(i32, i32) #2

attributes #0 = { convergent nounwind "amdgpu-flat-work-group-size"="1,256" "target-cpu"="gfx950" "uniform-work-group-size"="true" }
attributes #1 = { nocallback nofree nosync nounwind speculatable willreturn memory(none) }
attributes #2 = { nocallback nofree nosync nounwind speculatable willreturn memory(none) }
