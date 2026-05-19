; RUN-INPUTS: 0x0
; RUN-LLVM-BUILD: build/llvm-fuzzer
source_filename = "/tmp/fuzzx-amdgpu-orenamd@semianalysis.com/head-pr198373-196418-198412-198419-gen6-20260519-060948/corpus/directed-gpu/shared/.seed-3664506.ll"
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
  br i1 %ok, label %body, label %exit

body:                                             ; preds = %entry
  %idx64 = zext i32 %idx to i64
  %in.ptr = getelementptr i32, ptr addrspace(1) %in, i64 %idx64
  %v = load i32, ptr addrspace(1) %in.ptr, align 4
  %salt = mul i32 %idx, -1640531527
  %mix = xor i32 %v, %salt
  %out.ptr = getelementptr i32, ptr addrspace(1) %out, i64 %idx64
  %fuzz.loop.nest.cond = icmp ult i32 0, 4
  %fuzz.i64pair.idiom.a64 = zext i32 %mix to i64
  %fuzz.i64pair.idiom.b64 = zext i32 %v to i64
  %fuzz.i64pair.idiom.pair.hi = shl i64 %fuzz.i64pair.idiom.a64, 32
  %fuzz.i64pair.idiom.pair = or i64 %fuzz.i64pair.idiom.pair.hi, %fuzz.i64pair.idiom.b64
  %fuzz.i64pair.idiom.mask64 = and i64 %fuzz.i64pair.idiom.pair, 71777214294589695
  %fuzz.i64pair.idiom.spread = mul i64 %fuzz.i64pair.idiom.mask64, 65793
  %fuzz.i64pair.idiom.spread.hi.shr = lshr i64 %fuzz.i64pair.idiom.spread, 32
  %fuzz.i64pair.idiom.spread.hi.i32 = trunc i64 %fuzz.i64pair.idiom.spread.hi.shr to i32
  %fuzz.i64pair.idiom.spread.lo.i32 = trunc i64 %fuzz.i64pair.idiom.spread to i32
  %fuzz.i64pair.idiom.spread.fold = xor i32 %fuzz.i64pair.idiom.spread.hi.i32, %fuzz.i64pair.idiom.spread.lo.i32
  %fuzz.i64pair.idiom.a641 = zext i32 %fuzz.i64pair.idiom.spread.fold to i64
  %fuzz.i64pair.idiom.b642 = zext i32 %mix to i64
  %fuzz.i64pair.idiom.pair.hi3 = shl i64 %fuzz.i64pair.idiom.a641, 32
  %fuzz.i64pair.idiom.pair4 = or i64 %fuzz.i64pair.idiom.pair.hi3, %fuzz.i64pair.idiom.b642
  %fuzz.i64pair.idiom.mul64 = mul i64 %fuzz.i64pair.idiom.a641, %fuzz.i64pair.idiom.b642
  %fuzz.i64pair.idiom.mul.hi.shr = lshr i64 %fuzz.i64pair.idiom.mul64, 32
  %fuzz.i64pair.idiom.mul.hi.i32 = trunc i64 %fuzz.i64pair.idiom.mul.hi.shr to i32
  %fuzz.i64pair.idiom.mul.lo.i32 = trunc i64 %fuzz.i64pair.idiom.mul64 to i32
  %fuzz.i64pair.idiom.mul.fold = xor i32 %fuzz.i64pair.idiom.mul.hi.i32, %fuzz.i64pair.idiom.mul.lo.i32
  %fuzz.bool.cmp0 = icmp sge i32 %fuzz.i64pair.idiom.mul.fold, 0
  %fuzz.bool.cmp1 = icmp ne i32 0, -797244570
  %fuzz.bool.or = or i1 %fuzz.bool.cmp0, %fuzz.bool.cmp1
  %fuzz.bool.zext = zext i1 %fuzz.bool.or to i32
  %fuzz.bool.i32.select = select i1 %fuzz.bool.or, i32 %fuzz.i64pair.idiom.mul.fold, i32 0
  %fuzz.loop.trip.mask = and i32 %fuzz.bool.i32.select, 15
  %fuzz.loop.trip = add i32 %fuzz.loop.trip.mask, 1
  br label %fuzz.loop.header

fuzz.loop.header:                                 ; preds = %fuzz.nested.loop.exit40, %body
  %fuzz.loop.iv = phi i32 [ 0, %body ], [ %fuzz.loop.next, %fuzz.nested.loop.exit40 ]
  %fuzz.loop.acc = phi i32 [ %fuzz.bool.i32.select, %body ], [ %fuzz.loop.acc.inner44, %fuzz.nested.loop.exit40 ]
  %fuzz.loop.cond = icmp ult i32 %fuzz.loop.iv, %fuzz.loop.trip
  br i1 %fuzz.loop.cond, label %fuzz.loop.body, label %fuzz.loop.exit

fuzz.loop.body:                                   ; preds = %fuzz.loop.header
  %fuzz.cfg.predmask.idiom.cmp = icmp ugt i32 %fuzz.loop.acc, 0
  %fuzz.cfg.predmask.idiom.mask.sext = sext i1 %fuzz.cfg.predmask.idiom.cmp to i32
  %fuzz.cfg.predmask.idiom.not = xor i32 %fuzz.cfg.predmask.idiom.mask.sext, -1
  %fuzz.cfg.predmask.idiom.sign = ashr i32 %fuzz.loop.acc, 31
  %fuzz.cfg.predmask.idiom.abs.flip = xor i32 %fuzz.loop.acc, %fuzz.cfg.predmask.idiom.sign
  %fuzz.cfg.predmask.idiom.abs = sub i32 %fuzz.cfg.predmask.idiom.abs.flip, %fuzz.cfg.predmask.idiom.sign
  %fuzz.cfg.predmask.idiom.abs.select = select i1 %fuzz.cfg.predmask.idiom.cmp, i32 %fuzz.cfg.predmask.idiom.abs, i32 %fuzz.loop.acc
  %fuzz.cfg.bytecmp.idiom.a.byte.trunc = trunc i32 %fuzz.cfg.predmask.idiom.abs.select to i8
  %fuzz.cfg.bytecmp.idiom.a.byte.zext = zext i8 %fuzz.cfg.bytecmp.idiom.a.byte.trunc to i32
  %fuzz.cfg.bytecmp.idiom.b.byte.trunc = trunc i32 31 to i8
  %fuzz.cfg.bytecmp.idiom.b.byte.zext = zext i8 %fuzz.cfg.bytecmp.idiom.b.byte.trunc to i32
  %fuzz.cfg.bytecmp.idiom.a.i8 = trunc i32 %fuzz.cfg.bytecmp.idiom.a.byte.zext to i8
  %fuzz.cfg.bytecmp.idiom.a.s32 = sext i8 %fuzz.cfg.bytecmp.idiom.a.i8 to i32
  %fuzz.cfg.bytecmp.idiom.b.i8 = trunc i32 %fuzz.cfg.bytecmp.idiom.b.byte.zext to i8
  %fuzz.cfg.bytecmp.idiom.b.s32 = sext i8 %fuzz.cfg.bytecmp.idiom.b.i8 to i32
  %fuzz.cfg.bytecmp.idiom.slt = icmp slt i32 %fuzz.cfg.bytecmp.idiom.a.s32, %fuzz.cfg.bytecmp.idiom.b.s32
  %fuzz.cfg.bytecmp.idiom.cmp.i32 = zext i1 %fuzz.cfg.bytecmp.idiom.slt to i32
  %fuzz.cfg.bytecmp.idiom.count = add i32 0, %fuzz.cfg.bytecmp.idiom.cmp.i32
  %fuzz.cfg.bytecmp.idiom.byte.sel = select i1 %fuzz.cfg.bytecmp.idiom.slt, i32 %fuzz.cfg.bytecmp.idiom.a.byte.zext, i32 %fuzz.cfg.bytecmp.idiom.b.byte.zext
  %fuzz.cfg.bytecmp.idiom.a.byte.shr = lshr i32 %fuzz.cfg.predmask.idiom.abs.select, 8
  %fuzz.cfg.bytecmp.idiom.a.byte.trunc1 = trunc i32 %fuzz.cfg.bytecmp.idiom.a.byte.shr to i8
  %fuzz.cfg.bytecmp.idiom.a.byte.zext2 = zext i8 %fuzz.cfg.bytecmp.idiom.a.byte.trunc1 to i32
  %fuzz.cfg.bytecmp.idiom.b.byte.shr = lshr i32 31, 8
  %fuzz.cfg.bytecmp.idiom.b.byte.trunc3 = trunc i32 %fuzz.cfg.bytecmp.idiom.b.byte.shr to i8
  %fuzz.cfg.bytecmp.idiom.b.byte.zext4 = zext i8 %fuzz.cfg.bytecmp.idiom.b.byte.trunc3 to i32
  %fuzz.cfg.bytecmp.idiom.a.i85 = trunc i32 %fuzz.cfg.bytecmp.idiom.a.byte.zext2 to i8
  %fuzz.cfg.bytecmp.idiom.a.s326 = sext i8 %fuzz.cfg.bytecmp.idiom.a.i85 to i32
  %fuzz.cfg.bytecmp.idiom.b.i87 = trunc i32 %fuzz.cfg.bytecmp.idiom.b.byte.zext4 to i8
  %fuzz.cfg.bytecmp.idiom.b.s328 = sext i8 %fuzz.cfg.bytecmp.idiom.b.i87 to i32
  %fuzz.cfg.bytecmp.idiom.slt9 = icmp slt i32 %fuzz.cfg.bytecmp.idiom.a.s326, %fuzz.cfg.bytecmp.idiom.b.s328
  %fuzz.cfg.bytecmp.idiom.cmp.i3210 = zext i1 %fuzz.cfg.bytecmp.idiom.slt9 to i32
  %fuzz.cfg.bytecmp.idiom.count11 = add i32 %fuzz.cfg.bytecmp.idiom.count, %fuzz.cfg.bytecmp.idiom.cmp.i3210
  %fuzz.cfg.bytecmp.idiom.byte.a = select i1 %fuzz.cfg.bytecmp.idiom.slt9, i32 %fuzz.cfg.bytecmp.idiom.a.byte.zext2, i32 0
  %fuzz.cfg.bytecmp.idiom.byte.xor = xor i32 %fuzz.cfg.bytecmp.idiom.byte.a, %fuzz.cfg.bytecmp.idiom.b.byte.zext4
  %fuzz.cfg.bytecmp.idiom.a.byte.shr12 = lshr i32 %fuzz.cfg.predmask.idiom.abs.select, 16
  %fuzz.cfg.bytecmp.idiom.a.byte.trunc13 = trunc i32 %fuzz.cfg.bytecmp.idiom.a.byte.shr12 to i8
  %fuzz.cfg.bytecmp.idiom.a.byte.zext14 = zext i8 %fuzz.cfg.bytecmp.idiom.a.byte.trunc13 to i32
  %fuzz.cfg.bytecmp.idiom.b.byte.shr15 = lshr i32 31, 16
  %fuzz.cfg.bytecmp.idiom.b.byte.trunc16 = trunc i32 %fuzz.cfg.bytecmp.idiom.b.byte.shr15 to i8
  %fuzz.cfg.bytecmp.idiom.b.byte.zext17 = zext i8 %fuzz.cfg.bytecmp.idiom.b.byte.trunc16 to i32
  %fuzz.cfg.bytecmp.idiom.eq = icmp eq i32 %fuzz.cfg.bytecmp.idiom.a.byte.zext14, %fuzz.cfg.bytecmp.idiom.b.byte.zext17
  %fuzz.cfg.bytecmp.idiom.cmp.i3218 = zext i1 %fuzz.cfg.bytecmp.idiom.eq to i32
  %fuzz.cfg.bytecmp.idiom.count19 = add i32 %fuzz.cfg.bytecmp.idiom.count11, %fuzz.cfg.bytecmp.idiom.cmp.i3218
  %fuzz.cfg.bytecmp.idiom.byte.b = select i1 %fuzz.cfg.bytecmp.idiom.eq, i32 %fuzz.cfg.bytecmp.idiom.b.byte.zext17, i32 %fuzz.cfg.bytecmp.idiom.a.byte.zext14
  %fuzz.cfg.bytecmp.idiom.byte.add = add i32 %fuzz.cfg.bytecmp.idiom.byte.b, %fuzz.cfg.bytecmp.idiom.cmp.i3218
  %fuzz.cfg.bytecmp.idiom.a.byte.shr20 = lshr i32 %fuzz.cfg.predmask.idiom.abs.select, 24
  %fuzz.cfg.bytecmp.idiom.a.byte.trunc21 = trunc i32 %fuzz.cfg.bytecmp.idiom.a.byte.shr20 to i8
  %fuzz.cfg.bytecmp.idiom.a.byte.zext22 = zext i8 %fuzz.cfg.bytecmp.idiom.a.byte.trunc21 to i32
  %fuzz.cfg.bytecmp.idiom.b.byte.shr23 = lshr i32 31, 24
  %fuzz.cfg.bytecmp.idiom.b.byte.trunc24 = trunc i32 %fuzz.cfg.bytecmp.idiom.b.byte.shr23 to i8
  %fuzz.cfg.bytecmp.idiom.b.byte.zext25 = zext i8 %fuzz.cfg.bytecmp.idiom.b.byte.trunc24 to i32
  %fuzz.cfg.bytecmp.idiom.eq26 = icmp eq i32 %fuzz.cfg.bytecmp.idiom.a.byte.zext22, %fuzz.cfg.bytecmp.idiom.b.byte.zext25
  %fuzz.cfg.bytecmp.idiom.cmp.i3227 = zext i1 %fuzz.cfg.bytecmp.idiom.eq26 to i32
  %fuzz.cfg.bytecmp.idiom.count28 = add i32 %fuzz.cfg.bytecmp.idiom.count19, %fuzz.cfg.bytecmp.idiom.cmp.i3227
  %fuzz.cfg.bytecmp.idiom.byte.sel29 = select i1 %fuzz.cfg.bytecmp.idiom.eq26, i32 %fuzz.cfg.bytecmp.idiom.a.byte.zext22, i32 %fuzz.cfg.bytecmp.idiom.b.byte.zext25
  %fuzz.cfg.bytecmp.idiom.pack.mask = and i32 %fuzz.cfg.bytecmp.idiom.byte.sel, 255
  %fuzz.cfg.bytecmp.idiom.pack.add = add i32 0, %fuzz.cfg.bytecmp.idiom.pack.mask
  %fuzz.cfg.bytecmp.idiom.pack.mask30 = and i32 %fuzz.cfg.bytecmp.idiom.byte.xor, 255
  %fuzz.cfg.bytecmp.idiom.pack.shift = shl i32 %fuzz.cfg.bytecmp.idiom.pack.mask30, 8
  %fuzz.cfg.bytecmp.idiom.pack.add31 = add i32 %fuzz.cfg.bytecmp.idiom.pack.add, %fuzz.cfg.bytecmp.idiom.pack.shift
  %fuzz.cfg.bytecmp.idiom.pack.mask32 = and i32 %fuzz.cfg.bytecmp.idiom.byte.add, 255
  %fuzz.cfg.bytecmp.idiom.pack.shift33 = shl i32 %fuzz.cfg.bytecmp.idiom.pack.mask32, 16
  %fuzz.cfg.bytecmp.idiom.pack.add34 = add i32 %fuzz.cfg.bytecmp.idiom.pack.add31, %fuzz.cfg.bytecmp.idiom.pack.shift33
  %fuzz.cfg.bytecmp.idiom.pack.mask35 = and i32 %fuzz.cfg.bytecmp.idiom.byte.sel29, 255
  %fuzz.cfg.bytecmp.idiom.pack.shift36 = shl i32 %fuzz.cfg.bytecmp.idiom.pack.mask35, 24
  %fuzz.cfg.bytecmp.idiom.pack.add37 = add i32 %fuzz.cfg.bytecmp.idiom.pack.add34, %fuzz.cfg.bytecmp.idiom.pack.shift36
  %fuzz.cfg.bytecmp.idiom.count.xor = xor i32 %fuzz.cfg.bytecmp.idiom.pack.add37, %fuzz.cfg.bytecmp.idiom.count28
  %fuzz.loop.trip.inner.mask41 = and i32 -2147483648, 3
  %fuzz.loop.trip.inner42 = add i32 %fuzz.loop.trip.inner.mask41, 1
  br label %fuzz.nested.loop.header38

fuzz.nested.loop.header38:                        ; preds = %fuzz.nested.join, %fuzz.loop.body
  %fuzz.loop.iv.inner43 = phi i32 [ 0, %fuzz.loop.body ], [ %fuzz.loop.next.inner192, %fuzz.nested.join ]
  %fuzz.loop.acc.inner44 = phi i32 [ %fuzz.cfg.bytecmp.idiom.count.xor, %fuzz.loop.body ], [ %fuzz.nested.phi, %fuzz.nested.join ]
  %fuzz.loop.cond.inner45 = icmp ult i32 %fuzz.loop.iv.inner43, %fuzz.loop.trip.inner42
  br i1 %fuzz.loop.cond.inner45, label %fuzz.nested.loop.body39, label %fuzz.nested.loop.exit40

fuzz.nested.loop.body39:                          ; preds = %fuzz.nested.loop.header38
  %fuzz.cfg.packunpack.idiom.a0.trunc = trunc i32 %fuzz.loop.acc.inner44 to i8
  %fuzz.cfg.packunpack.idiom.a0.zext = zext i8 %fuzz.cfg.packunpack.idiom.a0.trunc to i32
  %fuzz.cfg.packunpack.idiom.a1.shr = lshr i32 %fuzz.loop.acc.inner44, 8
  %fuzz.cfg.packunpack.idiom.a1.trunc = trunc i32 %fuzz.cfg.packunpack.idiom.a1.shr to i8
  %fuzz.cfg.packunpack.idiom.a1.zext = zext i8 %fuzz.cfg.packunpack.idiom.a1.trunc to i32
  %fuzz.cfg.packunpack.idiom.a2.shr = lshr i32 %fuzz.loop.acc.inner44, 16
  %fuzz.cfg.packunpack.idiom.a2.trunc = trunc i32 %fuzz.cfg.packunpack.idiom.a2.shr to i8
  %fuzz.cfg.packunpack.idiom.a2.zext = zext i8 %fuzz.cfg.packunpack.idiom.a2.trunc to i32
  %fuzz.cfg.packunpack.idiom.b0.trunc = trunc i32 7 to i8
  %fuzz.cfg.packunpack.idiom.b0.zext = zext i8 %fuzz.cfg.packunpack.idiom.b0.trunc to i32
  %fuzz.cfg.packunpack.idiom.b1.shr = lshr i32 7, 8
  %fuzz.cfg.packunpack.idiom.b1.trunc = trunc i32 %fuzz.cfg.packunpack.idiom.b1.shr to i8
  %fuzz.cfg.packunpack.idiom.b1.zext = zext i8 %fuzz.cfg.packunpack.idiom.b1.trunc to i32
  %fuzz.cfg.packunpack.idiom.b2.shr = lshr i32 7, 16
  %fuzz.cfg.packunpack.idiom.b2.trunc = trunc i32 %fuzz.cfg.packunpack.idiom.b2.shr to i8
  %fuzz.cfg.packunpack.idiom.b2.zext = zext i8 %fuzz.cfg.packunpack.idiom.b2.trunc to i32
  %fuzz.cfg.packunpack.idiom.byte.mul0 = mul i32 %fuzz.cfg.packunpack.idiom.a0.zext, %fuzz.cfg.packunpack.idiom.b0.zext
  %fuzz.cfg.packunpack.idiom.byte.mul1 = mul i32 %fuzz.cfg.packunpack.idiom.a1.zext, %fuzz.cfg.packunpack.idiom.b1.zext
  %fuzz.cfg.packunpack.idiom.byte.mul2 = mul i32 %fuzz.cfg.packunpack.idiom.a2.zext, %fuzz.cfg.packunpack.idiom.b2.zext
  %fuzz.cfg.packunpack.idiom.byte.sum01 = add i32 %fuzz.cfg.packunpack.idiom.byte.mul0, %fuzz.cfg.packunpack.idiom.byte.mul1
  %fuzz.cfg.packunpack.idiom.byte.sum = add i32 %fuzz.cfg.packunpack.idiom.byte.sum01, %fuzz.cfg.packunpack.idiom.byte.mul2
  %fuzz.cfg.ovchain.idiom.uadd0.call = call { i32, i1 } @llvm.uadd.with.overflow.i32(i32 %fuzz.cfg.packunpack.idiom.byte.sum, i32 255)
  %fuzz.cfg.ovchain.idiom.uadd0.value = extractvalue { i32, i1 } %fuzz.cfg.ovchain.idiom.uadd0.call, 0
  %fuzz.cfg.ovchain.idiom.uadd0.overflow = extractvalue { i32, i1 } %fuzz.cfg.ovchain.idiom.uadd0.call, 1
  %fuzz.cfg.ovchain.idiom.carry0 = zext i1 %fuzz.cfg.ovchain.idiom.uadd0.overflow to i32
  %fuzz.cfg.ovchain.idiom.sub.rhs = add i32 7, %fuzz.cfg.ovchain.idiom.carry0
  %fuzz.cfg.ovchain.idiom.usub0.call = call { i32, i1 } @llvm.usub.with.overflow.i32(i32 %fuzz.cfg.ovchain.idiom.uadd0.value, i32 %fuzz.cfg.ovchain.idiom.sub.rhs)
  %fuzz.cfg.ovchain.idiom.usub0.value = extractvalue { i32, i1 } %fuzz.cfg.ovchain.idiom.usub0.call, 0
  %fuzz.cfg.ovchain.idiom.usub0.overflow = extractvalue { i32, i1 } %fuzz.cfg.ovchain.idiom.usub0.call, 1
  %fuzz.cfg.ovchain.idiom.borrow0 = zext i1 %fuzz.cfg.ovchain.idiom.usub0.overflow to i32
  %fuzz.cfg.ovchain.idiom.sadd0.call = call { i32, i1 } @llvm.sadd.with.overflow.i32(i32 %fuzz.cfg.ovchain.idiom.usub0.value, i32 7)
  %fuzz.cfg.ovchain.idiom.sadd0.value = extractvalue { i32, i1 } %fuzz.cfg.ovchain.idiom.sadd0.call, 0
  %fuzz.cfg.ovchain.idiom.sadd0.overflow = extractvalue { i32, i1 } %fuzz.cfg.ovchain.idiom.sadd0.call, 1
  %fuzz.cfg.ovchain.idiom.sadd.mask.sext = sext i1 %fuzz.cfg.ovchain.idiom.sadd0.overflow to i32
  %fuzz.cfg.ovchain.idiom.sadd.keep = and i32 %fuzz.cfg.ovchain.idiom.sadd0.value, %fuzz.cfg.ovchain.idiom.sadd.mask.sext
  %fuzz.cfg.ovchain.idiom.sadd.not = xor i32 %fuzz.cfg.ovchain.idiom.sadd.mask.sext, -1
  %fuzz.cfg.ovchain.idiom.sadd.fallback = and i32 %fuzz.cfg.packunpack.idiom.byte.sum, %fuzz.cfg.ovchain.idiom.sadd.not
  %fuzz.cfg.ovchain.idiom.sadd.select = or i32 %fuzz.cfg.ovchain.idiom.sadd.keep, %fuzz.cfg.ovchain.idiom.sadd.fallback
  %fuzz.cfg.rotcascade.idiom.shift.seed = add i32 31, 1
  %fuzz.cfg.rotcascade.idiom.shift = and i32 %fuzz.cfg.rotcascade.idiom.shift.seed, 31
  %fuzz.cfg.rotcascade.idiom.inv.raw = sub i32 32, %fuzz.cfg.rotcascade.idiom.shift
  %fuzz.cfg.rotcascade.idiom.inv = and i32 %fuzz.cfg.rotcascade.idiom.inv.raw, 31
  %fuzz.cfg.rotcascade.idiom.rotl.lo = shl i32 %fuzz.cfg.ovchain.idiom.sadd.select, %fuzz.cfg.rotcascade.idiom.shift
  %fuzz.cfg.rotcascade.idiom.rotl.hi = lshr i32 %fuzz.cfg.ovchain.idiom.sadd.select, %fuzz.cfg.rotcascade.idiom.inv
  %fuzz.cfg.rotcascade.idiom.rotl = or i32 %fuzz.cfg.rotcascade.idiom.rotl.lo, %fuzz.cfg.rotcascade.idiom.rotl.hi
  %fuzz.cfg.rotcascade.idiom.rotr.lo = lshr i32 31, %fuzz.cfg.rotcascade.idiom.shift
  %fuzz.cfg.rotcascade.idiom.rotr.hi = shl i32 31, %fuzz.cfg.rotcascade.idiom.inv
  %fuzz.cfg.rotcascade.idiom.rotr = or i32 %fuzz.cfg.rotcascade.idiom.rotr.lo, %fuzz.cfg.rotcascade.idiom.rotr.hi
  %fuzz.cfg.rotcascade.idiom.pop = call i32 @llvm.ctpop.i32(i32 %fuzz.cfg.rotcascade.idiom.rotl)
  %fuzz.cfg.rotcascade.idiom.lt = icmp ult i32 %fuzz.cfg.rotcascade.idiom.pop, 17
  %fuzz.cfg.rotcascade.idiom.mask.select = select i1 %fuzz.cfg.rotcascade.idiom.lt, i32 -1, i32 0
  %fuzz.cfg.rotcascade.idiom.merge.l = and i32 %fuzz.cfg.rotcascade.idiom.rotl, %fuzz.cfg.rotcascade.idiom.mask.select
  %fuzz.cfg.rotcascade.idiom.mask.not = xor i32 %fuzz.cfg.rotcascade.idiom.mask.select, -1
  %fuzz.cfg.rotcascade.idiom.merge.r = and i32 %fuzz.cfg.rotcascade.idiom.rotr, %fuzz.cfg.rotcascade.idiom.mask.not
  %fuzz.cfg.rotcascade.idiom.merge = or i32 %fuzz.cfg.rotcascade.idiom.merge.l, %fuzz.cfg.rotcascade.idiom.merge.r
  %fuzz.cfg.rotcascade.idiom.acc.pop = add i32 %fuzz.cfg.ovchain.idiom.sadd.select, %fuzz.cfg.rotcascade.idiom.pop
  %fuzz.cfg.rotcascade.idiom.acc.next = xor i32 %fuzz.cfg.rotcascade.idiom.merge, %fuzz.cfg.rotcascade.idiom.acc.pop
  %fuzz.cfg.rotcascade.idiom.seed.next = add i32 31, %fuzz.cfg.rotcascade.idiom.rotl
  %fuzz.cfg.rotcascade.idiom.shift.seed46 = add i32 %fuzz.cfg.rotcascade.idiom.seed.next, 8
  %fuzz.cfg.rotcascade.idiom.shift47 = and i32 %fuzz.cfg.rotcascade.idiom.shift.seed46, 31
  %fuzz.cfg.rotcascade.idiom.inv.raw48 = sub i32 32, %fuzz.cfg.rotcascade.idiom.shift47
  %fuzz.cfg.rotcascade.idiom.inv49 = and i32 %fuzz.cfg.rotcascade.idiom.inv.raw48, 31
  %fuzz.cfg.rotcascade.idiom.rotl.lo50 = shl i32 %fuzz.cfg.rotcascade.idiom.acc.next, %fuzz.cfg.rotcascade.idiom.shift47
  %fuzz.cfg.rotcascade.idiom.rotl.hi51 = lshr i32 %fuzz.cfg.rotcascade.idiom.acc.next, %fuzz.cfg.rotcascade.idiom.inv49
  %fuzz.cfg.rotcascade.idiom.rotl52 = or i32 %fuzz.cfg.rotcascade.idiom.rotl.lo50, %fuzz.cfg.rotcascade.idiom.rotl.hi51
  %fuzz.cfg.rotcascade.idiom.rotr.lo53 = lshr i32 %fuzz.cfg.rotcascade.idiom.seed.next, %fuzz.cfg.rotcascade.idiom.shift47
  %fuzz.cfg.rotcascade.idiom.rotr.hi54 = shl i32 %fuzz.cfg.rotcascade.idiom.seed.next, %fuzz.cfg.rotcascade.idiom.inv49
  %fuzz.cfg.rotcascade.idiom.rotr55 = or i32 %fuzz.cfg.rotcascade.idiom.rotr.lo53, %fuzz.cfg.rotcascade.idiom.rotr.hi54
  %fuzz.cfg.rotcascade.idiom.pop56 = call i32 @llvm.ctpop.i32(i32 %fuzz.cfg.rotcascade.idiom.rotl52)
  %fuzz.cfg.rotcascade.idiom.lt57 = icmp ult i32 %fuzz.cfg.rotcascade.idiom.pop56, 17
  %fuzz.cfg.rotcascade.idiom.mask.sext = sext i1 %fuzz.cfg.rotcascade.idiom.lt57 to i32
  %fuzz.cfg.rotcascade.idiom.merge.l58 = and i32 %fuzz.cfg.rotcascade.idiom.rotl52, %fuzz.cfg.rotcascade.idiom.mask.sext
  %fuzz.cfg.rotcascade.idiom.mask.not59 = xor i32 %fuzz.cfg.rotcascade.idiom.mask.sext, -1
  %fuzz.cfg.rotcascade.idiom.merge.r60 = and i32 %fuzz.cfg.rotcascade.idiom.rotr55, %fuzz.cfg.rotcascade.idiom.mask.not59
  %fuzz.cfg.rotcascade.idiom.merge61 = or i32 %fuzz.cfg.rotcascade.idiom.merge.l58, %fuzz.cfg.rotcascade.idiom.merge.r60
  %fuzz.cfg.rotcascade.idiom.acc.pop62 = add i32 %fuzz.cfg.rotcascade.idiom.acc.next, %fuzz.cfg.rotcascade.idiom.pop56
  %fuzz.cfg.rotcascade.idiom.acc.next63 = xor i32 %fuzz.cfg.rotcascade.idiom.merge61, %fuzz.cfg.rotcascade.idiom.acc.pop62
  %fuzz.cfg.rotcascade.idiom.seed.next64 = add i32 %fuzz.cfg.rotcascade.idiom.seed.next, %fuzz.cfg.rotcascade.idiom.rotl52
  %fuzz.cfg.rotcascade.idiom.shift.seed65 = add i32 %fuzz.cfg.rotcascade.idiom.seed.next64, 15
  %fuzz.cfg.rotcascade.idiom.shift66 = and i32 %fuzz.cfg.rotcascade.idiom.shift.seed65, 31
  %fuzz.cfg.rotcascade.idiom.inv.raw67 = sub i32 32, %fuzz.cfg.rotcascade.idiom.shift66
  %fuzz.cfg.rotcascade.idiom.inv68 = and i32 %fuzz.cfg.rotcascade.idiom.inv.raw67, 31
  %fuzz.cfg.rotcascade.idiom.rotl.lo69 = shl i32 %fuzz.cfg.rotcascade.idiom.acc.next63, %fuzz.cfg.rotcascade.idiom.shift66
  %fuzz.cfg.rotcascade.idiom.rotl.hi70 = lshr i32 %fuzz.cfg.rotcascade.idiom.acc.next63, %fuzz.cfg.rotcascade.idiom.inv68
  %fuzz.cfg.rotcascade.idiom.rotl71 = or i32 %fuzz.cfg.rotcascade.idiom.rotl.lo69, %fuzz.cfg.rotcascade.idiom.rotl.hi70
  %fuzz.cfg.rotcascade.idiom.rotr.lo72 = lshr i32 %fuzz.cfg.rotcascade.idiom.seed.next64, %fuzz.cfg.rotcascade.idiom.shift66
  %fuzz.cfg.rotcascade.idiom.rotr.hi73 = shl i32 %fuzz.cfg.rotcascade.idiom.seed.next64, %fuzz.cfg.rotcascade.idiom.inv68
  %fuzz.cfg.rotcascade.idiom.rotr74 = or i32 %fuzz.cfg.rotcascade.idiom.rotr.lo72, %fuzz.cfg.rotcascade.idiom.rotr.hi73
  %fuzz.cfg.rotcascade.idiom.pop75 = call i32 @llvm.ctpop.i32(i32 %fuzz.cfg.rotcascade.idiom.rotl71)
  %fuzz.cfg.rotcascade.idiom.lt76 = icmp ult i32 %fuzz.cfg.rotcascade.idiom.pop75, 17
  %fuzz.cfg.rotcascade.idiom.mask.select77 = select i1 %fuzz.cfg.rotcascade.idiom.lt76, i32 -1, i32 0
  %fuzz.cfg.rotcascade.idiom.merge.l78 = and i32 %fuzz.cfg.rotcascade.idiom.rotl71, %fuzz.cfg.rotcascade.idiom.mask.select77
  %fuzz.cfg.rotcascade.idiom.mask.not79 = xor i32 %fuzz.cfg.rotcascade.idiom.mask.select77, -1
  %fuzz.cfg.rotcascade.idiom.merge.r80 = and i32 %fuzz.cfg.rotcascade.idiom.rotr74, %fuzz.cfg.rotcascade.idiom.mask.not79
  %fuzz.cfg.rotcascade.idiom.merge81 = or i32 %fuzz.cfg.rotcascade.idiom.merge.l78, %fuzz.cfg.rotcascade.idiom.merge.r80
  %fuzz.cfg.rotcascade.idiom.acc.pop82 = add i32 %fuzz.cfg.rotcascade.idiom.acc.next63, %fuzz.cfg.rotcascade.idiom.pop75
  %fuzz.cfg.rotcascade.idiom.acc.next83 = xor i32 %fuzz.cfg.rotcascade.idiom.merge81, %fuzz.cfg.rotcascade.idiom.acc.pop82
  %fuzz.cfg.rotcascade.idiom.seed.next84 = add i32 %fuzz.cfg.rotcascade.idiom.seed.next64, %fuzz.cfg.rotcascade.idiom.rotl71
  %fuzz.cfg.rotcascade.idiom.shift.seed85 = add i32 %fuzz.cfg.rotcascade.idiom.seed.next84, 22
  %fuzz.cfg.rotcascade.idiom.shift86 = and i32 %fuzz.cfg.rotcascade.idiom.shift.seed85, 31
  %fuzz.cfg.rotcascade.idiom.inv.raw87 = sub i32 32, %fuzz.cfg.rotcascade.idiom.shift86
  %fuzz.cfg.rotcascade.idiom.inv88 = and i32 %fuzz.cfg.rotcascade.idiom.inv.raw87, 31
  %fuzz.cfg.rotcascade.idiom.rotl.lo89 = shl i32 %fuzz.cfg.rotcascade.idiom.acc.next83, %fuzz.cfg.rotcascade.idiom.shift86
  %fuzz.cfg.rotcascade.idiom.rotl.hi90 = lshr i32 %fuzz.cfg.rotcascade.idiom.acc.next83, %fuzz.cfg.rotcascade.idiom.inv88
  %fuzz.cfg.rotcascade.idiom.rotl91 = or i32 %fuzz.cfg.rotcascade.idiom.rotl.lo89, %fuzz.cfg.rotcascade.idiom.rotl.hi90
  %fuzz.cfg.rotcascade.idiom.rotr.lo92 = lshr i32 %fuzz.cfg.rotcascade.idiom.seed.next84, %fuzz.cfg.rotcascade.idiom.shift86
  %fuzz.cfg.rotcascade.idiom.rotr.hi93 = shl i32 %fuzz.cfg.rotcascade.idiom.seed.next84, %fuzz.cfg.rotcascade.idiom.inv88
  %fuzz.cfg.rotcascade.idiom.rotr94 = or i32 %fuzz.cfg.rotcascade.idiom.rotr.lo92, %fuzz.cfg.rotcascade.idiom.rotr.hi93
  %fuzz.cfg.rotcascade.idiom.pop95 = call i32 @llvm.ctpop.i32(i32 %fuzz.cfg.rotcascade.idiom.rotl91)
  %fuzz.cfg.rotcascade.idiom.lt96 = icmp ult i32 %fuzz.cfg.rotcascade.idiom.pop95, 17
  %fuzz.cfg.rotcascade.idiom.mask.select97 = select i1 %fuzz.cfg.rotcascade.idiom.lt96, i32 -1, i32 0
  %fuzz.cfg.rotcascade.idiom.merge.l98 = and i32 %fuzz.cfg.rotcascade.idiom.rotl91, %fuzz.cfg.rotcascade.idiom.mask.select97
  %fuzz.cfg.rotcascade.idiom.mask.not99 = xor i32 %fuzz.cfg.rotcascade.idiom.mask.select97, -1
  %fuzz.cfg.rotcascade.idiom.merge.r100 = and i32 %fuzz.cfg.rotcascade.idiom.rotr94, %fuzz.cfg.rotcascade.idiom.mask.not99
  %fuzz.cfg.rotcascade.idiom.merge101 = or i32 %fuzz.cfg.rotcascade.idiom.merge.l98, %fuzz.cfg.rotcascade.idiom.merge.r100
  %fuzz.cfg.rotcascade.idiom.acc.pop102 = add i32 %fuzz.cfg.rotcascade.idiom.acc.next83, %fuzz.cfg.rotcascade.idiom.pop95
  %fuzz.cfg.rotcascade.idiom.acc.next103 = xor i32 %fuzz.cfg.rotcascade.idiom.merge101, %fuzz.cfg.rotcascade.idiom.acc.pop102
  %fuzz.cfg.rotcascade.idiom.seed.next104 = add i32 %fuzz.cfg.rotcascade.idiom.seed.next84, %fuzz.cfg.rotcascade.idiom.rotl91
  %fuzz.cfg.rotcascade.idiom.seed.sub = sub i32 %fuzz.cfg.rotcascade.idiom.seed.next104, %fuzz.cfg.rotcascade.idiom.acc.next103
  %fuzz.cfg.i64pair.idiom.a64 = zext i32 %fuzz.cfg.rotcascade.idiom.seed.sub to i64
  %fuzz.cfg.i64pair.idiom.b64 = zext i32 2 to i64
  %fuzz.cfg.i64pair.idiom.pair.hi = shl i64 %fuzz.cfg.i64pair.idiom.a64, 32
  %fuzz.cfg.i64pair.idiom.pair = or i64 %fuzz.cfg.i64pair.idiom.pair.hi, %fuzz.cfg.i64pair.idiom.b64
  %fuzz.cfg.i64pair.idiom.cmp.x = xor i64 %fuzz.cfg.i64pair.idiom.pair, 3823022773417008773
  %fuzz.cfg.i64pair.idiom.cmp = icmp ugt i64 %fuzz.cfg.i64pair.idiom.pair, %fuzz.cfg.i64pair.idiom.cmp.x
  %fuzz.cfg.i64pair.idiom.sel.add = add i64 %fuzz.cfg.i64pair.idiom.pair, %fuzz.cfg.i64pair.idiom.a64
  %fuzz.cfg.i64pair.idiom.sel.sub = sub i64 %fuzz.cfg.i64pair.idiom.pair, %fuzz.cfg.i64pair.idiom.b64
  %fuzz.cfg.i64pair.idiom.sel = select i1 %fuzz.cfg.i64pair.idiom.cmp, i64 %fuzz.cfg.i64pair.idiom.sel.add, i64 %fuzz.cfg.i64pair.idiom.sel.sub
  %fuzz.cfg.i64pair.idiom.sel.hi.shr = lshr i64 %fuzz.cfg.i64pair.idiom.sel, 32
  %fuzz.cfg.i64pair.idiom.sel.hi.i32 = trunc i64 %fuzz.cfg.i64pair.idiom.sel.hi.shr to i32
  %fuzz.cfg.i64pair.idiom.sel.lo.i32 = trunc i64 %fuzz.cfg.i64pair.idiom.sel to i32
  %fuzz.cfg.i64pair.idiom.sel.fold = xor i32 %fuzz.cfg.i64pair.idiom.sel.hi.i32, %fuzz.cfg.i64pair.idiom.sel.lo.i32
  %fuzz.nested.branch = icmp sge i32 %fuzz.cfg.i64pair.idiom.sel.fold, 2
  br i1 %fuzz.nested.branch, label %fuzz.nested.then, label %fuzz.nested.else

fuzz.nested.then:                                 ; preds = %fuzz.nested.loop.body39
  %fuzz.cfg.bytecmp.idiom.a.byte.trunc105 = trunc i32 %fuzz.cfg.i64pair.idiom.sel.fold to i8
  %fuzz.cfg.bytecmp.idiom.a.byte.zext106 = zext i8 %fuzz.cfg.bytecmp.idiom.a.byte.trunc105 to i32
  %fuzz.cfg.bytecmp.idiom.b.byte.trunc107 = trunc i32 1 to i8
  %fuzz.cfg.bytecmp.idiom.b.byte.zext108 = zext i8 %fuzz.cfg.bytecmp.idiom.b.byte.trunc107 to i32
  %fuzz.cfg.bytecmp.idiom.ugt = icmp ugt i32 %fuzz.cfg.bytecmp.idiom.a.byte.zext106, %fuzz.cfg.bytecmp.idiom.b.byte.zext108
  %fuzz.cfg.bytecmp.idiom.cmp.i32109 = zext i1 %fuzz.cfg.bytecmp.idiom.ugt to i32
  %fuzz.cfg.bytecmp.idiom.count110 = add i32 0, %fuzz.cfg.bytecmp.idiom.cmp.i32109
  %fuzz.cfg.bytecmp.idiom.byte.a111 = select i1 %fuzz.cfg.bytecmp.idiom.ugt, i32 %fuzz.cfg.bytecmp.idiom.a.byte.zext106, i32 0
  %fuzz.cfg.bytecmp.idiom.byte.xor112 = xor i32 %fuzz.cfg.bytecmp.idiom.byte.a111, %fuzz.cfg.bytecmp.idiom.b.byte.zext108
  %fuzz.cfg.bytecmp.idiom.a.byte.shr113 = lshr i32 %fuzz.cfg.i64pair.idiom.sel.fold, 8
  %fuzz.cfg.bytecmp.idiom.a.byte.trunc114 = trunc i32 %fuzz.cfg.bytecmp.idiom.a.byte.shr113 to i8
  %fuzz.cfg.bytecmp.idiom.a.byte.zext115 = zext i8 %fuzz.cfg.bytecmp.idiom.a.byte.trunc114 to i32
  %fuzz.cfg.bytecmp.idiom.b.byte.shr116 = lshr i32 1, 8
  %fuzz.cfg.bytecmp.idiom.b.byte.trunc117 = trunc i32 %fuzz.cfg.bytecmp.idiom.b.byte.shr116 to i8
  %fuzz.cfg.bytecmp.idiom.b.byte.zext118 = zext i8 %fuzz.cfg.bytecmp.idiom.b.byte.trunc117 to i32
  %fuzz.cfg.bytecmp.idiom.eq119 = icmp eq i32 %fuzz.cfg.bytecmp.idiom.a.byte.zext115, %fuzz.cfg.bytecmp.idiom.b.byte.zext118
  %fuzz.cfg.bytecmp.idiom.cmp.i32120 = zext i1 %fuzz.cfg.bytecmp.idiom.eq119 to i32
  %fuzz.cfg.bytecmp.idiom.count121 = add i32 %fuzz.cfg.bytecmp.idiom.count110, %fuzz.cfg.bytecmp.idiom.cmp.i32120
  %fuzz.cfg.bytecmp.idiom.byte.sel122 = select i1 %fuzz.cfg.bytecmp.idiom.eq119, i32 %fuzz.cfg.bytecmp.idiom.a.byte.zext115, i32 %fuzz.cfg.bytecmp.idiom.b.byte.zext118
  %fuzz.cfg.bytecmp.idiom.a.byte.shr123 = lshr i32 %fuzz.cfg.i64pair.idiom.sel.fold, 16
  %fuzz.cfg.bytecmp.idiom.a.byte.trunc124 = trunc i32 %fuzz.cfg.bytecmp.idiom.a.byte.shr123 to i8
  %fuzz.cfg.bytecmp.idiom.a.byte.zext125 = zext i8 %fuzz.cfg.bytecmp.idiom.a.byte.trunc124 to i32
  %fuzz.cfg.bytecmp.idiom.b.byte.shr126 = lshr i32 1, 16
  %fuzz.cfg.bytecmp.idiom.b.byte.trunc127 = trunc i32 %fuzz.cfg.bytecmp.idiom.b.byte.shr126 to i8
  %fuzz.cfg.bytecmp.idiom.b.byte.zext128 = zext i8 %fuzz.cfg.bytecmp.idiom.b.byte.trunc127 to i32
  %fuzz.cfg.bytecmp.idiom.eq129 = icmp eq i32 %fuzz.cfg.bytecmp.idiom.a.byte.zext125, %fuzz.cfg.bytecmp.idiom.b.byte.zext128
  %fuzz.cfg.bytecmp.idiom.cmp.i32130 = zext i1 %fuzz.cfg.bytecmp.idiom.eq129 to i32
  %fuzz.cfg.bytecmp.idiom.count131 = add i32 %fuzz.cfg.bytecmp.idiom.count121, %fuzz.cfg.bytecmp.idiom.cmp.i32130
  %fuzz.cfg.bytecmp.idiom.byte.sel132 = select i1 %fuzz.cfg.bytecmp.idiom.eq129, i32 %fuzz.cfg.bytecmp.idiom.a.byte.zext125, i32 %fuzz.cfg.bytecmp.idiom.b.byte.zext128
  %fuzz.cfg.bytecmp.idiom.a.byte.shr133 = lshr i32 %fuzz.cfg.i64pair.idiom.sel.fold, 24
  %fuzz.cfg.bytecmp.idiom.a.byte.trunc134 = trunc i32 %fuzz.cfg.bytecmp.idiom.a.byte.shr133 to i8
  %fuzz.cfg.bytecmp.idiom.a.byte.zext135 = zext i8 %fuzz.cfg.bytecmp.idiom.a.byte.trunc134 to i32
  %fuzz.cfg.bytecmp.idiom.b.byte.shr136 = lshr i32 1, 24
  %fuzz.cfg.bytecmp.idiom.b.byte.trunc137 = trunc i32 %fuzz.cfg.bytecmp.idiom.b.byte.shr136 to i8
  %fuzz.cfg.bytecmp.idiom.b.byte.zext138 = zext i8 %fuzz.cfg.bytecmp.idiom.b.byte.trunc137 to i32
  %fuzz.cfg.bytecmp.idiom.ult = icmp ult i32 %fuzz.cfg.bytecmp.idiom.a.byte.zext135, %fuzz.cfg.bytecmp.idiom.b.byte.zext138
  %fuzz.cfg.bytecmp.idiom.cmp.i32139 = zext i1 %fuzz.cfg.bytecmp.idiom.ult to i32
  %fuzz.cfg.bytecmp.idiom.count140 = add i32 %fuzz.cfg.bytecmp.idiom.count131, %fuzz.cfg.bytecmp.idiom.cmp.i32139
  %fuzz.cfg.bytecmp.idiom.byte.b141 = select i1 %fuzz.cfg.bytecmp.idiom.ult, i32 %fuzz.cfg.bytecmp.idiom.b.byte.zext138, i32 %fuzz.cfg.bytecmp.idiom.a.byte.zext135
  %fuzz.cfg.bytecmp.idiom.byte.add142 = add i32 %fuzz.cfg.bytecmp.idiom.byte.b141, %fuzz.cfg.bytecmp.idiom.cmp.i32139
  %fuzz.cfg.bytecmp.idiom.pack.mask143 = and i32 %fuzz.cfg.bytecmp.idiom.byte.xor112, 255
  %fuzz.cfg.bytecmp.idiom.pack.or = or i32 0, %fuzz.cfg.bytecmp.idiom.pack.mask143
  %fuzz.cfg.bytecmp.idiom.pack.mask144 = and i32 %fuzz.cfg.bytecmp.idiom.byte.sel122, 255
  %fuzz.cfg.bytecmp.idiom.pack.shift145 = shl i32 %fuzz.cfg.bytecmp.idiom.pack.mask144, 8
  %fuzz.cfg.bytecmp.idiom.pack.or146 = or i32 %fuzz.cfg.bytecmp.idiom.pack.or, %fuzz.cfg.bytecmp.idiom.pack.shift145
  %fuzz.cfg.bytecmp.idiom.pack.mask147 = and i32 %fuzz.cfg.bytecmp.idiom.byte.sel132, 255
  %fuzz.cfg.bytecmp.idiom.pack.shift148 = shl i32 %fuzz.cfg.bytecmp.idiom.pack.mask147, 16
  %fuzz.cfg.bytecmp.idiom.pack.or149 = or i32 %fuzz.cfg.bytecmp.idiom.pack.or146, %fuzz.cfg.bytecmp.idiom.pack.shift148
  %fuzz.cfg.bytecmp.idiom.pack.mask150 = and i32 %fuzz.cfg.bytecmp.idiom.byte.add142, 255
  %fuzz.cfg.bytecmp.idiom.pack.shift151 = shl i32 %fuzz.cfg.bytecmp.idiom.pack.mask150, 24
  %fuzz.cfg.bytecmp.idiom.pack.or152 = or i32 %fuzz.cfg.bytecmp.idiom.pack.or149, %fuzz.cfg.bytecmp.idiom.pack.shift151
  %fuzz.cfg.bytecmp.idiom.count.shl = shl i32 %fuzz.cfg.bytecmp.idiom.count140, 24
  %fuzz.cfg.bytecmp.idiom.count.add = add i32 %fuzz.cfg.bytecmp.idiom.pack.or152, %fuzz.cfg.bytecmp.idiom.count.shl
  %fuzz.cfg.i64byteperm.idiom.byte.shr = lshr i32 -1, 16
  %fuzz.cfg.i64byteperm.idiom.byte.trunc = trunc i32 %fuzz.cfg.i64byteperm.idiom.byte.shr to i8
  %fuzz.cfg.i64byteperm.idiom.byte.zext = zext i8 %fuzz.cfg.i64byteperm.idiom.byte.trunc to i32
  %fuzz.cfg.i64byteperm.idiom.byte64 = zext i32 %fuzz.cfg.i64byteperm.idiom.byte.zext to i64
  %fuzz.cfg.i64byteperm.idiom.byte.shl = shl i64 %fuzz.cfg.i64byteperm.idiom.byte64, 0
  %fuzz.cfg.i64byteperm.idiom.wide = or i64 0, %fuzz.cfg.i64byteperm.idiom.byte.shl
  %fuzz.cfg.i64byteperm.idiom.byte.shr153 = lshr i32 %fuzz.cfg.bytecmp.idiom.count.add, 16
  %fuzz.cfg.i64byteperm.idiom.byte.trunc154 = trunc i32 %fuzz.cfg.i64byteperm.idiom.byte.shr153 to i8
  %fuzz.cfg.i64byteperm.idiom.byte.zext155 = zext i8 %fuzz.cfg.i64byteperm.idiom.byte.trunc154 to i32
  %fuzz.cfg.i64byteperm.idiom.byte64156 = zext i32 %fuzz.cfg.i64byteperm.idiom.byte.zext155 to i64
  %fuzz.cfg.i64byteperm.idiom.byte.shl157 = shl i64 %fuzz.cfg.i64byteperm.idiom.byte64156, 8
  %fuzz.cfg.i64byteperm.idiom.wide158 = or i64 %fuzz.cfg.i64byteperm.idiom.wide, %fuzz.cfg.i64byteperm.idiom.byte.shl157
  %fuzz.cfg.i64byteperm.idiom.byte.shr159 = lshr i32 -1, 24
  %fuzz.cfg.i64byteperm.idiom.byte.trunc160 = trunc i32 %fuzz.cfg.i64byteperm.idiom.byte.shr159 to i8
  %fuzz.cfg.i64byteperm.idiom.byte.zext161 = zext i8 %fuzz.cfg.i64byteperm.idiom.byte.trunc160 to i32
  %fuzz.cfg.i64byteperm.idiom.byte64162 = zext i32 %fuzz.cfg.i64byteperm.idiom.byte.zext161 to i64
  %fuzz.cfg.i64byteperm.idiom.byte.shl163 = shl i64 %fuzz.cfg.i64byteperm.idiom.byte64162, 24
  %fuzz.cfg.i64byteperm.idiom.wide164 = or i64 %fuzz.cfg.i64byteperm.idiom.wide158, %fuzz.cfg.i64byteperm.idiom.byte.shl163
  %fuzz.cfg.i64byteperm.idiom.byte.shr165 = lshr i32 %fuzz.cfg.bytecmp.idiom.count.add, 8
  %fuzz.cfg.i64byteperm.idiom.byte.trunc166 = trunc i32 %fuzz.cfg.i64byteperm.idiom.byte.shr165 to i8
  %fuzz.cfg.i64byteperm.idiom.byte.zext167 = zext i8 %fuzz.cfg.i64byteperm.idiom.byte.trunc166 to i32
  %fuzz.cfg.i64byteperm.idiom.byte64168 = zext i32 %fuzz.cfg.i64byteperm.idiom.byte.zext167 to i64
  %fuzz.cfg.i64byteperm.idiom.byte.shl169 = shl i64 %fuzz.cfg.i64byteperm.idiom.byte64168, 0
  %fuzz.cfg.i64byteperm.idiom.wide170 = or i64 %fuzz.cfg.i64byteperm.idiom.wide164, %fuzz.cfg.i64byteperm.idiom.byte.shl169
  %fuzz.cfg.i64byteperm.idiom.byte.trunc171 = trunc i32 -1 to i8
  %fuzz.cfg.i64byteperm.idiom.byte.zext172 = zext i8 %fuzz.cfg.i64byteperm.idiom.byte.trunc171 to i32
  %fuzz.cfg.i64byteperm.idiom.byte64173 = zext i32 %fuzz.cfg.i64byteperm.idiom.byte.zext172 to i64
  %fuzz.cfg.i64byteperm.idiom.byte.shl174 = shl i64 %fuzz.cfg.i64byteperm.idiom.byte64173, 32
  %fuzz.cfg.i64byteperm.idiom.wide175 = or i64 %fuzz.cfg.i64byteperm.idiom.wide170, %fuzz.cfg.i64byteperm.idiom.byte.shl174
  %fuzz.cfg.i64byteperm.idiom.byte.shr176 = lshr i32 %fuzz.cfg.bytecmp.idiom.count.add, 8
  %fuzz.cfg.i64byteperm.idiom.byte.trunc177 = trunc i32 %fuzz.cfg.i64byteperm.idiom.byte.shr176 to i8
  %fuzz.cfg.i64byteperm.idiom.byte.zext178 = zext i8 %fuzz.cfg.i64byteperm.idiom.byte.trunc177 to i32
  %fuzz.cfg.i64byteperm.idiom.byte64179 = zext i32 %fuzz.cfg.i64byteperm.idiom.byte.zext178 to i64
  %fuzz.cfg.i64byteperm.idiom.byte.shl180 = shl i64 %fuzz.cfg.i64byteperm.idiom.byte64179, 8
  %fuzz.cfg.i64byteperm.idiom.wide181 = or i64 %fuzz.cfg.i64byteperm.idiom.wide175, %fuzz.cfg.i64byteperm.idiom.byte.shl180
  %fuzz.cfg.i64byteperm.idiom.byte.trunc182 = trunc i32 -1 to i8
  %fuzz.cfg.i64byteperm.idiom.byte.zext183 = zext i8 %fuzz.cfg.i64byteperm.idiom.byte.trunc182 to i32
  %fuzz.cfg.i64byteperm.idiom.byte64184 = zext i32 %fuzz.cfg.i64byteperm.idiom.byte.zext183 to i64
  %fuzz.cfg.i64byteperm.idiom.byte.shl185 = shl i64 %fuzz.cfg.i64byteperm.idiom.byte64184, 40
  %fuzz.cfg.i64byteperm.idiom.wide186 = or i64 %fuzz.cfg.i64byteperm.idiom.wide181, %fuzz.cfg.i64byteperm.idiom.byte.shl185
  %fuzz.cfg.i64byteperm.idiom.byte.trunc187 = trunc i32 %fuzz.cfg.bytecmp.idiom.count.add to i8
  %fuzz.cfg.i64byteperm.idiom.byte.zext188 = zext i8 %fuzz.cfg.i64byteperm.idiom.byte.trunc187 to i32
  %fuzz.cfg.i64byteperm.idiom.byte64189 = zext i32 %fuzz.cfg.i64byteperm.idiom.byte.zext188 to i64
  %fuzz.cfg.i64byteperm.idiom.byte.shl190 = shl i64 %fuzz.cfg.i64byteperm.idiom.byte64189, 48
  %fuzz.cfg.i64byteperm.idiom.wide191 = or i64 %fuzz.cfg.i64byteperm.idiom.wide186, %fuzz.cfg.i64byteperm.idiom.byte.shl190
  %fuzz.cfg.i64byteperm.idiom.bswap = call i64 @llvm.bswap.i64(i64 %fuzz.cfg.i64byteperm.idiom.wide191)
  %fuzz.cfg.i64byteperm.idiom.ctpop = call i64 @llvm.ctpop.i64(i64 %fuzz.cfg.i64byteperm.idiom.wide191)
  %fuzz.cfg.i64byteperm.idiom.pop.shl = shl i64 %fuzz.cfg.i64byteperm.idiom.ctpop, 8
  %fuzz.cfg.i64byteperm.idiom.add.pop = add i64 %fuzz.cfg.i64byteperm.idiom.wide191, %fuzz.cfg.i64byteperm.idiom.pop.shl
  %fuzz.cfg.i64byteperm.idiom.hi.shr = lshr i64 %fuzz.cfg.i64byteperm.idiom.add.pop, 32
  %fuzz.cfg.i64byteperm.idiom.hi = trunc i64 %fuzz.cfg.i64byteperm.idiom.hi.shr to i32
  %fuzz.cfg.i64byteperm.idiom.lo = trunc i64 %fuzz.cfg.i64byteperm.idiom.add.pop to i32
  %fuzz.cfg.i64byteperm.idiom.fold.xor = xor i32 %fuzz.cfg.i64byteperm.idiom.hi, %fuzz.cfg.i64byteperm.idiom.lo
  br label %fuzz.nested.join

fuzz.nested.else:                                 ; preds = %fuzz.nested.loop.body39
  %fuzz.cfg.swar.idiom.lowbits = and i32 %fuzz.cfg.i64pair.idiom.sel.fold, 286331153
  %fuzz.cfg.swar.idiom.scaled = mul i32 %fuzz.cfg.swar.idiom.lowbits, 252645135
  %fuzz.cfg.swar.idiom.scaled.high = lshr i32 %fuzz.cfg.swar.idiom.scaled, 28
  %fuzz.cfg.swar.idiom.scaled.mix = xor i32 %fuzz.cfg.swar.idiom.scaled.high, -1
  br label %fuzz.nested.join

fuzz.nested.join:                                 ; preds = %fuzz.nested.else, %fuzz.nested.then
  %fuzz.nested.phi = phi i32 [ %fuzz.cfg.i64byteperm.idiom.fold.xor, %fuzz.nested.then ], [ %fuzz.cfg.swar.idiom.scaled.mix, %fuzz.nested.else ]
  %fuzz.loop.next.inner192 = add i32 %fuzz.loop.iv.inner43, 1
  br label %fuzz.nested.loop.header38

fuzz.nested.loop.exit40:                          ; preds = %fuzz.nested.loop.header38
  %fuzz.loop.next = add i32 %fuzz.loop.iv, 1
  br label %fuzz.loop.header

fuzz.loop.exit:                                   ; preds = %fuzz.loop.header
  store i32 %fuzz.loop.acc, ptr addrspace(1) %out.ptr, align 4
  ret void

exit:                                             ; preds = %entry
  ret void
}

; Function Attrs: nocallback nofree nosync nounwind speculatable willreturn memory(none)
declare noundef i32 @llvm.amdgcn.workgroup.id.x() #1

; Function Attrs: nocallback nofree nosync nounwind speculatable willreturn memory(none)
declare noundef range(i32 0, 1024) i32 @llvm.amdgcn.workitem.id.x() #1

; Function Attrs: nocallback nofree nosync nounwind speculatable willreturn memory(none)
declare i32 @llvm.bitreverse.i32(i32) #2

; Function Attrs: nocallback nofree nosync nounwind speculatable willreturn memory(none)
declare { i32, i1 } @llvm.uadd.with.overflow.i32(i32, i32) #2

; Function Attrs: nocallback nofree nosync nounwind speculatable willreturn memory(none)
declare { i32, i1 } @llvm.usub.with.overflow.i32(i32, i32) #2

; Function Attrs: nocallback nofree nosync nounwind speculatable willreturn memory(none)
declare { i32, i1 } @llvm.sadd.with.overflow.i32(i32, i32) #2

; Function Attrs: nocallback nofree nosync nounwind speculatable willreturn memory(none)
declare i32 @llvm.ctpop.i32(i32) #2

; Function Attrs: nocallback nofree nosync nounwind speculatable willreturn memory(none)
declare i64 @llvm.ctpop.i64(i64) #2

; Function Attrs: nocallback nofree nosync nounwind speculatable willreturn memory(none)
declare i64 @llvm.bswap.i64(i64) #2

; uselistorder directives
uselistorder ptr @llvm.ctpop.i32, { 3, 2, 1, 0 }

attributes #0 = { convergent nounwind "amdgpu-flat-work-group-size"="1,256" "target-cpu"="gfx950" "uniform-work-group-size" }
attributes #1 = { nocallback nofree nosync nounwind speculatable willreturn memory(none) }
attributes #2 = { nocallback nofree nosync nounwind speculatable willreturn memory(none) }
