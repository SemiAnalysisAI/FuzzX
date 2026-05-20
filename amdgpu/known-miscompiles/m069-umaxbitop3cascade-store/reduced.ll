; RUN-LLVM-BUILD: build/llvm-fuzzer
; RUN-INPUTS: 0
; ModuleID = 'fuzzx-amdgpu-ir-bitcode'
source_filename = "/tmp/fuzzx-amdgpu-orenamd@semianalysis.com/head-pr198373-196418-198412-198419-gen13-20260519-085306/corpus/directed-gpu/shared/.seed-3996662.ll"
target datalayout = "e-m:e-p:64:64-p1:64:64-p2:32:32-p3:32:32-p4:64:64-p5:32:32-p6:32:32-p7:160:256:256:32-p8:128:128:128:48-p9:192:256:256:32-i64:64-v16:16-v24:32-v32:32-v48:64-v96:128-v192:256-v256:256-v512:512-v1024:1024-v2048:2048-n32:64-S32-A5-G1-ni:7:8:9"
target triple = "amdgcn-amd-amdhsa"

; Function Attrs: convergent nounwind
define protected amdgpu_kernel void @fuzz_kernel(ptr addrspace(1) %in, ptr addrspace(1) %out, i32 %n) #0 {
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
  %fuzz.umin = call i32 @llvm.umin.i32(i32 %mix, i32 -1)
  br label %fuzz.loop.multi.header

fuzz.loop.multi.header:                           ; preds = %fuzz.loop.multi.continue, %body
  %fuzz.loop.iv.multi = phi i32 [ 0, %body ], [ %fuzz.loop.multi.next, %fuzz.loop.multi.continue ]
  %fuzz.loop.acc.multi = phi i32 [ %fuzz.umin, %body ], [ %fuzz.loop.multi.acc.next, %fuzz.loop.multi.continue ]
  %fuzz.loop.multi.cond = icmp ult i32 %fuzz.loop.iv.multi, 5
  br i1 %fuzz.loop.multi.cond, label %fuzz.loop.multi.body, label %fuzz.loop.multi.exit

fuzz.loop.multi.body:                             ; preds = %fuzz.loop.multi.header
  %fuzz.cfg.i64pair.idiom.a64 = zext i32 %fuzz.loop.acc.multi to i64
  %fuzz.cfg.i64pair.idiom.b64 = zext i32 7 to i64
  %fuzz.cfg.i64pair.idiom.pair.hi = shl i64 %fuzz.cfg.i64pair.idiom.a64, 22
  %fuzz.cfg.i64pair.idiom.pair = or i64 %fuzz.cfg.i64pair.idiom.pair.hi, %fuzz.cfg.i64pair.idiom.b64
  %fuzz.cfg.i64pair.idiom.pair.add.c = add i64 %fuzz.cfg.i64pair.idiom.pair, 36028797018963967
  %fuzz.cfg.i64pair.idiom.pair.add.b = add i64 %fuzz.cfg.i64pair.idiom.pair.add.c, %fuzz.cfg.i64pair.idiom.b64
  %fuzz.cfg.i64pair.idiom.sum.hi.shr = lshr i64 %fuzz.cfg.i64pair.idiom.pair.add.b, 32
  %fuzz.cfg.i64pair.idiom.sum.hi.i32 = trunc i64 %fuzz.cfg.i64pair.idiom.sum.hi.shr to i32
  %fuzz.cfg.i64pair.idiom.sum.lo.i32 = trunc i64 %fuzz.cfg.i64pair.idiom.pair.add.b to i32
  %fuzz.cfg.i64pair.idiom.sum.fold = add i32 %fuzz.cfg.i64pair.idiom.sum.hi.i32, %fuzz.cfg.i64pair.idiom.sum.lo.i32
  %fuzz.cfg.bswap = call i32 @llvm.bswap.i32(i32 %fuzz.cfg.i64pair.idiom.sum.fold)
  %fuzz.loop.multi.exit.key = and i32 %fuzz.cfg.bswap, 3
  switch i32 %fuzz.loop.multi.exit.key, label %fuzz.loop.multi.continue [
    i32 0, label %fuzz.loop.multi.break.a
    i32 1, label %fuzz.loop.multi.break.b
  ]

fuzz.loop.multi.break.a:                          ; preds = %fuzz.loop.multi.body
  %fuzz.loop.multi.break.a.val = xor i32 %fuzz.cfg.bswap, %fuzz.loop.iv.multi
  br label %fuzz.loop.multi.exit

fuzz.loop.multi.break.b:                          ; preds = %fuzz.loop.multi.body
  %fuzz.loop.multi.break.b.val = add i32 %fuzz.cfg.bswap, 1
  br label %fuzz.loop.multi.exit

fuzz.loop.multi.continue:                         ; preds = %fuzz.loop.multi.body
  %fuzz.loop.multi.acc.next = xor i32 %fuzz.cfg.bswap, 1
  %fuzz.loop.multi.next = add i32 %fuzz.loop.iv.multi, 1
  br label %fuzz.loop.multi.header

fuzz.loop.multi.exit:                             ; preds = %fuzz.loop.multi.break.b, %fuzz.loop.multi.break.a, %fuzz.loop.multi.header
  %fuzz.loop.multi.exit.value = phi i32 [ %fuzz.loop.acc.multi, %fuzz.loop.multi.header ], [ %fuzz.loop.multi.break.a.val, %fuzz.loop.multi.break.a ], [ %fuzz.loop.multi.break.b.val, %fuzz.loop.multi.break.b ]
  %fuzz.overflow.call = call { i32, i1 } @llvm.smul.with.overflow.i32(i32 %fuzz.loop.multi.exit.value, i32 3)
  %fuzz.overflow.value = extractvalue { i32, i1 } %fuzz.overflow.call, 0
  %fuzz.overflow.overflow = extractvalue { i32, i1 } %fuzz.overflow.call, 1
  %fuzz.overflow.overflow.i32 = zext i1 %fuzz.overflow.overflow to i32
  %fuzz.overflow.xor = xor i32 %fuzz.overflow.value, %fuzz.overflow.overflow.i32
  br label %fuzz.loop.nest.header

fuzz.loop.nest.header:                            ; preds = %fuzz.nested.loop.exit, %fuzz.loop.multi.exit
  %fuzz.loop.nest.iv = phi i32 [ 0, %fuzz.loop.multi.exit ], [ %fuzz.loop.nest.next, %fuzz.nested.loop.exit ]
  %fuzz.loop.nest.acc = phi i32 [ %fuzz.overflow.xor, %fuzz.loop.multi.exit ], [ %fuzz.loop.acc.inner, %fuzz.nested.loop.exit ]
  %fuzz.loop.nest.cond = icmp ult i32 %fuzz.loop.nest.iv, 3
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
  %fuzz.cfg.bitrunmask.idiom.count.a.add = add i32 %fuzz.loop.acc.inner, 27
  %fuzz.cfg.bitrunmask.idiom.count.a = and i32 %fuzz.cfg.bitrunmask.idiom.count.a.add, 31
  %fuzz.cfg.bitrunmask.idiom.count.b.xor = xor i32 %fuzz.loop.multi.exit.value, 18
  %fuzz.cfg.bitrunmask.idiom.count.b = and i32 %fuzz.cfg.bitrunmask.idiom.count.b.xor, 31
  %fuzz.cfg.bitrunmask.idiom.run.a.shl = shl i32 1, %fuzz.cfg.bitrunmask.idiom.count.a
  %fuzz.cfg.bitrunmask.idiom.run.a = sub i32 %fuzz.cfg.bitrunmask.idiom.run.a.shl, 1
  %fuzz.cfg.bitrunmask.idiom.run.b.shl = shl i32 2147483647, %fuzz.cfg.bitrunmask.idiom.count.b
  %fuzz.cfg.bitrunmask.idiom.run.b = sub i32 %fuzz.cfg.bitrunmask.idiom.run.b.shl, 1
  %fuzz.cfg.bitrunmask.idiom.inv.a.raw = sub i32 32, %fuzz.cfg.bitrunmask.idiom.count.a
  %fuzz.cfg.bitrunmask.idiom.inv.a = and i32 %fuzz.cfg.bitrunmask.idiom.inv.a.raw, 31
  %fuzz.cfg.bitrunmask.idiom.left = shl i32 %fuzz.loop.acc.inner, %fuzz.cfg.bitrunmask.idiom.count.b
  %fuzz.cfg.bitrunmask.idiom.right = lshr i32 %fuzz.loop.multi.exit.value, %fuzz.cfg.bitrunmask.idiom.inv.a
  %fuzz.cfg.bitrunmask.idiom.window = or i32 %fuzz.cfg.bitrunmask.idiom.left, %fuzz.cfg.bitrunmask.idiom.right
  %fuzz.cfg.bitrunmask.idiom.masked.a = and i32 %fuzz.cfg.bitrunmask.idiom.window, %fuzz.cfg.bitrunmask.idiom.run.a
  %fuzz.cfg.bitrunmask.idiom.window.xor = xor i32 %fuzz.cfg.bitrunmask.idiom.window, %fuzz.loop.multi.exit.value
  %fuzz.cfg.bitrunmask.idiom.masked.b = and i32 %fuzz.cfg.bitrunmask.idiom.window.xor, %fuzz.cfg.bitrunmask.idiom.run.b
  %fuzz.cfg.bitrunmask.idiom.lt = icmp ult i32 %fuzz.cfg.bitrunmask.idiom.count.a, %fuzz.cfg.bitrunmask.idiom.count.b
  %fuzz.cfg.bitrunmask.idiom.select = select i1 %fuzz.cfg.bitrunmask.idiom.lt, i32 %fuzz.cfg.bitrunmask.idiom.masked.a, i32 %fuzz.cfg.bitrunmask.idiom.masked.b
  %fuzz.cfg.bitrunmask.idiom.sub = sub i32 %fuzz.cfg.bitrunmask.idiom.window, %fuzz.cfg.bitrunmask.idiom.select
  %fuzz.loop.acc.inner.mix = xor i32 %fuzz.cfg.bitrunmask.idiom.sub, %fuzz.loop.iv.inner
  %fuzz.loop.next.inner = add i32 %fuzz.loop.iv.inner, 1
  br label %fuzz.nested.loop.header

fuzz.nested.loop.exit:                            ; preds = %fuzz.nested.loop.header
  %fuzz.loop.nest.next = add i32 %fuzz.loop.nest.iv, 1
  br label %fuzz.loop.nest.header

fuzz.loop.nest.exit:                              ; preds = %fuzz.loop.nest.header
  %fuzz.veci16selectcarry.idiom.a.half.trunc = trunc i32 %fuzz.loop.nest.acc to i16
  %fuzz.veci16selectcarry.idiom.a.half.zext = zext i16 %fuzz.veci16selectcarry.idiom.a.half.trunc to i32
  %fuzz.veci16selectcarry.idiom.b.half.shr = lshr i32 2147483647, 16
  %fuzz.veci16selectcarry.idiom.b.half.trunc = trunc i32 %fuzz.veci16selectcarry.idiom.b.half.shr to i16
  %fuzz.veci16selectcarry.idiom.b.half.zext = zext i16 %fuzz.veci16selectcarry.idiom.b.half.trunc to i32
  %fuzz.veci16selectcarry.idiom.sum = add i32 %fuzz.veci16selectcarry.idiom.a.half.zext, %fuzz.veci16selectcarry.idiom.b.half.zext
  %fuzz.veci16selectcarry.idiom.carry.seed = lshr i32 %fuzz.veci16selectcarry.idiom.sum, 16
  %fuzz.veci16selectcarry.idiom.carry.shl = shl i32 %fuzz.veci16selectcarry.idiom.carry.seed, 0
  %fuzz.veci16selectcarry.idiom.a.mix = xor i32 %fuzz.veci16selectcarry.idiom.a.half.zext, %fuzz.veci16selectcarry.idiom.carry.shl
  %fuzz.veci16selectcarry.idiom.a.trunc = trunc i32 %fuzz.veci16selectcarry.idiom.a.mix to i16
  %fuzz.veci16selectcarry.idiom.b.mix = add i32 %fuzz.veci16selectcarry.idiom.b.half.zext, 42143
  %fuzz.veci16selectcarry.idiom.b.trunc = trunc i32 %fuzz.veci16selectcarry.idiom.b.mix to i16
  %fuzz.veci16selectcarry.idiom.a.half.shr = lshr i32 %fuzz.loop.nest.acc, 16
  %fuzz.veci16selectcarry.idiom.a.half.trunc1 = trunc i32 %fuzz.veci16selectcarry.idiom.a.half.shr to i16
  %fuzz.veci16selectcarry.idiom.a.half.zext2 = zext i16 %fuzz.veci16selectcarry.idiom.a.half.trunc1 to i32
  %fuzz.veci16selectcarry.idiom.b.half.trunc3 = trunc i32 %fuzz.loop.nest.acc to i16
  %fuzz.veci16selectcarry.idiom.b.half.zext4 = zext i16 %fuzz.veci16selectcarry.idiom.b.half.trunc3 to i32
  %fuzz.veci16selectcarry.idiom.sum5 = add i32 %fuzz.veci16selectcarry.idiom.a.half.zext2, %fuzz.veci16selectcarry.idiom.b.half.zext4
  %fuzz.veci16selectcarry.idiom.carry.seed6 = lshr i32 %fuzz.veci16selectcarry.idiom.sum5, 16
  %fuzz.veci16selectcarry.idiom.carry.shl7 = shl i32 %fuzz.veci16selectcarry.idiom.carry.seed6, 1
  %fuzz.veci16selectcarry.idiom.a.mix8 = xor i32 %fuzz.veci16selectcarry.idiom.a.half.zext2, %fuzz.veci16selectcarry.idiom.carry.shl7
  %fuzz.veci16selectcarry.idiom.a.trunc9 = trunc i32 %fuzz.veci16selectcarry.idiom.a.mix8 to i16
  %fuzz.veci16selectcarry.idiom.b.mix10 = add i32 %fuzz.veci16selectcarry.idiom.b.half.zext4, 12505
  %fuzz.veci16selectcarry.idiom.b.trunc11 = trunc i32 %fuzz.veci16selectcarry.idiom.b.mix10 to i16
  %fuzz.veci16selectcarry.idiom.a.half.trunc12 = trunc i32 2147483647 to i16
  %fuzz.veci16selectcarry.idiom.a.half.zext13 = zext i16 %fuzz.veci16selectcarry.idiom.a.half.trunc12 to i32
  %fuzz.veci16selectcarry.idiom.b.half.shr14 = lshr i32 2147483647, 16
  %fuzz.veci16selectcarry.idiom.b.half.trunc15 = trunc i32 %fuzz.veci16selectcarry.idiom.b.half.shr14 to i16
  %fuzz.veci16selectcarry.idiom.b.half.zext16 = zext i16 %fuzz.veci16selectcarry.idiom.b.half.trunc15 to i32
  %fuzz.veci16selectcarry.idiom.sum17 = add i32 %fuzz.veci16selectcarry.idiom.a.half.zext13, %fuzz.veci16selectcarry.idiom.b.half.zext16
  %fuzz.veci16selectcarry.idiom.carry.seed18 = lshr i32 %fuzz.veci16selectcarry.idiom.sum17, 16
  %fuzz.veci16selectcarry.idiom.carry.shl19 = shl i32 %fuzz.veci16selectcarry.idiom.carry.seed18, 2
  %fuzz.veci16selectcarry.idiom.a.mix20 = xor i32 %fuzz.veci16selectcarry.idiom.a.half.zext13, %fuzz.veci16selectcarry.idiom.carry.shl19
  %fuzz.veci16selectcarry.idiom.a.trunc21 = trunc i32 %fuzz.veci16selectcarry.idiom.a.mix20 to i16
  %fuzz.veci16selectcarry.idiom.b.mix22 = add i32 %fuzz.veci16selectcarry.idiom.b.half.zext16, 49246
  %fuzz.veci16selectcarry.idiom.b.trunc23 = trunc i32 %fuzz.veci16selectcarry.idiom.b.mix22 to i16
  %fuzz.veci16selectcarry.idiom.a.half.shr24 = lshr i32 2147483647, 16
  %fuzz.veci16selectcarry.idiom.a.half.trunc25 = trunc i32 %fuzz.veci16selectcarry.idiom.a.half.shr24 to i16
  %fuzz.veci16selectcarry.idiom.a.half.zext26 = zext i16 %fuzz.veci16selectcarry.idiom.a.half.trunc25 to i32
  %fuzz.veci16selectcarry.idiom.b.half.trunc27 = trunc i32 %fuzz.loop.nest.acc to i16
  %fuzz.veci16selectcarry.idiom.b.half.zext28 = zext i16 %fuzz.veci16selectcarry.idiom.b.half.trunc27 to i32
  %fuzz.veci16selectcarry.idiom.sum29 = add i32 %fuzz.veci16selectcarry.idiom.a.half.zext26, %fuzz.veci16selectcarry.idiom.b.half.zext28
  %fuzz.veci16selectcarry.idiom.carry.seed30 = lshr i32 %fuzz.veci16selectcarry.idiom.sum29, 16
  %fuzz.veci16selectcarry.idiom.carry.shl31 = shl i32 %fuzz.veci16selectcarry.idiom.carry.seed30, 3
  %fuzz.veci16selectcarry.idiom.a.mix32 = xor i32 %fuzz.veci16selectcarry.idiom.a.half.zext26, %fuzz.veci16selectcarry.idiom.carry.shl31
  %fuzz.veci16selectcarry.idiom.a.trunc33 = trunc i32 %fuzz.veci16selectcarry.idiom.a.mix32 to i16
  %fuzz.veci16selectcarry.idiom.b.mix34 = add i32 %fuzz.veci16selectcarry.idiom.b.half.zext28, 42440
  %fuzz.veci16selectcarry.idiom.b.trunc35 = trunc i32 %fuzz.veci16selectcarry.idiom.b.mix34 to i16
  %fuzz.veci16selectcarry.idiom.a.half.trunc36 = trunc i32 %fuzz.loop.nest.acc to i16
  %fuzz.veci16selectcarry.idiom.a.half.zext37 = zext i16 %fuzz.veci16selectcarry.idiom.a.half.trunc36 to i32
  %fuzz.veci16selectcarry.idiom.b.half.shr38 = lshr i32 2147483647, 16
  %fuzz.veci16selectcarry.idiom.b.half.trunc39 = trunc i32 %fuzz.veci16selectcarry.idiom.b.half.shr38 to i16
  %fuzz.veci16selectcarry.idiom.b.half.zext40 = zext i16 %fuzz.veci16selectcarry.idiom.b.half.trunc39 to i32
  %fuzz.veci16selectcarry.idiom.sum41 = add i32 %fuzz.veci16selectcarry.idiom.a.half.zext37, %fuzz.veci16selectcarry.idiom.b.half.zext40
  %fuzz.veci16selectcarry.idiom.carry.seed42 = lshr i32 %fuzz.veci16selectcarry.idiom.sum41, 16
  %fuzz.veci16selectcarry.idiom.carry.shl43 = shl i32 %fuzz.veci16selectcarry.idiom.carry.seed42, 4
  %fuzz.veci16selectcarry.idiom.a.mix44 = xor i32 %fuzz.veci16selectcarry.idiom.a.half.zext37, %fuzz.veci16selectcarry.idiom.carry.shl43
  %fuzz.veci16selectcarry.idiom.a.trunc45 = trunc i32 %fuzz.veci16selectcarry.idiom.a.mix44 to i16
  %fuzz.veci16selectcarry.idiom.b.mix46 = add i32 %fuzz.veci16selectcarry.idiom.b.half.zext40, 7771
  %fuzz.veci16selectcarry.idiom.b.trunc47 = trunc i32 %fuzz.veci16selectcarry.idiom.b.mix46 to i16
  %fuzz.veci16selectcarry.idiom.a.half.shr48 = lshr i32 %fuzz.loop.nest.acc, 16
  %fuzz.veci16selectcarry.idiom.a.half.trunc49 = trunc i32 %fuzz.veci16selectcarry.idiom.a.half.shr48 to i16
  %fuzz.veci16selectcarry.idiom.a.half.zext50 = zext i16 %fuzz.veci16selectcarry.idiom.a.half.trunc49 to i32
  %fuzz.veci16selectcarry.idiom.b.half.trunc51 = trunc i32 %fuzz.loop.nest.acc to i16
  %fuzz.veci16selectcarry.idiom.b.half.zext52 = zext i16 %fuzz.veci16selectcarry.idiom.b.half.trunc51 to i32
  %fuzz.veci16selectcarry.idiom.sum53 = add i32 %fuzz.veci16selectcarry.idiom.a.half.zext50, %fuzz.veci16selectcarry.idiom.b.half.zext52
  %fuzz.veci16selectcarry.idiom.carry.seed54 = lshr i32 %fuzz.veci16selectcarry.idiom.sum53, 16
  %fuzz.veci16selectcarry.idiom.carry.shl55 = shl i32 %fuzz.veci16selectcarry.idiom.carry.seed54, 5
  %fuzz.veci16selectcarry.idiom.a.mix56 = xor i32 %fuzz.veci16selectcarry.idiom.a.half.zext50, %fuzz.veci16selectcarry.idiom.carry.shl55
  %fuzz.veci16selectcarry.idiom.a.trunc57 = trunc i32 %fuzz.veci16selectcarry.idiom.a.mix56 to i16
  %fuzz.veci16selectcarry.idiom.b.mix58 = add i32 %fuzz.veci16selectcarry.idiom.b.half.zext52, 47937
  %fuzz.veci16selectcarry.idiom.b.trunc59 = trunc i32 %fuzz.veci16selectcarry.idiom.b.mix58 to i16
  %fuzz.veci16selectcarry.idiom.a.half.trunc60 = trunc i32 2147483647 to i16
  %fuzz.veci16selectcarry.idiom.a.half.zext61 = zext i16 %fuzz.veci16selectcarry.idiom.a.half.trunc60 to i32
  %fuzz.veci16selectcarry.idiom.b.half.shr62 = lshr i32 2147483647, 16
  %fuzz.veci16selectcarry.idiom.b.half.trunc63 = trunc i32 %fuzz.veci16selectcarry.idiom.b.half.shr62 to i16
  %fuzz.veci16selectcarry.idiom.b.half.zext64 = zext i16 %fuzz.veci16selectcarry.idiom.b.half.trunc63 to i32
  %fuzz.veci16selectcarry.idiom.sum65 = add i32 %fuzz.veci16selectcarry.idiom.a.half.zext61, %fuzz.veci16selectcarry.idiom.b.half.zext64
  %fuzz.veci16selectcarry.idiom.carry.seed66 = lshr i32 %fuzz.veci16selectcarry.idiom.sum65, 16
  %fuzz.veci16selectcarry.idiom.carry.shl67 = shl i32 %fuzz.veci16selectcarry.idiom.carry.seed66, 6
  %fuzz.veci16selectcarry.idiom.a.mix68 = xor i32 %fuzz.veci16selectcarry.idiom.a.half.zext61, %fuzz.veci16selectcarry.idiom.carry.shl67
  %fuzz.veci16selectcarry.idiom.a.trunc69 = trunc i32 %fuzz.veci16selectcarry.idiom.a.mix68 to i16
  %fuzz.veci16selectcarry.idiom.b.mix70 = add i32 %fuzz.veci16selectcarry.idiom.b.half.zext64, 13492
  %fuzz.veci16selectcarry.idiom.b.trunc71 = trunc i32 %fuzz.veci16selectcarry.idiom.b.mix70 to i16
  %fuzz.veci16selectcarry.idiom.a.half.shr72 = lshr i32 2147483647, 16
  %fuzz.veci16selectcarry.idiom.a.half.trunc73 = trunc i32 %fuzz.veci16selectcarry.idiom.a.half.shr72 to i16
  %fuzz.veci16selectcarry.idiom.a.half.zext74 = zext i16 %fuzz.veci16selectcarry.idiom.a.half.trunc73 to i32
  %fuzz.veci16selectcarry.idiom.b.half.trunc75 = trunc i32 %fuzz.loop.nest.acc to i16
  %fuzz.veci16selectcarry.idiom.b.half.zext76 = zext i16 %fuzz.veci16selectcarry.idiom.b.half.trunc75 to i32
  %fuzz.veci16selectcarry.idiom.sum77 = add i32 %fuzz.veci16selectcarry.idiom.a.half.zext74, %fuzz.veci16selectcarry.idiom.b.half.zext76
  %fuzz.veci16selectcarry.idiom.carry.seed78 = lshr i32 %fuzz.veci16selectcarry.idiom.sum77, 16
  %fuzz.veci16selectcarry.idiom.carry.shl79 = shl i32 %fuzz.veci16selectcarry.idiom.carry.seed78, 7
  %fuzz.veci16selectcarry.idiom.a.mix80 = xor i32 %fuzz.veci16selectcarry.idiom.a.half.zext74, %fuzz.veci16selectcarry.idiom.carry.shl79
  %fuzz.veci16selectcarry.idiom.a.trunc81 = trunc i32 %fuzz.veci16selectcarry.idiom.a.mix80 to i16
  %fuzz.veci16selectcarry.idiom.b.mix82 = add i32 %fuzz.veci16selectcarry.idiom.b.half.zext76, 38357
  %fuzz.veci16selectcarry.idiom.b.trunc83 = trunc i32 %fuzz.veci16selectcarry.idiom.b.mix82 to i16
  %fuzz.vec.ins = insertelement <8 x i16> zeroinitializer, i16 %fuzz.veci16selectcarry.idiom.a.trunc, i32 0
  %fuzz.vec.ins84 = insertelement <8 x i16> %fuzz.vec.ins, i16 %fuzz.veci16selectcarry.idiom.a.trunc9, i32 1
  %fuzz.vec.ins85 = insertelement <8 x i16> %fuzz.vec.ins84, i16 %fuzz.veci16selectcarry.idiom.a.trunc21, i32 2
  %fuzz.vec.ins86 = insertelement <8 x i16> %fuzz.vec.ins85, i16 %fuzz.veci16selectcarry.idiom.a.trunc33, i32 3
  %fuzz.vec.ins87 = insertelement <8 x i16> %fuzz.vec.ins86, i16 %fuzz.veci16selectcarry.idiom.a.trunc45, i32 4
  %fuzz.vec.ins88 = insertelement <8 x i16> %fuzz.vec.ins87, i16 %fuzz.veci16selectcarry.idiom.a.trunc57, i32 5
  %fuzz.vec.ins89 = insertelement <8 x i16> %fuzz.vec.ins88, i16 %fuzz.veci16selectcarry.idiom.a.trunc69, i32 6
  %fuzz.vec.ins90 = insertelement <8 x i16> %fuzz.vec.ins89, i16 %fuzz.veci16selectcarry.idiom.a.trunc81, i32 7
  %fuzz.vec.ins91 = insertelement <8 x i16> zeroinitializer, i16 %fuzz.veci16selectcarry.idiom.b.trunc, i32 0
  %fuzz.vec.ins92 = insertelement <8 x i16> %fuzz.vec.ins91, i16 %fuzz.veci16selectcarry.idiom.b.trunc11, i32 1
  %fuzz.vec.ins93 = insertelement <8 x i16> %fuzz.vec.ins92, i16 %fuzz.veci16selectcarry.idiom.b.trunc23, i32 2
  %fuzz.vec.ins94 = insertelement <8 x i16> %fuzz.vec.ins93, i16 %fuzz.veci16selectcarry.idiom.b.trunc35, i32 3
  %fuzz.vec.ins95 = insertelement <8 x i16> %fuzz.vec.ins94, i16 %fuzz.veci16selectcarry.idiom.b.trunc47, i32 4
  %fuzz.vec.ins96 = insertelement <8 x i16> %fuzz.vec.ins95, i16 %fuzz.veci16selectcarry.idiom.b.trunc59, i32 5
  %fuzz.vec.ins97 = insertelement <8 x i16> %fuzz.vec.ins96, i16 %fuzz.veci16selectcarry.idiom.b.trunc71, i32 6
  %fuzz.vec.ins98 = insertelement <8 x i16> %fuzz.vec.ins97, i16 %fuzz.veci16selectcarry.idiom.b.trunc83, i32 7
  %fuzz.veci16selectcarry.idiom.rot = shufflevector <8 x i16> %fuzz.vec.ins90, <8 x i16> %fuzz.vec.ins90, <8 x i32> <i32 1, i32 2, i32 3, i32 4, i32 5, i32 6, i32 7, i32 0>
  %fuzz.veci16selectcarry.idiom.rev = shufflevector <8 x i16> %fuzz.vec.ins98, <8 x i16> %fuzz.vec.ins98, <8 x i32> <i32 7, i32 6, i32 5, i32 4, i32 3, i32 2, i32 1, i32 0>
  %fuzz.veci16selectcarry.idiom.cmp = icmp ugt <8 x i16> %fuzz.veci16selectcarry.idiom.rot, %fuzz.veci16selectcarry.idiom.rev
  %fuzz.veci16selectcarry.idiom.hi = select <8 x i1> %fuzz.veci16selectcarry.idiom.cmp, <8 x i16> %fuzz.veci16selectcarry.idiom.rot, <8 x i16> %fuzz.veci16selectcarry.idiom.rev
  %fuzz.veci16selectcarry.idiom.lo = select <8 x i1> %fuzz.veci16selectcarry.idiom.cmp, <8 x i16> %fuzz.veci16selectcarry.idiom.rev, <8 x i16> %fuzz.veci16selectcarry.idiom.rot
  %fuzz.veci16selectcarry.idiom.lo.shr = lshr <8 x i16> %fuzz.veci16selectcarry.idiom.lo, <i16 3, i16 7, i16 8, i16 2, i16 5, i16 15, i16 8, i16 13>
  %fuzz.veci16selectcarry.idiom.mixed.xor = xor <8 x i16> %fuzz.veci16selectcarry.idiom.hi, %fuzz.veci16selectcarry.idiom.lo.shr
  %fuzz.veci16selectcarry.idiom.lane = extractelement <8 x i16> %fuzz.veci16selectcarry.idiom.mixed.xor, i32 0
  %fuzz.veci16selectcarry.idiom.lane.zext = zext i16 %fuzz.veci16selectcarry.idiom.lane to i32
  %fuzz.veci16selectcarry.idiom.fold.xor = xor i32 0, %fuzz.veci16selectcarry.idiom.lane.zext
  %fuzz.veci16selectcarry.idiom.fold = add i32 %fuzz.veci16selectcarry.idiom.fold.xor, 0
  %fuzz.veci16selectcarry.idiom.lane99 = extractelement <8 x i16> %fuzz.veci16selectcarry.idiom.mixed.xor, i32 1
  %fuzz.veci16selectcarry.idiom.lane.zext100 = zext i16 %fuzz.veci16selectcarry.idiom.lane99 to i32
  %fuzz.veci16selectcarry.idiom.fold.xor101 = xor i32 %fuzz.veci16selectcarry.idiom.fold, %fuzz.veci16selectcarry.idiom.lane.zext100
  %fuzz.veci16selectcarry.idiom.fold102 = add i32 %fuzz.veci16selectcarry.idiom.fold.xor101, 273
  %fuzz.veci16selectcarry.idiom.byte = and i32 %fuzz.veci16selectcarry.idiom.fold102, 255
  %fuzz.veci16selectcarry.idiom.lane103 = extractelement <8 x i16> %fuzz.veci16selectcarry.idiom.mixed.xor, i32 2
  %fuzz.veci16selectcarry.idiom.lane.zext104 = zext i16 %fuzz.veci16selectcarry.idiom.lane103 to i32
  %fuzz.veci16selectcarry.idiom.fold.xor105 = xor i32 %fuzz.veci16selectcarry.idiom.fold102, %fuzz.veci16selectcarry.idiom.lane.zext104
  %fuzz.veci16selectcarry.idiom.fold106 = add i32 %fuzz.veci16selectcarry.idiom.fold.xor105, 546
  %fuzz.veci16selectcarry.idiom.lane107 = extractelement <8 x i16> %fuzz.veci16selectcarry.idiom.mixed.xor, i32 3
  %fuzz.veci16selectcarry.idiom.lane.zext108 = zext i16 %fuzz.veci16selectcarry.idiom.lane107 to i32
  %fuzz.veci16selectcarry.idiom.fold.xor109 = xor i32 %fuzz.veci16selectcarry.idiom.fold106, %fuzz.veci16selectcarry.idiom.lane.zext108
  %fuzz.veci16selectcarry.idiom.fold110 = add i32 %fuzz.veci16selectcarry.idiom.fold.xor109, 819
  %fuzz.veci16selectcarry.idiom.byte111 = and i32 %fuzz.veci16selectcarry.idiom.fold110, 255
  %fuzz.veci16selectcarry.idiom.lane112 = extractelement <8 x i16> %fuzz.veci16selectcarry.idiom.mixed.xor, i32 4
  %fuzz.veci16selectcarry.idiom.lane.zext113 = zext i16 %fuzz.veci16selectcarry.idiom.lane112 to i32
  %fuzz.veci16selectcarry.idiom.fold.xor114 = xor i32 %fuzz.veci16selectcarry.idiom.fold110, %fuzz.veci16selectcarry.idiom.lane.zext113
  %fuzz.veci16selectcarry.idiom.fold115 = add i32 %fuzz.veci16selectcarry.idiom.fold.xor114, 1092
  %fuzz.veci16selectcarry.idiom.lane116 = extractelement <8 x i16> %fuzz.veci16selectcarry.idiom.mixed.xor, i32 5
  %fuzz.veci16selectcarry.idiom.lane.zext117 = zext i16 %fuzz.veci16selectcarry.idiom.lane116 to i32
  %fuzz.veci16selectcarry.idiom.fold.xor118 = xor i32 %fuzz.veci16selectcarry.idiom.fold115, %fuzz.veci16selectcarry.idiom.lane.zext117
  %fuzz.veci16selectcarry.idiom.fold119 = add i32 %fuzz.veci16selectcarry.idiom.fold.xor118, 1365
  %fuzz.veci16selectcarry.idiom.byte120 = and i32 %fuzz.veci16selectcarry.idiom.fold119, 255
  %fuzz.veci16selectcarry.idiom.lane121 = extractelement <8 x i16> %fuzz.veci16selectcarry.idiom.mixed.xor, i32 6
  %fuzz.veci16selectcarry.idiom.lane.zext122 = zext i16 %fuzz.veci16selectcarry.idiom.lane121 to i32
  %fuzz.veci16selectcarry.idiom.fold.xor123 = xor i32 %fuzz.veci16selectcarry.idiom.fold119, %fuzz.veci16selectcarry.idiom.lane.zext122
  %fuzz.veci16selectcarry.idiom.fold124 = add i32 %fuzz.veci16selectcarry.idiom.fold.xor123, 1638
  %fuzz.veci16selectcarry.idiom.lane125 = extractelement <8 x i16> %fuzz.veci16selectcarry.idiom.mixed.xor, i32 7
  %fuzz.veci16selectcarry.idiom.lane.zext126 = zext i16 %fuzz.veci16selectcarry.idiom.lane125 to i32
  %fuzz.veci16selectcarry.idiom.fold.xor127 = xor i32 %fuzz.veci16selectcarry.idiom.fold124, %fuzz.veci16selectcarry.idiom.lane.zext126
  %fuzz.veci16selectcarry.idiom.fold128 = add i32 %fuzz.veci16selectcarry.idiom.fold.xor127, 1911
  %fuzz.veci16selectcarry.idiom.byte129 = and i32 %fuzz.veci16selectcarry.idiom.fold128, 255
  %fuzz.veci16selectcarry.idiom.pack.mask = and i32 %fuzz.veci16selectcarry.idiom.byte, 255
  %fuzz.veci16selectcarry.idiom.pack.or = or i32 0, %fuzz.veci16selectcarry.idiom.pack.mask
  %fuzz.veci16selectcarry.idiom.pack.mask130 = and i32 %fuzz.veci16selectcarry.idiom.byte111, 255
  %fuzz.veci16selectcarry.idiom.pack.shift = shl i32 %fuzz.veci16selectcarry.idiom.pack.mask130, 8
  %fuzz.veci16selectcarry.idiom.pack.or131 = or i32 %fuzz.veci16selectcarry.idiom.pack.or, %fuzz.veci16selectcarry.idiom.pack.shift
  %fuzz.veci16selectcarry.idiom.pack.mask132 = and i32 %fuzz.veci16selectcarry.idiom.byte120, 255
  %fuzz.veci16selectcarry.idiom.pack.shift133 = shl i32 %fuzz.veci16selectcarry.idiom.pack.mask132, 16
  %fuzz.veci16selectcarry.idiom.pack.or134 = or i32 %fuzz.veci16selectcarry.idiom.pack.or131, %fuzz.veci16selectcarry.idiom.pack.shift133
  %fuzz.veci16selectcarry.idiom.pack.mask135 = and i32 %fuzz.veci16selectcarry.idiom.byte129, 255
  %fuzz.veci16selectcarry.idiom.pack.shift136 = shl i32 %fuzz.veci16selectcarry.idiom.pack.mask135, 24
  %fuzz.veci16selectcarry.idiom.pack.or137 = or i32 %fuzz.veci16selectcarry.idiom.pack.or134, %fuzz.veci16selectcarry.idiom.pack.shift136
  %fuzz.veci16selectcarry.idiom.fold.xor138 = xor i32 %fuzz.veci16selectcarry.idiom.pack.or137, %fuzz.veci16selectcarry.idiom.fold128
  %fuzz.i64bitinterleave.idiom.a.lo = and i32 %fuzz.veci16selectcarry.idiom.fold.xor138, 65535
  %fuzz.i64bitinterleave.idiom.b.lo = and i32 %fuzz.veci16selectcarry.idiom.b.mix58, 65535
  %fuzz.i64bitinterleave.idiom.a.shr = lshr i32 %fuzz.i64bitinterleave.idiom.a.lo, 0
  %fuzz.i64bitinterleave.idiom.a.bit = and i32 %fuzz.i64bitinterleave.idiom.a.shr, 1
  %fuzz.i64bitinterleave.idiom.b.shr = lshr i32 %fuzz.i64bitinterleave.idiom.b.lo, 0
  %fuzz.i64bitinterleave.idiom.b.bit = and i32 %fuzz.i64bitinterleave.idiom.b.shr, 1
  %fuzz.i64bitinterleave.idiom.a.shl = shl i32 %fuzz.i64bitinterleave.idiom.a.bit, 0
  %fuzz.i64bitinterleave.idiom.b.shl = shl i32 %fuzz.i64bitinterleave.idiom.b.bit, 1
  %fuzz.i64bitinterleave.idiom.pair = or i32 %fuzz.i64bitinterleave.idiom.a.shl, %fuzz.i64bitinterleave.idiom.b.shl
  %fuzz.i64bitinterleave.idiom.accumulate = or i32 0, %fuzz.i64bitinterleave.idiom.pair
  %fuzz.i64bitinterleave.idiom.a.shr1 = lshr i32 %fuzz.i64bitinterleave.idiom.a.lo, 1
  %fuzz.i64bitinterleave.idiom.a.bit2 = and i32 %fuzz.i64bitinterleave.idiom.a.shr1, 1
  %fuzz.i64bitinterleave.idiom.b.shr3 = lshr i32 %fuzz.i64bitinterleave.idiom.b.lo, 1
  %fuzz.i64bitinterleave.idiom.b.bit4 = and i32 %fuzz.i64bitinterleave.idiom.b.shr3, 1
  %fuzz.i64bitinterleave.idiom.a.shl5 = shl i32 %fuzz.i64bitinterleave.idiom.a.bit2, 2
  %fuzz.i64bitinterleave.idiom.b.shl6 = shl i32 %fuzz.i64bitinterleave.idiom.b.bit4, 3
  %fuzz.i64bitinterleave.idiom.pair7 = or i32 %fuzz.i64bitinterleave.idiom.a.shl5, %fuzz.i64bitinterleave.idiom.b.shl6
  %fuzz.i64bitinterleave.idiom.accumulate8 = or i32 %fuzz.i64bitinterleave.idiom.accumulate, %fuzz.i64bitinterleave.idiom.pair7
  %fuzz.i64bitinterleave.idiom.a.shr9 = lshr i32 %fuzz.i64bitinterleave.idiom.a.lo, 2
  %fuzz.i64bitinterleave.idiom.a.bit10 = and i32 %fuzz.i64bitinterleave.idiom.a.shr9, 1
  %fuzz.i64bitinterleave.idiom.b.shr11 = lshr i32 %fuzz.i64bitinterleave.idiom.b.lo, 2
  %fuzz.i64bitinterleave.idiom.b.bit12 = and i32 %fuzz.i64bitinterleave.idiom.b.shr11, 1
  %fuzz.i64bitinterleave.idiom.a.shl13 = shl i32 %fuzz.i64bitinterleave.idiom.a.bit10, 4
  %fuzz.i64bitinterleave.idiom.b.shl14 = shl i32 %fuzz.i64bitinterleave.idiom.b.bit12, 5
  %fuzz.i64bitinterleave.idiom.pair15 = or i32 %fuzz.i64bitinterleave.idiom.a.shl13, %fuzz.i64bitinterleave.idiom.b.shl14
  %fuzz.i64bitinterleave.idiom.accumulate16 = or i32 %fuzz.i64bitinterleave.idiom.accumulate8, %fuzz.i64bitinterleave.idiom.pair15
  %fuzz.i64bitinterleave.idiom.a.shr17 = lshr i32 %fuzz.i64bitinterleave.idiom.a.lo, 3
  %fuzz.i64bitinterleave.idiom.a.bit18 = and i32 %fuzz.i64bitinterleave.idiom.a.shr17, 1
  %fuzz.i64bitinterleave.idiom.b.shr19 = lshr i32 %fuzz.i64bitinterleave.idiom.b.lo, 3
  %fuzz.i64bitinterleave.idiom.b.bit20 = and i32 %fuzz.i64bitinterleave.idiom.b.shr19, 1
  %fuzz.i64bitinterleave.idiom.a.shl21 = shl i32 %fuzz.i64bitinterleave.idiom.a.bit18, 6
  %fuzz.i64bitinterleave.idiom.b.shl22 = shl i32 %fuzz.i64bitinterleave.idiom.b.bit20, 7
  %fuzz.i64bitinterleave.idiom.pair23 = or i32 %fuzz.i64bitinterleave.idiom.a.shl21, %fuzz.i64bitinterleave.idiom.b.shl22
  %fuzz.i64bitinterleave.idiom.accumulate24 = or i32 %fuzz.i64bitinterleave.idiom.accumulate16, %fuzz.i64bitinterleave.idiom.pair23
  %fuzz.i64bitinterleave.idiom.a.shr25 = lshr i32 %fuzz.i64bitinterleave.idiom.a.lo, 4
  %fuzz.i64bitinterleave.idiom.a.bit26 = and i32 %fuzz.i64bitinterleave.idiom.a.shr25, 1
  %fuzz.i64bitinterleave.idiom.b.shr27 = lshr i32 %fuzz.i64bitinterleave.idiom.b.lo, 4
  %fuzz.i64bitinterleave.idiom.b.bit28 = and i32 %fuzz.i64bitinterleave.idiom.b.shr27, 1
  %fuzz.i64bitinterleave.idiom.a.shl29 = shl i32 %fuzz.i64bitinterleave.idiom.a.bit26, 8
  %fuzz.i64bitinterleave.idiom.b.shl30 = shl i32 %fuzz.i64bitinterleave.idiom.b.bit28, 9
  %fuzz.i64bitinterleave.idiom.pair31 = or i32 %fuzz.i64bitinterleave.idiom.a.shl29, %fuzz.i64bitinterleave.idiom.b.shl30
  %fuzz.i64bitinterleave.idiom.accumulate32 = or i32 %fuzz.i64bitinterleave.idiom.accumulate24, %fuzz.i64bitinterleave.idiom.pair31
  %fuzz.i64bitinterleave.idiom.a.shr33 = lshr i32 %fuzz.i64bitinterleave.idiom.a.lo, 5
  %fuzz.i64bitinterleave.idiom.a.bit34 = and i32 %fuzz.i64bitinterleave.idiom.a.shr33, 1
  %fuzz.i64bitinterleave.idiom.b.shr35 = lshr i32 %fuzz.i64bitinterleave.idiom.b.lo, 5
  %fuzz.i64bitinterleave.idiom.b.bit36 = and i32 %fuzz.i64bitinterleave.idiom.b.shr35, 1
  %fuzz.i64bitinterleave.idiom.a.shl37 = shl i32 %fuzz.i64bitinterleave.idiom.a.bit34, 10
  %fuzz.i64bitinterleave.idiom.b.shl38 = shl i32 %fuzz.i64bitinterleave.idiom.b.bit36, 11
  %fuzz.i64bitinterleave.idiom.pair39 = or i32 %fuzz.i64bitinterleave.idiom.a.shl37, %fuzz.i64bitinterleave.idiom.b.shl38
  %fuzz.i64bitinterleave.idiom.accumulate40 = or i32 %fuzz.i64bitinterleave.idiom.accumulate32, %fuzz.i64bitinterleave.idiom.pair39
  %fuzz.i64bitinterleave.idiom.a.shr41 = lshr i32 %fuzz.i64bitinterleave.idiom.a.lo, 6
  %fuzz.i64bitinterleave.idiom.a.bit42 = and i32 %fuzz.i64bitinterleave.idiom.a.shr41, 1
  %fuzz.i64bitinterleave.idiom.b.shr43 = lshr i32 %fuzz.i64bitinterleave.idiom.b.lo, 6
  %fuzz.i64bitinterleave.idiom.b.bit44 = and i32 %fuzz.i64bitinterleave.idiom.b.shr43, 1
  %fuzz.i64bitinterleave.idiom.a.shl45 = shl i32 %fuzz.i64bitinterleave.idiom.a.bit42, 12
  %fuzz.i64bitinterleave.idiom.b.shl46 = shl i32 %fuzz.i64bitinterleave.idiom.b.bit44, 13
  %fuzz.i64bitinterleave.idiom.pair47 = or i32 %fuzz.i64bitinterleave.idiom.a.shl45, %fuzz.i64bitinterleave.idiom.b.shl46
  %fuzz.i64bitinterleave.idiom.accumulate48 = or i32 %fuzz.i64bitinterleave.idiom.accumulate40, %fuzz.i64bitinterleave.idiom.pair47
  %fuzz.i64bitinterleave.idiom.a.shr49 = lshr i32 %fuzz.i64bitinterleave.idiom.a.lo, 7
  %fuzz.i64bitinterleave.idiom.a.bit50 = and i32 %fuzz.i64bitinterleave.idiom.a.shr49, 1
  %fuzz.i64bitinterleave.idiom.b.shr51 = lshr i32 %fuzz.i64bitinterleave.idiom.b.lo, 7
  %fuzz.i64bitinterleave.idiom.b.bit52 = and i32 %fuzz.i64bitinterleave.idiom.b.shr51, 1
  %fuzz.i64bitinterleave.idiom.a.shl53 = shl i32 %fuzz.i64bitinterleave.idiom.a.bit50, 14
  %fuzz.i64bitinterleave.idiom.b.shl54 = shl i32 %fuzz.i64bitinterleave.idiom.b.bit52, 15
  %fuzz.i64bitinterleave.idiom.pair55 = or i32 %fuzz.i64bitinterleave.idiom.a.shl53, %fuzz.i64bitinterleave.idiom.b.shl54
  %fuzz.i64bitinterleave.idiom.accumulate56 = or i32 %fuzz.i64bitinterleave.idiom.accumulate48, %fuzz.i64bitinterleave.idiom.pair55
  %fuzz.i64bitinterleave.idiom.a.shr57 = lshr i32 %fuzz.i64bitinterleave.idiom.a.lo, 8
  %fuzz.i64bitinterleave.idiom.a.bit58 = and i32 %fuzz.i64bitinterleave.idiom.a.shr57, 1
  %fuzz.i64bitinterleave.idiom.b.shr59 = lshr i32 %fuzz.i64bitinterleave.idiom.b.lo, 8
  %fuzz.i64bitinterleave.idiom.b.bit60 = and i32 %fuzz.i64bitinterleave.idiom.b.shr59, 1
  %fuzz.i64bitinterleave.idiom.a.shl61 = shl i32 %fuzz.i64bitinterleave.idiom.a.bit58, 16
  %fuzz.i64bitinterleave.idiom.b.shl62 = shl i32 %fuzz.i64bitinterleave.idiom.b.bit60, 17
  %fuzz.i64bitinterleave.idiom.pair63 = or i32 %fuzz.i64bitinterleave.idiom.a.shl61, %fuzz.i64bitinterleave.idiom.b.shl62
  %fuzz.i64bitinterleave.idiom.accumulate64 = or i32 %fuzz.i64bitinterleave.idiom.accumulate56, %fuzz.i64bitinterleave.idiom.pair63
  %fuzz.i64bitinterleave.idiom.a.shr65 = lshr i32 %fuzz.i64bitinterleave.idiom.a.lo, 9
  %fuzz.i64bitinterleave.idiom.a.bit66 = and i32 %fuzz.i64bitinterleave.idiom.a.shr65, 1
  %fuzz.i64bitinterleave.idiom.b.shr67 = lshr i32 %fuzz.i64bitinterleave.idiom.b.lo, 9
  %fuzz.i64bitinterleave.idiom.b.bit68 = and i32 %fuzz.i64bitinterleave.idiom.b.shr67, 1
  %fuzz.i64bitinterleave.idiom.a.shl69 = shl i32 %fuzz.i64bitinterleave.idiom.a.bit66, 18
  %fuzz.i64bitinterleave.idiom.b.shl70 = shl i32 %fuzz.i64bitinterleave.idiom.b.bit68, 19
  %fuzz.i64bitinterleave.idiom.pair71 = or i32 %fuzz.i64bitinterleave.idiom.a.shl69, %fuzz.i64bitinterleave.idiom.b.shl70
  %fuzz.i64bitinterleave.idiom.accumulate72 = or i32 %fuzz.i64bitinterleave.idiom.accumulate64, %fuzz.i64bitinterleave.idiom.pair71
  %fuzz.i64bitinterleave.idiom.a.shr73 = lshr i32 %fuzz.i64bitinterleave.idiom.a.lo, 10
  %fuzz.i64bitinterleave.idiom.a.bit74 = and i32 %fuzz.i64bitinterleave.idiom.a.shr73, 1
  %fuzz.i64bitinterleave.idiom.b.shr75 = lshr i32 %fuzz.i64bitinterleave.idiom.b.lo, 10
  %fuzz.i64bitinterleave.idiom.b.bit76 = and i32 %fuzz.i64bitinterleave.idiom.b.shr75, 1
  %fuzz.i64bitinterleave.idiom.a.shl77 = shl i32 %fuzz.i64bitinterleave.idiom.a.bit74, 20
  %fuzz.i64bitinterleave.idiom.b.shl78 = shl i32 %fuzz.i64bitinterleave.idiom.b.bit76, 21
  %fuzz.i64bitinterleave.idiom.pair79 = or i32 %fuzz.i64bitinterleave.idiom.a.shl77, %fuzz.i64bitinterleave.idiom.b.shl78
  %fuzz.i64bitinterleave.idiom.accumulate80 = or i32 %fuzz.i64bitinterleave.idiom.accumulate72, %fuzz.i64bitinterleave.idiom.pair79
  %fuzz.i64bitinterleave.idiom.a.shr81 = lshr i32 %fuzz.i64bitinterleave.idiom.a.lo, 11
  %fuzz.i64bitinterleave.idiom.a.bit82 = and i32 %fuzz.i64bitinterleave.idiom.a.shr81, 1
  %fuzz.i64bitinterleave.idiom.b.shr83 = lshr i32 %fuzz.i64bitinterleave.idiom.b.lo, 11
  %fuzz.i64bitinterleave.idiom.b.bit84 = and i32 %fuzz.i64bitinterleave.idiom.b.shr83, 1
  %fuzz.i64bitinterleave.idiom.a.shl85 = shl i32 %fuzz.i64bitinterleave.idiom.a.bit82, 22
  %fuzz.i64bitinterleave.idiom.b.shl86 = shl i32 %fuzz.i64bitinterleave.idiom.b.bit84, 23
  %fuzz.i64bitinterleave.idiom.pair87 = or i32 %fuzz.i64bitinterleave.idiom.a.shl85, %fuzz.i64bitinterleave.idiom.b.shl86
  %fuzz.i64bitinterleave.idiom.accumulate88 = or i32 %fuzz.i64bitinterleave.idiom.accumulate80, %fuzz.i64bitinterleave.idiom.pair87
  %fuzz.i64bitinterleave.idiom.a.shr89 = lshr i32 %fuzz.i64bitinterleave.idiom.a.lo, 12
  %fuzz.i64bitinterleave.idiom.a.bit90 = and i32 %fuzz.i64bitinterleave.idiom.a.shr89, 1
  %fuzz.i64bitinterleave.idiom.b.shr91 = lshr i32 %fuzz.i64bitinterleave.idiom.b.lo, 12
  %fuzz.i64bitinterleave.idiom.b.bit92 = and i32 %fuzz.i64bitinterleave.idiom.b.shr91, 1
  %fuzz.i64bitinterleave.idiom.a.shl93 = shl i32 %fuzz.i64bitinterleave.idiom.a.bit90, 24
  %fuzz.i64bitinterleave.idiom.b.shl94 = shl i32 %fuzz.i64bitinterleave.idiom.b.bit92, 25
  %fuzz.i64bitinterleave.idiom.pair95 = or i32 %fuzz.i64bitinterleave.idiom.a.shl93, %fuzz.i64bitinterleave.idiom.b.shl94
  %fuzz.i64bitinterleave.idiom.accumulate96 = or i32 %fuzz.i64bitinterleave.idiom.accumulate88, %fuzz.i64bitinterleave.idiom.pair95
  %fuzz.i64bitinterleave.idiom.a.shr97 = lshr i32 %fuzz.i64bitinterleave.idiom.a.lo, 13
  %fuzz.i64bitinterleave.idiom.a.bit98 = and i32 %fuzz.i64bitinterleave.idiom.a.shr97, 1
  %fuzz.i64bitinterleave.idiom.b.shr99 = lshr i32 %fuzz.i64bitinterleave.idiom.b.lo, 13
  %fuzz.i64bitinterleave.idiom.b.bit100 = and i32 %fuzz.i64bitinterleave.idiom.b.shr99, 1
  %fuzz.i64bitinterleave.idiom.a.shl101 = shl i32 %fuzz.i64bitinterleave.idiom.a.bit98, 26
  %fuzz.i64bitinterleave.idiom.b.shl102 = shl i32 %fuzz.i64bitinterleave.idiom.b.bit100, 27
  %fuzz.i64bitinterleave.idiom.pair103 = or i32 %fuzz.i64bitinterleave.idiom.a.shl101, %fuzz.i64bitinterleave.idiom.b.shl102
  %fuzz.i64bitinterleave.idiom.accumulate104 = or i32 %fuzz.i64bitinterleave.idiom.accumulate96, %fuzz.i64bitinterleave.idiom.pair103
  %fuzz.i64bitinterleave.idiom.a.shr105 = lshr i32 %fuzz.i64bitinterleave.idiom.a.lo, 14
  %fuzz.i64bitinterleave.idiom.a.bit106 = and i32 %fuzz.i64bitinterleave.idiom.a.shr105, 1
  %fuzz.i64bitinterleave.idiom.b.shr107 = lshr i32 %fuzz.i64bitinterleave.idiom.b.lo, 14
  %fuzz.i64bitinterleave.idiom.b.bit108 = and i32 %fuzz.i64bitinterleave.idiom.b.shr107, 1
  %fuzz.i64bitinterleave.idiom.a.shl109 = shl i32 %fuzz.i64bitinterleave.idiom.a.bit106, 28
  %fuzz.i64bitinterleave.idiom.b.shl110 = shl i32 %fuzz.i64bitinterleave.idiom.b.bit108, 29
  %fuzz.i64bitinterleave.idiom.pair111 = or i32 %fuzz.i64bitinterleave.idiom.a.shl109, %fuzz.i64bitinterleave.idiom.b.shl110
  %fuzz.i64bitinterleave.idiom.accumulate112 = or i32 %fuzz.i64bitinterleave.idiom.accumulate104, %fuzz.i64bitinterleave.idiom.pair111
  %fuzz.i64bitinterleave.idiom.a.shr113 = lshr i32 %fuzz.i64bitinterleave.idiom.a.lo, 15
  %fuzz.i64bitinterleave.idiom.a.bit114 = and i32 %fuzz.i64bitinterleave.idiom.a.shr113, 1
  %fuzz.i64bitinterleave.idiom.b.shr115 = lshr i32 %fuzz.i64bitinterleave.idiom.b.lo, 15
  %fuzz.i64bitinterleave.idiom.b.bit116 = and i32 %fuzz.i64bitinterleave.idiom.b.shr115, 1
  %fuzz.i64bitinterleave.idiom.a.shl117 = shl i32 %fuzz.i64bitinterleave.idiom.a.bit114, 30
  %fuzz.i64bitinterleave.idiom.b.shl118 = shl i32 %fuzz.i64bitinterleave.idiom.b.bit116, 31
  %fuzz.i64bitinterleave.idiom.pair119 = or i32 %fuzz.i64bitinterleave.idiom.a.shl117, %fuzz.i64bitinterleave.idiom.b.shl118
  %fuzz.i64bitinterleave.idiom.accumulate120 = or i32 %fuzz.i64bitinterleave.idiom.accumulate112, %fuzz.i64bitinterleave.idiom.pair119
  %fuzz.i64bitinterleave.idiom.a64 = zext i32 %fuzz.veci16selectcarry.idiom.fold.xor138 to i64
  %fuzz.i64bitinterleave.idiom.int64 = zext i32 %fuzz.i64bitinterleave.idiom.accumulate120 to i64
  %fuzz.i64bitinterleave.idiom.a64.shl = shl i64 %fuzz.i64bitinterleave.idiom.a64, 32
  %fuzz.i64bitinterleave.idiom.combined = or i64 %fuzz.i64bitinterleave.idiom.a64.shl, %fuzz.i64bitinterleave.idiom.int64
  %fuzz.i64bitinterleave.idiom.hi.shr = lshr i64 %fuzz.i64bitinterleave.idiom.combined, 8
  %fuzz.i64bitinterleave.idiom.hi.add = add i64 %fuzz.i64bitinterleave.idiom.combined, %fuzz.i64bitinterleave.idiom.hi.shr
  %fuzz.i64bitinterleave.idiom.fold.lo = trunc i64 %fuzz.i64bitinterleave.idiom.hi.add to i32
  %fuzz.i64bitinterleave.idiom.fold.hi64 = lshr i64 %fuzz.i64bitinterleave.idiom.hi.add, 32
  %fuzz.i64bitinterleave.idiom.fold.hi = trunc i64 %fuzz.i64bitinterleave.idiom.fold.hi64 to i32
  %fuzz.i64bitinterleave.idiom.fold.xor = xor i32 %fuzz.i64bitinterleave.idiom.fold.lo, %fuzz.i64bitinterleave.idiom.fold.hi
  %fuzz.i64bitinterleave.idiom.a.xor = xor i32 %fuzz.i64bitinterleave.idiom.fold.xor, %fuzz.veci16selectcarry.idiom.fold.xor138
  %fuzz.umaxbitop3cascade.idiom.mix = xor i32 %fuzz.i64bitinterleave.idiom.a.xor, 31
  %fuzz.umaxbitop3cascade.idiom.not.a = xor i32 %fuzz.i64bitinterleave.idiom.a.xor, -1
  %fuzz.umaxbitop3cascade.idiom.not.b = xor i32 31, -1
  %fuzz.umaxbitop3cascade.idiom.a.shl = shl i32 %fuzz.i64bitinterleave.idiom.a.xor, 1
  %fuzz.umaxbitop3cascade.idiom.umax.cmp = icmp ugt i32 %fuzz.umaxbitop3cascade.idiom.a.shl, %fuzz.umaxbitop3cascade.idiom.mix
  %fuzz.umaxbitop3cascade.idiom.umax.umax = select i1 %fuzz.umaxbitop3cascade.idiom.umax.cmp, i32 %fuzz.umaxbitop3cascade.idiom.a.shl, i32 %fuzz.umaxbitop3cascade.idiom.mix
  %fuzz.umaxbitop3cascade.idiom.umin.cmp = icmp ugt i32 %fuzz.umaxbitop3cascade.idiom.a.shl, %fuzz.umaxbitop3cascade.idiom.mix
  %fuzz.umaxbitop3cascade.idiom.umin.umin = select i1 %fuzz.umaxbitop3cascade.idiom.umin.cmp, i32 %fuzz.umaxbitop3cascade.idiom.mix, i32 %fuzz.umaxbitop3cascade.idiom.a.shl
  %fuzz.umaxbitop3cascade.idiom.max.xor.y = xor i32 %fuzz.umaxbitop3cascade.idiom.umax.umax, %fuzz.umaxbitop3cascade.idiom.mix
  %fuzz.umaxbitop3cascade.idiom.not.x = xor i32 %fuzz.umaxbitop3cascade.idiom.a.shl, -1
  %fuzz.umaxbitop3cascade.idiom.max.xor.y.and.notx = and i32 %fuzz.umaxbitop3cascade.idiom.max.xor.y, %fuzz.umaxbitop3cascade.idiom.not.x
  %fuzz.umaxbitop3cascade.idiom.lane.xor2 = xor i32 %fuzz.umaxbitop3cascade.idiom.max.xor.y.and.notx, %fuzz.umaxbitop3cascade.idiom.umin.umin
  %fuzz.umaxbitop3cascade.idiom.acc.xor = xor i32 0, %fuzz.umaxbitop3cascade.idiom.lane.xor2
  %fuzz.umaxbitop3cascade.idiom.acc.next = add i32 %fuzz.umaxbitop3cascade.idiom.acc.xor, 1
  %fuzz.umaxbitop3cascade.idiom.mix.shr = lshr i32 %fuzz.umaxbitop3cascade.idiom.mix, 8
  %fuzz.umaxbitop3cascade.idiom.a.or.mix = or i32 %fuzz.i64bitinterleave.idiom.a.xor, %fuzz.umaxbitop3cascade.idiom.mix.shr
  %fuzz.umaxbitop3cascade.idiom.umax.cmp1 = icmp ugt i32 %fuzz.umaxbitop3cascade.idiom.a.or.mix, %fuzz.umaxbitop3cascade.idiom.not.a
  %fuzz.umaxbitop3cascade.idiom.umax.umax2 = select i1 %fuzz.umaxbitop3cascade.idiom.umax.cmp1, i32 %fuzz.umaxbitop3cascade.idiom.a.or.mix, i32 %fuzz.umaxbitop3cascade.idiom.not.a
  %fuzz.umaxbitop3cascade.idiom.umin.cmp3 = icmp ugt i32 %fuzz.umaxbitop3cascade.idiom.a.or.mix, %fuzz.umaxbitop3cascade.idiom.not.a
  %fuzz.umaxbitop3cascade.idiom.umin.umin4 = select i1 %fuzz.umaxbitop3cascade.idiom.umin.cmp3, i32 %fuzz.umaxbitop3cascade.idiom.not.a, i32 %fuzz.umaxbitop3cascade.idiom.a.or.mix
  %fuzz.umaxbitop3cascade.idiom.max.xor.y5 = xor i32 %fuzz.umaxbitop3cascade.idiom.umax.umax2, %fuzz.umaxbitop3cascade.idiom.not.a
  %fuzz.umaxbitop3cascade.idiom.not.x6 = xor i32 %fuzz.umaxbitop3cascade.idiom.a.or.mix, -1
  %fuzz.umaxbitop3cascade.idiom.max.xor.y.and.notx7 = and i32 %fuzz.umaxbitop3cascade.idiom.max.xor.y5, %fuzz.umaxbitop3cascade.idiom.not.x6
  %fuzz.umaxbitop3cascade.idiom.lane.xor28 = xor i32 %fuzz.umaxbitop3cascade.idiom.max.xor.y.and.notx7, %fuzz.umaxbitop3cascade.idiom.umin.umin4
  %fuzz.umaxbitop3cascade.idiom.acc.xor9 = xor i32 %fuzz.umaxbitop3cascade.idiom.acc.next, %fuzz.umaxbitop3cascade.idiom.lane.xor28
  %fuzz.umaxbitop3cascade.idiom.acc.next10 = add i32 %fuzz.umaxbitop3cascade.idiom.acc.xor9, 59
  %fuzz.umaxbitop3cascade.idiom.mix.and.nota = and i32 %fuzz.umaxbitop3cascade.idiom.mix, %fuzz.umaxbitop3cascade.idiom.not.a
  %fuzz.umaxbitop3cascade.idiom.umax.cmp11 = icmp ugt i32 %fuzz.umaxbitop3cascade.idiom.mix.and.nota, 31
  %fuzz.umaxbitop3cascade.idiom.umax.umax12 = select i1 %fuzz.umaxbitop3cascade.idiom.umax.cmp11, i32 %fuzz.umaxbitop3cascade.idiom.mix.and.nota, i32 31
  %fuzz.umaxbitop3cascade.idiom.umin.cmp13 = icmp ugt i32 %fuzz.umaxbitop3cascade.idiom.mix.and.nota, 31
  %fuzz.umaxbitop3cascade.idiom.umin.umin14 = select i1 %fuzz.umaxbitop3cascade.idiom.umin.cmp13, i32 31, i32 %fuzz.umaxbitop3cascade.idiom.mix.and.nota
  %fuzz.umaxbitop3cascade.idiom.max.xor.y15 = xor i32 %fuzz.umaxbitop3cascade.idiom.umax.umax12, 31
  %fuzz.umaxbitop3cascade.idiom.not.x16 = xor i32 %fuzz.umaxbitop3cascade.idiom.mix.and.nota, -1
  %fuzz.umaxbitop3cascade.idiom.max.xor.y.and.notx17 = and i32 %fuzz.umaxbitop3cascade.idiom.max.xor.y15, %fuzz.umaxbitop3cascade.idiom.not.x16
  %fuzz.umaxbitop3cascade.idiom.lane.xor218 = xor i32 %fuzz.umaxbitop3cascade.idiom.max.xor.y.and.notx17, %fuzz.umaxbitop3cascade.idiom.umin.umin14
  %fuzz.umaxbitop3cascade.idiom.acc.xor19 = xor i32 %fuzz.umaxbitop3cascade.idiom.acc.next10, %fuzz.umaxbitop3cascade.idiom.lane.xor218
  %fuzz.umaxbitop3cascade.idiom.acc.next20 = add i32 %fuzz.umaxbitop3cascade.idiom.acc.xor19, 117
  %fuzz.umaxbitop3cascade.idiom.mix.shr21 = lshr i32 %fuzz.umaxbitop3cascade.idiom.mix, 14
  %fuzz.umaxbitop3cascade.idiom.a.or.mix22 = or i32 %fuzz.i64bitinterleave.idiom.a.xor, %fuzz.umaxbitop3cascade.idiom.mix.shr21
  %fuzz.umaxbitop3cascade.idiom.umax.cmp23 = icmp ugt i32 %fuzz.umaxbitop3cascade.idiom.a.or.mix22, %fuzz.umaxbitop3cascade.idiom.not.a
  %fuzz.umaxbitop3cascade.idiom.umax.umax24 = select i1 %fuzz.umaxbitop3cascade.idiom.umax.cmp23, i32 %fuzz.umaxbitop3cascade.idiom.a.or.mix22, i32 %fuzz.umaxbitop3cascade.idiom.not.a
  %fuzz.umaxbitop3cascade.idiom.umin.cmp25 = icmp ugt i32 %fuzz.umaxbitop3cascade.idiom.a.or.mix22, %fuzz.umaxbitop3cascade.idiom.not.a
  %fuzz.umaxbitop3cascade.idiom.umin.umin26 = select i1 %fuzz.umaxbitop3cascade.idiom.umin.cmp25, i32 %fuzz.umaxbitop3cascade.idiom.not.a, i32 %fuzz.umaxbitop3cascade.idiom.a.or.mix22
  %fuzz.umaxbitop3cascade.idiom.min.and.max = and i32 %fuzz.umaxbitop3cascade.idiom.umin.umin26, %fuzz.umaxbitop3cascade.idiom.umax.umax24
  %fuzz.umaxbitop3cascade.idiom.x.xor.y = xor i32 %fuzz.umaxbitop3cascade.idiom.a.or.mix22, %fuzz.umaxbitop3cascade.idiom.not.a
  %fuzz.umaxbitop3cascade.idiom.lane.or = or i32 %fuzz.umaxbitop3cascade.idiom.min.and.max, %fuzz.umaxbitop3cascade.idiom.x.xor.y
  %fuzz.umaxbitop3cascade.idiom.acc.xor27 = xor i32 %fuzz.umaxbitop3cascade.idiom.acc.next20, %fuzz.umaxbitop3cascade.idiom.lane.or
  %fuzz.umaxbitop3cascade.idiom.acc.next28 = add i32 %fuzz.umaxbitop3cascade.idiom.acc.xor27, 175
  %fuzz.umaxbitop3cascade.idiom.final.ashr = ashr i32 %fuzz.umaxbitop3cascade.idiom.acc.next28, 31
  %fuzz.umaxbitop3cascade.idiom.a.add = add i32 %fuzz.umaxbitop3cascade.idiom.acc.next28, %fuzz.i64bitinterleave.idiom.a.xor
  store i32 %fuzz.umaxbitop3cascade.idiom.a.add, ptr addrspace(1) %out.ptr, align 4
  br label %exit

exit:                                             ; preds = %fuzz.loop.nest.exit, %entry
  ret void
}

; Function Attrs: nocallback nofree nosync nounwind speculatable willreturn memory(none)
declare noundef i32 @llvm.amdgcn.workgroup.id.x() #1

; Function Attrs: nocallback nofree nosync nounwind speculatable willreturn memory(none)
declare noundef range(i32 0, 1024) i32 @llvm.amdgcn.workitem.id.x() #1

; Function Attrs: nocallback nocreateundeforpoison nofree nosync nounwind speculatable willreturn memory(none)
declare i32 @llvm.umin.i32(i32, i32) #2

; Function Attrs: nocallback nocreateundeforpoison nofree nosync nounwind speculatable willreturn memory(none)
declare i32 @llvm.bswap.i32(i32) #2

; Function Attrs: nocallback nocreateundeforpoison nofree nosync nounwind speculatable willreturn memory(none)
declare { i32, i1 } @llvm.smul.with.overflow.i32(i32, i32) #2

attributes #0 = { convergent nounwind "amdgpu-flat-work-group-size"="1,256" "target-cpu"="gfx950" "uniform-work-group-size"="true" }
attributes #1 = { nocallback nofree nosync nounwind speculatable willreturn memory(none) }
attributes #2 = { nocallback nocreateundeforpoison nofree nosync nounwind speculatable willreturn memory(none) }

!llvm.module.flags = !{!0, !1, !2}

!0 = !{i32 1, !"amdhsa_code_object_version", i32 600}
!1 = !{i32 1, !"amdgpu_printf_kind", !"hostcall"}
!2 = !{i32 8, !"PIC Level", i32 2}
