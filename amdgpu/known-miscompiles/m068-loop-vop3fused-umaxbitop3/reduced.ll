; RUN-LLVM-BUILD: build/llvm-fuzzer
; RUN-INPUTS: 0
; RUN-LLVM-BUILD: build/llvm-fuzzer
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
  %fuzz.umin = call i32 @llvm.umin.i32(i32 %mix, i32 -2147483648)
  %fuzz.bitrunmask.idiom.count.a.add = add i32 %fuzz.umin, 21
  %fuzz.bitrunmask.idiom.count.a = and i32 %fuzz.bitrunmask.idiom.count.a.add, 31
  %fuzz.bitrunmask.idiom.count.b.xor = xor i32 -1, 28
  %fuzz.bitrunmask.idiom.count.b = and i32 %fuzz.bitrunmask.idiom.count.b.xor, 31
  %fuzz.bitrunmask.idiom.run.a.shl = shl i32 1, %fuzz.bitrunmask.idiom.count.a
  %fuzz.bitrunmask.idiom.run.a = sub i32 %fuzz.bitrunmask.idiom.run.a.shl, 1
  %fuzz.bitrunmask.idiom.run.b.shl = shl i32 1, %fuzz.bitrunmask.idiom.count.b
  %fuzz.bitrunmask.idiom.run.b = sub i32 %fuzz.bitrunmask.idiom.run.b.shl, 1
  %fuzz.bitrunmask.idiom.inv.a.raw = sub i32 32, %fuzz.bitrunmask.idiom.count.a
  %fuzz.bitrunmask.idiom.inv.a = and i32 %fuzz.bitrunmask.idiom.inv.a.raw, 31
  %fuzz.bitrunmask.idiom.left = shl i32 %fuzz.umin, %fuzz.bitrunmask.idiom.count.b
  %fuzz.bitrunmask.idiom.right = lshr i32 -1, %fuzz.bitrunmask.idiom.inv.a
  %fuzz.bitrunmask.idiom.window = or i32 %fuzz.bitrunmask.idiom.left, %fuzz.bitrunmask.idiom.right
  %fuzz.bitrunmask.idiom.masked.a = and i32 %fuzz.bitrunmask.idiom.window, %fuzz.bitrunmask.idiom.run.a
  %fuzz.bitrunmask.idiom.window.xor = xor i32 %fuzz.bitrunmask.idiom.window, -1
  %fuzz.bitrunmask.idiom.masked.b = and i32 %fuzz.bitrunmask.idiom.window.xor, %fuzz.bitrunmask.idiom.run.b
  %fuzz.bitrunmask.idiom.lt = icmp ult i32 %fuzz.bitrunmask.idiom.count.a, %fuzz.bitrunmask.idiom.count.b
  %fuzz.bitrunmask.idiom.select = select i1 %fuzz.bitrunmask.idiom.lt, i32 %fuzz.bitrunmask.idiom.masked.a, i32 %fuzz.bitrunmask.idiom.masked.b
  %fuzz.bitrunmask.idiom.sel.mask = and i32 %fuzz.bitrunmask.idiom.select, 16711935
  %fuzz.bitrunmask.idiom.win.mask = and i32 %fuzz.bitrunmask.idiom.window, -16711936
  %fuzz.bitrunmask.idiom.merge = or i32 %fuzz.bitrunmask.idiom.sel.mask, %fuzz.bitrunmask.idiom.win.mask
  br label %fuzz.loop.multi.header

fuzz.loop.multi.header:                           ; preds = %fuzz.loop.multi.continue, %body
  %fuzz.loop.iv.multi = phi i32 [ 0, %body ], [ %fuzz.loop.multi.next, %fuzz.loop.multi.continue ]
  %fuzz.loop.acc.multi = phi i32 [ %fuzz.bitrunmask.idiom.merge, %body ], [ %fuzz.loop.multi.acc.next, %fuzz.loop.multi.continue ]
  %fuzz.loop.multi.cond = icmp ult i32 %fuzz.loop.iv.multi, 1
  br i1 %fuzz.loop.multi.cond, label %fuzz.loop.multi.body, label %fuzz.loop.multi.exit

fuzz.loop.multi.body:                             ; preds = %fuzz.loop.multi.header
  %fuzz.cfg.bool.cmp0 = icmp ugt i32 %fuzz.loop.acc.multi, %fuzz.bitrunmask.idiom.window
  %fuzz.cfg.bool.cmp1 = icmp ult i32 %fuzz.bitrunmask.idiom.window, 2147483647
  %fuzz.cfg.bool.or = or i1 %fuzz.cfg.bool.cmp0, %fuzz.cfg.bool.cmp1
  %fuzz.cfg.bool.zext = zext i1 %fuzz.cfg.bool.or to i32
  %fuzz.cfg.bool.xor.i32 = xor i32 %fuzz.loop.acc.multi, %fuzz.cfg.bool.zext
  %fuzz.cfg.clamppack.idiom.add.a.trunc = trunc i32 %fuzz.cfg.bool.xor.i32 to i8
  %fuzz.cfg.clamppack.idiom.add.a.zext = zext i8 %fuzz.cfg.clamppack.idiom.add.a.trunc to i32
  %fuzz.cfg.clamppack.idiom.add.b.trunc = trunc i32 %fuzz.bitrunmask.idiom.window to i8
  %fuzz.cfg.clamppack.idiom.add.b.zext = zext i8 %fuzz.cfg.clamppack.idiom.add.b.trunc to i32
  %fuzz.cfg.clamppack.idiom.u8.add = add i32 %fuzz.cfg.clamppack.idiom.add.a.zext, %fuzz.cfg.clamppack.idiom.add.b.zext
  %fuzz.cfg.clamppack.idiom.u8.add.sat.below = icmp ult i32 %fuzz.cfg.clamppack.idiom.u8.add, 0
  %fuzz.cfg.clamppack.idiom.u8.add.sat.atleast = select i1 %fuzz.cfg.clamppack.idiom.u8.add.sat.below, i32 0, i32 %fuzz.cfg.clamppack.idiom.u8.add
  %fuzz.cfg.clamppack.idiom.u8.add.sat.above = icmp ugt i32 %fuzz.cfg.clamppack.idiom.u8.add.sat.atleast, 255
  %fuzz.cfg.clamppack.idiom.u8.add.sat.clamp = select i1 %fuzz.cfg.clamppack.idiom.u8.add.sat.above, i32 255, i32 %fuzz.cfg.clamppack.idiom.u8.add.sat.atleast
  %fuzz.cfg.clamppack.idiom.add.a.shr = lshr i32 %fuzz.cfg.bool.xor.i32, 8
  %fuzz.cfg.clamppack.idiom.add.a.trunc1 = trunc i32 %fuzz.cfg.clamppack.idiom.add.a.shr to i8
  %fuzz.cfg.clamppack.idiom.add.a.zext2 = zext i8 %fuzz.cfg.clamppack.idiom.add.a.trunc1 to i32
  %fuzz.cfg.clamppack.idiom.add.b.shr = lshr i32 %fuzz.bitrunmask.idiom.window, 8
  %fuzz.cfg.clamppack.idiom.add.b.trunc3 = trunc i32 %fuzz.cfg.clamppack.idiom.add.b.shr to i8
  %fuzz.cfg.clamppack.idiom.add.b.zext4 = zext i8 %fuzz.cfg.clamppack.idiom.add.b.trunc3 to i32
  %fuzz.cfg.clamppack.idiom.u8.add5 = add i32 %fuzz.cfg.clamppack.idiom.add.a.zext2, %fuzz.cfg.clamppack.idiom.add.b.zext4
  %fuzz.cfg.clamppack.idiom.u8.add.sat.below6 = icmp ult i32 %fuzz.cfg.clamppack.idiom.u8.add5, 0
  %fuzz.cfg.clamppack.idiom.u8.add.sat.atleast7 = select i1 %fuzz.cfg.clamppack.idiom.u8.add.sat.below6, i32 0, i32 %fuzz.cfg.clamppack.idiom.u8.add5
  %fuzz.cfg.clamppack.idiom.u8.add.sat.above8 = icmp ugt i32 %fuzz.cfg.clamppack.idiom.u8.add.sat.atleast7, 255
  %fuzz.cfg.clamppack.idiom.u8.add.sat.clamp9 = select i1 %fuzz.cfg.clamppack.idiom.u8.add.sat.above8, i32 255, i32 %fuzz.cfg.clamppack.idiom.u8.add.sat.atleast7
  %fuzz.cfg.clamppack.idiom.add.a.shr10 = lshr i32 %fuzz.cfg.bool.xor.i32, 16
  %fuzz.cfg.clamppack.idiom.add.a.trunc11 = trunc i32 %fuzz.cfg.clamppack.idiom.add.a.shr10 to i8
  %fuzz.cfg.clamppack.idiom.add.a.zext12 = zext i8 %fuzz.cfg.clamppack.idiom.add.a.trunc11 to i32
  %fuzz.cfg.clamppack.idiom.add.b.shr13 = lshr i32 %fuzz.bitrunmask.idiom.window, 16
  %fuzz.cfg.clamppack.idiom.add.b.trunc14 = trunc i32 %fuzz.cfg.clamppack.idiom.add.b.shr13 to i8
  %fuzz.cfg.clamppack.idiom.add.b.zext15 = zext i8 %fuzz.cfg.clamppack.idiom.add.b.trunc14 to i32
  %fuzz.cfg.clamppack.idiom.u8.add16 = add i32 %fuzz.cfg.clamppack.idiom.add.a.zext12, %fuzz.cfg.clamppack.idiom.add.b.zext15
  %fuzz.cfg.clamppack.idiom.u8.add.sat.below17 = icmp ult i32 %fuzz.cfg.clamppack.idiom.u8.add16, 0
  %fuzz.cfg.clamppack.idiom.u8.add.sat.atleast18 = select i1 %fuzz.cfg.clamppack.idiom.u8.add.sat.below17, i32 0, i32 %fuzz.cfg.clamppack.idiom.u8.add16
  %fuzz.cfg.clamppack.idiom.u8.add.sat.above19 = icmp ugt i32 %fuzz.cfg.clamppack.idiom.u8.add.sat.atleast18, 255
  %fuzz.cfg.clamppack.idiom.u8.add.sat.clamp20 = select i1 %fuzz.cfg.clamppack.idiom.u8.add.sat.above19, i32 255, i32 %fuzz.cfg.clamppack.idiom.u8.add.sat.atleast18
  %fuzz.cfg.clamppack.idiom.add.a.shr21 = lshr i32 %fuzz.cfg.bool.xor.i32, 24
  %fuzz.cfg.clamppack.idiom.add.a.trunc22 = trunc i32 %fuzz.cfg.clamppack.idiom.add.a.shr21 to i8
  %fuzz.cfg.clamppack.idiom.add.a.zext23 = zext i8 %fuzz.cfg.clamppack.idiom.add.a.trunc22 to i32
  %fuzz.cfg.clamppack.idiom.add.b.shr24 = lshr i32 %fuzz.bitrunmask.idiom.window, 24
  %fuzz.cfg.clamppack.idiom.add.b.trunc25 = trunc i32 %fuzz.cfg.clamppack.idiom.add.b.shr24 to i8
  %fuzz.cfg.clamppack.idiom.add.b.zext26 = zext i8 %fuzz.cfg.clamppack.idiom.add.b.trunc25 to i32
  %fuzz.cfg.clamppack.idiom.u8.add27 = add i32 %fuzz.cfg.clamppack.idiom.add.a.zext23, %fuzz.cfg.clamppack.idiom.add.b.zext26
  %fuzz.cfg.clamppack.idiom.u8.add.sat.below28 = icmp ult i32 %fuzz.cfg.clamppack.idiom.u8.add27, 0
  %fuzz.cfg.clamppack.idiom.u8.add.sat.atleast29 = select i1 %fuzz.cfg.clamppack.idiom.u8.add.sat.below28, i32 0, i32 %fuzz.cfg.clamppack.idiom.u8.add27
  %fuzz.cfg.clamppack.idiom.u8.add.sat.above30 = icmp ugt i32 %fuzz.cfg.clamppack.idiom.u8.add.sat.atleast29, 255
  %fuzz.cfg.clamppack.idiom.u8.add.sat.clamp31 = select i1 %fuzz.cfg.clamppack.idiom.u8.add.sat.above30, i32 255, i32 %fuzz.cfg.clamppack.idiom.u8.add.sat.atleast29
  %fuzz.cfg.clamppack.idiom.u8.add.pack.mask = and i32 %fuzz.cfg.clamppack.idiom.u8.add.sat.clamp, 255
  %fuzz.cfg.clamppack.idiom.u8.add.pack.add = add i32 0, %fuzz.cfg.clamppack.idiom.u8.add.pack.mask
  %fuzz.cfg.clamppack.idiom.u8.add.pack.mask32 = and i32 %fuzz.cfg.clamppack.idiom.u8.add.sat.clamp9, 255
  %fuzz.cfg.clamppack.idiom.u8.add.pack.shift = shl i32 %fuzz.cfg.clamppack.idiom.u8.add.pack.mask32, 8
  %fuzz.cfg.clamppack.idiom.u8.add.pack.add33 = add i32 %fuzz.cfg.clamppack.idiom.u8.add.pack.add, %fuzz.cfg.clamppack.idiom.u8.add.pack.shift
  %fuzz.cfg.clamppack.idiom.u8.add.pack.mask34 = and i32 %fuzz.cfg.clamppack.idiom.u8.add.sat.clamp20, 255
  %fuzz.cfg.clamppack.idiom.u8.add.pack.shift35 = shl i32 %fuzz.cfg.clamppack.idiom.u8.add.pack.mask34, 16
  %fuzz.cfg.clamppack.idiom.u8.add.pack.add36 = add i32 %fuzz.cfg.clamppack.idiom.u8.add.pack.add33, %fuzz.cfg.clamppack.idiom.u8.add.pack.shift35
  %fuzz.cfg.clamppack.idiom.u8.add.pack.mask37 = and i32 %fuzz.cfg.clamppack.idiom.u8.add.sat.clamp31, 255
  %fuzz.cfg.clamppack.idiom.u8.add.pack.shift38 = shl i32 %fuzz.cfg.clamppack.idiom.u8.add.pack.mask37, 24
  %fuzz.cfg.clamppack.idiom.u8.add.pack.add39 = add i32 %fuzz.cfg.clamppack.idiom.u8.add.pack.add36, %fuzz.cfg.clamppack.idiom.u8.add.pack.shift38
  %fuzz.cfg.bitdeposit.idiom.src.shr = lshr i32 %fuzz.cfg.clamppack.idiom.u8.add.pack.add39, 16
  %fuzz.cfg.bitdeposit.idiom.bit = and i32 %fuzz.cfg.bitdeposit.idiom.src.shr, 1
  %fuzz.cfg.bitdeposit.idiom.compress.shl = shl i32 %fuzz.cfg.bitdeposit.idiom.bit, 0
  %fuzz.cfg.bitdeposit.idiom.compress = or i32 0, %fuzz.cfg.bitdeposit.idiom.compress.shl
  %fuzz.cfg.bitdeposit.idiom.deposit.shl = shl i32 %fuzz.cfg.bitdeposit.idiom.bit, 31
  %fuzz.cfg.bitdeposit.idiom.deposit = or i32 0, %fuzz.cfg.bitdeposit.idiom.deposit.shl
  %fuzz.cfg.bitdeposit.idiom.parity = xor i32 0, %fuzz.cfg.bitdeposit.idiom.bit
  %fuzz.cfg.bitdeposit.idiom.src.shr40 = lshr i32 %fuzz.bitrunmask.idiom.window, 15
  %fuzz.cfg.bitdeposit.idiom.bit41 = and i32 %fuzz.cfg.bitdeposit.idiom.src.shr40, 1
  %fuzz.cfg.bitdeposit.idiom.compress.shl42 = shl i32 %fuzz.cfg.bitdeposit.idiom.bit41, 1
  %fuzz.cfg.bitdeposit.idiom.compress43 = or i32 %fuzz.cfg.bitdeposit.idiom.compress, %fuzz.cfg.bitdeposit.idiom.compress.shl42
  %fuzz.cfg.bitdeposit.idiom.deposit.shl44 = shl i32 %fuzz.cfg.bitdeposit.idiom.bit41, 11
  %fuzz.cfg.bitdeposit.idiom.deposit45 = or i32 %fuzz.cfg.bitdeposit.idiom.deposit, %fuzz.cfg.bitdeposit.idiom.deposit.shl44
  %fuzz.cfg.bitdeposit.idiom.parity46 = xor i32 %fuzz.cfg.bitdeposit.idiom.parity, %fuzz.cfg.bitdeposit.idiom.bit41
  %fuzz.cfg.bitdeposit.idiom.src.shr47 = lshr i32 %fuzz.cfg.clamppack.idiom.u8.add.pack.add39, 10
  %fuzz.cfg.bitdeposit.idiom.bit48 = and i32 %fuzz.cfg.bitdeposit.idiom.src.shr47, 1
  %fuzz.cfg.bitdeposit.idiom.compress.shl49 = shl i32 %fuzz.cfg.bitdeposit.idiom.bit48, 2
  %fuzz.cfg.bitdeposit.idiom.compress50 = or i32 %fuzz.cfg.bitdeposit.idiom.compress43, %fuzz.cfg.bitdeposit.idiom.compress.shl49
  %fuzz.cfg.bitdeposit.idiom.deposit.shl51 = shl i32 %fuzz.cfg.bitdeposit.idiom.bit48, 26
  %fuzz.cfg.bitdeposit.idiom.deposit52 = or i32 %fuzz.cfg.bitdeposit.idiom.deposit45, %fuzz.cfg.bitdeposit.idiom.deposit.shl51
  %fuzz.cfg.bitdeposit.idiom.parity53 = xor i32 %fuzz.cfg.bitdeposit.idiom.parity46, %fuzz.cfg.bitdeposit.idiom.bit48
  %fuzz.cfg.bitdeposit.idiom.src.shr54 = lshr i32 %fuzz.bitrunmask.idiom.window, 28
  %fuzz.cfg.bitdeposit.idiom.bit55 = and i32 %fuzz.cfg.bitdeposit.idiom.src.shr54, 1
  %fuzz.cfg.bitdeposit.idiom.compress.shl56 = shl i32 %fuzz.cfg.bitdeposit.idiom.bit55, 3
  %fuzz.cfg.bitdeposit.idiom.compress57 = or i32 %fuzz.cfg.bitdeposit.idiom.compress50, %fuzz.cfg.bitdeposit.idiom.compress.shl56
  %fuzz.cfg.bitdeposit.idiom.deposit.shl58 = shl i32 %fuzz.cfg.bitdeposit.idiom.bit55, 20
  %fuzz.cfg.bitdeposit.idiom.deposit59 = or i32 %fuzz.cfg.bitdeposit.idiom.deposit52, %fuzz.cfg.bitdeposit.idiom.deposit.shl58
  %fuzz.cfg.bitdeposit.idiom.parity60 = xor i32 %fuzz.cfg.bitdeposit.idiom.parity53, %fuzz.cfg.bitdeposit.idiom.bit55
  %fuzz.cfg.bitdeposit.idiom.src.shr61 = lshr i32 %fuzz.cfg.clamppack.idiom.u8.add.pack.add39, 23
  %fuzz.cfg.bitdeposit.idiom.bit62 = and i32 %fuzz.cfg.bitdeposit.idiom.src.shr61, 1
  %fuzz.cfg.bitdeposit.idiom.compress.shl63 = shl i32 %fuzz.cfg.bitdeposit.idiom.bit62, 4
  %fuzz.cfg.bitdeposit.idiom.compress64 = or i32 %fuzz.cfg.bitdeposit.idiom.compress57, %fuzz.cfg.bitdeposit.idiom.compress.shl63
  %fuzz.cfg.bitdeposit.idiom.deposit.shl65 = shl i32 %fuzz.cfg.bitdeposit.idiom.bit62, 0
  %fuzz.cfg.bitdeposit.idiom.deposit66 = or i32 %fuzz.cfg.bitdeposit.idiom.deposit59, %fuzz.cfg.bitdeposit.idiom.deposit.shl65
  %fuzz.cfg.bitdeposit.idiom.parity67 = xor i32 %fuzz.cfg.bitdeposit.idiom.parity60, %fuzz.cfg.bitdeposit.idiom.bit62
  %fuzz.cfg.bitdeposit.idiom.src.shr68 = lshr i32 %fuzz.bitrunmask.idiom.window, 22
  %fuzz.cfg.bitdeposit.idiom.bit69 = and i32 %fuzz.cfg.bitdeposit.idiom.src.shr68, 1
  %fuzz.cfg.bitdeposit.idiom.compress.shl70 = shl i32 %fuzz.cfg.bitdeposit.idiom.bit69, 5
  %fuzz.cfg.bitdeposit.idiom.compress71 = or i32 %fuzz.cfg.bitdeposit.idiom.compress64, %fuzz.cfg.bitdeposit.idiom.compress.shl70
  %fuzz.cfg.bitdeposit.idiom.deposit.shl72 = shl i32 %fuzz.cfg.bitdeposit.idiom.bit69, 12
  %fuzz.cfg.bitdeposit.idiom.deposit73 = or i32 %fuzz.cfg.bitdeposit.idiom.deposit66, %fuzz.cfg.bitdeposit.idiom.deposit.shl72
  %fuzz.cfg.bitdeposit.idiom.parity74 = xor i32 %fuzz.cfg.bitdeposit.idiom.parity67, %fuzz.cfg.bitdeposit.idiom.bit69
  %fuzz.cfg.bitdeposit.idiom.src.shr75 = lshr i32 %fuzz.cfg.clamppack.idiom.u8.add.pack.add39, 15
  %fuzz.cfg.bitdeposit.idiom.bit76 = and i32 %fuzz.cfg.bitdeposit.idiom.src.shr75, -1431655766
  %fuzz.cfg.bitdeposit.idiom.compress.shl77 = shl i32 %fuzz.cfg.bitdeposit.idiom.bit76, 6
  %fuzz.cfg.bitdeposit.idiom.compress78 = or i32 %fuzz.cfg.bitdeposit.idiom.compress71, %fuzz.cfg.bitdeposit.idiom.compress.shl77
  %fuzz.cfg.bitdeposit.idiom.deposit.shl79 = shl i32 %fuzz.cfg.bitdeposit.idiom.bit76, 14
  %fuzz.cfg.bitdeposit.idiom.deposit80 = or i32 %fuzz.cfg.bitdeposit.idiom.deposit73, %fuzz.cfg.bitdeposit.idiom.deposit.shl79
  %fuzz.cfg.bitdeposit.idiom.parity81 = xor i32 %fuzz.cfg.bitdeposit.idiom.parity74, %fuzz.cfg.bitdeposit.idiom.bit76
  %fuzz.cfg.bitdeposit.idiom.src.shr82 = lshr i32 %fuzz.bitrunmask.idiom.window, 24
  %fuzz.cfg.bitdeposit.idiom.bit83 = and i32 %fuzz.cfg.bitdeposit.idiom.src.shr82, 1
  %fuzz.cfg.bitdeposit.idiom.compress.shl84 = shl i32 %fuzz.cfg.bitdeposit.idiom.bit83, 25
  %fuzz.cfg.bitdeposit.idiom.compress85 = or i32 %fuzz.cfg.bitdeposit.idiom.compress78, %fuzz.cfg.bitdeposit.idiom.compress.shl84
  %fuzz.cfg.bitdeposit.idiom.deposit.shl86 = shl i32 %fuzz.cfg.bitdeposit.idiom.bit83, 17
  %fuzz.cfg.bitdeposit.idiom.deposit87 = or i32 %fuzz.cfg.bitdeposit.idiom.deposit80, %fuzz.cfg.bitdeposit.idiom.deposit.shl86
  %fuzz.cfg.bitdeposit.idiom.parity88 = xor i32 %fuzz.cfg.bitdeposit.idiom.parity81, %fuzz.cfg.bitdeposit.idiom.bit83
  %fuzz.cfg.bitdeposit.idiom.brev = call i32 @llvm.bitreverse.i32(i32 %fuzz.cfg.bitdeposit.idiom.compress85)
  %fuzz.cfg.bitdeposit.idiom.brev.hi = lshr i32 %fuzz.cfg.bitdeposit.idiom.brev, 24
  %fuzz.cfg.bitdeposit.idiom.parity.even = icmp eq i32 %fuzz.cfg.bitdeposit.idiom.parity88, 0
  %fuzz.cfg.bitdeposit.idiom.parity.select = select i1 %fuzz.cfg.bitdeposit.idiom.parity.even, i32 %fuzz.cfg.bitdeposit.idiom.deposit87, i32 %fuzz.cfg.bitdeposit.idiom.brev.hi
  %fuzz.cfg.packunpack.idiom.lo16 = and i32 %fuzz.cfg.bitdeposit.idiom.parity.select, 65535
  %fuzz.cfg.packunpack.idiom.hi16 = and i32 7, 65535
  %fuzz.cfg.packunpack.idiom.hi16.shift = shl i32 %fuzz.cfg.packunpack.idiom.hi16, 16
  %fuzz.cfg.packunpack.idiom.half.merge = or i32 %fuzz.cfg.packunpack.idiom.lo16, %fuzz.cfg.packunpack.idiom.hi16.shift
  %fuzz.cfg.bitcount.idiom.pop.a = call i32 @llvm.ctpop.i32(i32 %fuzz.cfg.packunpack.idiom.half.merge)
  %fuzz.cfg.bitcount.idiom.pop.b = call i32 @llvm.ctpop.i32(i32 -2147483648)
  %fuzz.cfg.bitcount.idiom.pop.cmp = icmp ugt i32 %fuzz.cfg.bitcount.idiom.pop.a, %fuzz.cfg.bitcount.idiom.pop.b
  %fuzz.cfg.bitcount.idiom.pop.select = select i1 %fuzz.cfg.bitcount.idiom.pop.cmp, i32 %fuzz.cfg.packunpack.idiom.half.merge, i32 -2147483648
  %fuzz.loop.multi.exit.key = and i32 %fuzz.cfg.bitcount.idiom.pop.select, 3
  switch i32 %fuzz.loop.multi.exit.key, label %fuzz.loop.multi.continue [
    i32 0, label %fuzz.loop.multi.break.a
    i32 1, label %fuzz.loop.multi.break.b
  ]

fuzz.loop.multi.break.a:                          ; preds = %fuzz.loop.multi.body
  %fuzz.loop.multi.break.a.val = xor i32 %fuzz.cfg.bitcount.idiom.pop.select, %fuzz.loop.iv.multi
  br label %fuzz.loop.multi.exit

fuzz.loop.multi.break.b:                          ; preds = %fuzz.loop.multi.body
  %fuzz.loop.multi.break.b.val = add i32 %fuzz.cfg.bitcount.idiom.pop.select, %fuzz.bitrunmask.idiom.window
  br label %fuzz.loop.multi.exit

fuzz.loop.multi.continue:                         ; preds = %fuzz.loop.multi.body
  %fuzz.loop.multi.acc.next = xor i32 %fuzz.cfg.bitcount.idiom.pop.select, %fuzz.bitrunmask.idiom.window
  %fuzz.loop.multi.next = add i32 %fuzz.loop.iv.multi, 1
  br label %fuzz.loop.multi.header

fuzz.loop.multi.exit:                             ; preds = %fuzz.loop.multi.break.b, %fuzz.loop.multi.break.a, %fuzz.loop.multi.header
  %fuzz.loop.multi.exit.value = phi i32 [ %fuzz.loop.acc.multi, %fuzz.loop.multi.header ], [ %fuzz.loop.multi.break.a.val, %fuzz.loop.multi.break.a ], [ %fuzz.loop.multi.break.b.val, %fuzz.loop.multi.break.b ]
  %fuzz.ashr = ashr i32 %fuzz.loop.multi.exit.value, 11
  %fuzz.bitcount.idiom.smear1.shr = lshr i32 %fuzz.ashr, 1
  %fuzz.bitcount.idiom.smear1 = or i32 %fuzz.ashr, %fuzz.bitcount.idiom.smear1.shr
  %fuzz.bitcount.idiom.smear2.shr = lshr i32 %fuzz.bitcount.idiom.smear1, 2
  %fuzz.bitcount.idiom.smear2 = or i32 %fuzz.bitcount.idiom.smear1, %fuzz.bitcount.idiom.smear2.shr
  %fuzz.bitcount.idiom.smear4.shr = lshr i32 %fuzz.bitcount.idiom.smear2, 4
  %fuzz.bitcount.idiom.smear4 = or i32 %fuzz.bitcount.idiom.smear2, %fuzz.bitcount.idiom.smear4.shr
  %fuzz.bitcount.idiom.smear8.shr = lshr i32 %fuzz.bitcount.idiom.smear4, 8
  %fuzz.bitcount.idiom.smear8 = or i32 %fuzz.bitcount.idiom.smear4, %fuzz.bitcount.idiom.smear8.shr
  %fuzz.bitcount.idiom.smear16.shr = lshr i32 %fuzz.bitcount.idiom.smear8, 16
  %fuzz.bitcount.idiom.smear16 = or i32 %fuzz.bitcount.idiom.smear8, %fuzz.bitcount.idiom.smear16.shr
  %fuzz.bitcount.idiom.smear.pop = call i32 @llvm.ctpop.i32(i32 %fuzz.bitcount.idiom.smear16)
  %fuzz.bitcount.idiom.smear.mix = sub i32 %fuzz.bitcount.idiom.smear16, %fuzz.bitcount.idiom.smear.pop
  %fuzz.loop.nest.trip.mask = and i32 %fuzz.bitcount.idiom.smear.mix, 3
  %fuzz.loop.nest.trip = add i32 %fuzz.loop.nest.trip.mask, 1
  br label %fuzz.loop.nest.header

fuzz.loop.nest.header:                            ; preds = %fuzz.nested.loop.exit, %fuzz.loop.multi.exit
  %fuzz.loop.nest.iv = phi i32 [ 0, %fuzz.loop.multi.exit ], [ %fuzz.loop.nest.next, %fuzz.nested.loop.exit ]
  %fuzz.loop.nest.acc = phi i32 [ %fuzz.bitcount.idiom.smear.mix, %fuzz.loop.multi.exit ], [ %fuzz.loop.nest.acc.add, %fuzz.nested.loop.exit ]
  %fuzz.loop.nest.cond = icmp ult i32 %fuzz.loop.nest.iv, %fuzz.loop.nest.trip
  br i1 %fuzz.loop.nest.cond, label %fuzz.loop.nest.body, label %fuzz.loop.nest.exit

fuzz.loop.nest.body:                              ; preds = %fuzz.loop.nest.header
  br label %fuzz.nested.loop.header

fuzz.nested.loop.header:                          ; preds = %fuzz.nested.loop.continue, %fuzz.loop.nest.body
  %fuzz.loop.iv.inner = phi i32 [ 0, %fuzz.loop.nest.body ], [ %fuzz.loop.next.inner, %fuzz.nested.loop.continue ]
  %fuzz.loop.acc.inner = phi i32 [ %fuzz.loop.nest.acc, %fuzz.loop.nest.body ], [ %fuzz.cfg.avgdiff.idiom.half.sum, %fuzz.nested.loop.continue ]
  %fuzz.loop.cond.inner = icmp ult i32 %fuzz.loop.iv.inner, 1
  br i1 %fuzz.loop.cond.inner, label %fuzz.nested.loop.body, label %fuzz.nested.loop.exit

fuzz.nested.loop.body:                            ; preds = %fuzz.nested.loop.header
  %fuzz.cfg.avgdiff.idiom.lo.a.trunc = trunc i32 %fuzz.loop.acc.inner to i16
  %fuzz.cfg.avgdiff.idiom.lo.a.zext = zext i16 %fuzz.cfg.avgdiff.idiom.lo.a.trunc to i32
  %fuzz.cfg.avgdiff.idiom.lo.b.trunc = trunc i32 %fuzz.bitrunmask.idiom.count.b.xor to i16
  %fuzz.cfg.avgdiff.idiom.lo.b.zext = zext i16 %fuzz.cfg.avgdiff.idiom.lo.b.trunc to i32
  %fuzz.cfg.avgdiff.idiom.hi.a.shr = lshr i32 %fuzz.loop.acc.inner, 16
  %fuzz.cfg.avgdiff.idiom.hi.a.trunc = trunc i32 %fuzz.cfg.avgdiff.idiom.hi.a.shr to i16
  %fuzz.cfg.avgdiff.idiom.hi.a.zext = zext i16 %fuzz.cfg.avgdiff.idiom.hi.a.trunc to i32
  %fuzz.cfg.avgdiff.idiom.hi.b.shr = lshr i32 %fuzz.bitrunmask.idiom.count.b.xor, 16
  %fuzz.cfg.avgdiff.idiom.hi.b.trunc = trunc i32 %fuzz.cfg.avgdiff.idiom.hi.b.shr to i16
  %fuzz.cfg.avgdiff.idiom.hi.b.zext = zext i16 %fuzz.cfg.avgdiff.idiom.hi.b.trunc to i32
  %fuzz.cfg.avgdiff.idiom.half.lo.hi.cmp = icmp ugt i32 %fuzz.cfg.avgdiff.idiom.lo.a.zext, %fuzz.cfg.avgdiff.idiom.lo.b.zext
  %fuzz.cfg.avgdiff.idiom.half.lo.hi.umax = select i1 %fuzz.cfg.avgdiff.idiom.half.lo.hi.cmp, i32 %fuzz.cfg.avgdiff.idiom.lo.a.zext, i32 %fuzz.cfg.avgdiff.idiom.lo.b.zext
  %fuzz.cfg.avgdiff.idiom.half.lo.lo.cmp = icmp ugt i32 %fuzz.cfg.avgdiff.idiom.lo.a.zext, %fuzz.cfg.avgdiff.idiom.lo.b.zext
  %fuzz.cfg.avgdiff.idiom.half.lo.lo.umin = select i1 %fuzz.cfg.avgdiff.idiom.half.lo.lo.cmp, i32 %fuzz.cfg.avgdiff.idiom.lo.b.zext, i32 %fuzz.cfg.avgdiff.idiom.lo.a.zext
  %fuzz.cfg.avgdiff.idiom.half.lo.absdiff = sub i32 %fuzz.cfg.avgdiff.idiom.half.lo.hi.umax, %fuzz.cfg.avgdiff.idiom.half.lo.lo.umin
  %fuzz.cfg.avgdiff.idiom.half.hi.hi.cmp = icmp ugt i32 %fuzz.cfg.avgdiff.idiom.hi.a.zext, %fuzz.cfg.avgdiff.idiom.hi.b.zext
  %fuzz.cfg.avgdiff.idiom.half.hi.hi.umax = select i1 %fuzz.cfg.avgdiff.idiom.half.hi.hi.cmp, i32 %fuzz.cfg.avgdiff.idiom.hi.a.zext, i32 %fuzz.cfg.avgdiff.idiom.hi.b.zext
  %fuzz.cfg.avgdiff.idiom.half.hi.lo.cmp = icmp ugt i32 %fuzz.cfg.avgdiff.idiom.hi.a.zext, %fuzz.cfg.avgdiff.idiom.hi.b.zext
  %fuzz.cfg.avgdiff.idiom.half.hi.lo.umin = select i1 %fuzz.cfg.avgdiff.idiom.half.hi.lo.cmp, i32 %fuzz.cfg.avgdiff.idiom.hi.b.zext, i32 %fuzz.cfg.avgdiff.idiom.hi.a.zext
  %fuzz.cfg.avgdiff.idiom.half.hi.absdiff = sub i32 %fuzz.cfg.avgdiff.idiom.half.hi.hi.umax, %fuzz.cfg.avgdiff.idiom.half.hi.lo.umin
  %fuzz.cfg.avgdiff.idiom.half.sum = add i32 %fuzz.cfg.avgdiff.idiom.half.lo.absdiff, %fuzz.cfg.avgdiff.idiom.half.hi.absdiff
  %fuzz.loop.break.inner = icmp uge i32 %fuzz.cfg.avgdiff.idiom.half.sum, %fuzz.bitrunmask.idiom.count.b.xor
  br i1 %fuzz.loop.break.inner, label %fuzz.nested.loop.exit, label %fuzz.nested.loop.continue

fuzz.nested.loop.continue:                        ; preds = %fuzz.nested.loop.body
  %fuzz.loop.next.inner = add i32 %fuzz.loop.iv.inner, 1
  br label %fuzz.nested.loop.header

fuzz.nested.loop.exit:                            ; preds = %fuzz.nested.loop.body, %fuzz.nested.loop.header
  %fuzz.loop.exit.value.inner = phi i32 [ %fuzz.loop.acc.inner, %fuzz.nested.loop.header ], [ %fuzz.cfg.avgdiff.idiom.half.sum, %fuzz.nested.loop.body ]
  %fuzz.loop.nest.acc.add = add i32 %fuzz.loop.exit.value.inner, %fuzz.bitrunmask.idiom.count.b.xor
  %fuzz.loop.nest.next = add i32 %fuzz.loop.nest.iv, 1
  br label %fuzz.loop.nest.header

fuzz.loop.nest.exit:                              ; preds = %fuzz.loop.nest.header
  %fuzz.vop3fused.idiom.mix = xor i32 %fuzz.loop.nest.acc, %fuzz.bitrunmask.idiom.count.a.add
  %fuzz.vop3fused.idiom.c = add i32 %fuzz.vop3fused.idiom.mix, -1640531535
  %fuzz.vop3fused.idiom.ab.add = add i32 %fuzz.loop.nest.acc, %fuzz.bitrunmask.idiom.count.a.add
  %fuzz.vop3fused.idiom.add3 = add i32 %fuzz.vop3fused.idiom.ab.add, %fuzz.vop3fused.idiom.c
  %fuzz.vop3fused.idiom.a.shl1 = shl i32 %fuzz.loop.nest.acc, 2
  %fuzz.vop3fused.idiom.lshl.add = add i32 %fuzz.vop3fused.idiom.a.shl1, %fuzz.bitrunmask.idiom.count.a.add
  %fuzz.vop3fused.idiom.ab.add2 = add i32 %fuzz.loop.nest.acc, %fuzz.bitrunmask.idiom.count.a.add
  %fuzz.vop3fused.idiom.add.lshl = shl i32 %fuzz.vop3fused.idiom.ab.add2, 5
  %fuzz.vop3fused.idiom.a.shl3 = shl i32 %fuzz.loop.nest.acc, 7
  %fuzz.vop3fused.idiom.lshl.or = or i32 %fuzz.vop3fused.idiom.a.shl3, %fuzz.bitrunmask.idiom.count.a.add
  %fuzz.vop3fused.idiom.ab.and = and i32 %fuzz.loop.nest.acc, %fuzz.bitrunmask.idiom.count.a.add
  %fuzz.vop3fused.idiom.and.or = or i32 %fuzz.vop3fused.idiom.ab.and, %fuzz.vop3fused.idiom.c
  %fuzz.vop3fused.idiom.ab.or = or i32 %fuzz.loop.nest.acc, %fuzz.bitrunmask.idiom.count.a.add
  %fuzz.vop3fused.idiom.or3 = or i32 %fuzz.vop3fused.idiom.ab.or, %fuzz.vop3fused.idiom.c
  %fuzz.vop3fused.idiom.max.ab = call i32 @llvm.umax.i32(i32 %fuzz.loop.nest.acc, i32 %fuzz.bitrunmask.idiom.count.a.add)
  %fuzz.vop3fused.idiom.med3 = call i32 @llvm.umin.i32(i32 %fuzz.vop3fused.idiom.max.ab, i32 %fuzz.vop3fused.idiom.c)
  %fuzz.vop3fused.idiom.acc.lshl.add = add i32 %fuzz.vop3fused.idiom.add3, %fuzz.vop3fused.idiom.lshl.add
  %fuzz.vop3fused.idiom.acc.add.lshl = xor i32 %fuzz.vop3fused.idiom.acc.lshl.add, %fuzz.vop3fused.idiom.add.lshl
  %fuzz.vop3fused.idiom.acc.lshl.or = add i32 %fuzz.vop3fused.idiom.acc.add.lshl, %fuzz.vop3fused.idiom.lshl.or
  %fuzz.vop3fused.idiom.acc.and.or = xor i32 %fuzz.vop3fused.idiom.acc.lshl.or, %fuzz.vop3fused.idiom.and.or
  %fuzz.vop3fused.idiom.acc.or3 = add i32 %fuzz.vop3fused.idiom.acc.and.or, %fuzz.vop3fused.idiom.or3
  %fuzz.vop3fused.idiom.acc.med3 = xor i32 %fuzz.vop3fused.idiom.acc.or3, %fuzz.vop3fused.idiom.med3
  %fuzz.vop3fused.idiom.a.xor = xor i32 %fuzz.vop3fused.idiom.acc.med3, %fuzz.loop.nest.acc
  br label %fuzz.loop.header

fuzz.loop.header:                                 ; preds = %fuzz.loop.body, %fuzz.loop.nest.exit
  %fuzz.loop.iv = phi i32 [ 0, %fuzz.loop.nest.exit ], [ %fuzz.loop.next, %fuzz.loop.body ]
  %fuzz.loop.acc = phi i32 [ %fuzz.vop3fused.idiom.a.xor, %fuzz.loop.nest.exit ], [ %fuzz.cfg.umaxbitop3cascade.idiom.acc.next18, %fuzz.loop.body ]
  %fuzz.loop.cond = icmp ult i32 %fuzz.loop.iv, 3
  br i1 %fuzz.loop.cond, label %fuzz.loop.body, label %fuzz.loop.exit

fuzz.loop.body:                                   ; preds = %fuzz.loop.header
  %fuzz.cfg.umaxbitop3cascade.idiom.mix = xor i32 %fuzz.loop.acc, %fuzz.bitrunmask.idiom.merge
  %fuzz.cfg.umaxbitop3cascade.idiom.not.a = xor i32 %fuzz.loop.acc, -1
  %fuzz.cfg.umaxbitop3cascade.idiom.not.b = xor i32 %fuzz.bitrunmask.idiom.merge, -1
  %fuzz.cfg.umaxbitop3cascade.idiom.b.shr = lshr i32 %fuzz.bitrunmask.idiom.merge, 1
  %fuzz.cfg.umaxbitop3cascade.idiom.umax.cmp = icmp ugt i32 %fuzz.cfg.umaxbitop3cascade.idiom.b.shr, %fuzz.cfg.umaxbitop3cascade.idiom.not.b
  %fuzz.cfg.umaxbitop3cascade.idiom.umax.umax = select i1 %fuzz.cfg.umaxbitop3cascade.idiom.umax.cmp, i32 %fuzz.cfg.umaxbitop3cascade.idiom.b.shr, i32 %fuzz.cfg.umaxbitop3cascade.idiom.not.b
  %fuzz.cfg.umaxbitop3cascade.idiom.umin.cmp = icmp ugt i32 %fuzz.cfg.umaxbitop3cascade.idiom.b.shr, %fuzz.cfg.umaxbitop3cascade.idiom.not.b
  %fuzz.cfg.umaxbitop3cascade.idiom.umin.umin = select i1 %fuzz.cfg.umaxbitop3cascade.idiom.umin.cmp, i32 %fuzz.cfg.umaxbitop3cascade.idiom.not.b, i32 %fuzz.cfg.umaxbitop3cascade.idiom.b.shr
  %fuzz.cfg.umaxbitop3cascade.idiom.min.or.x = or i32 %fuzz.cfg.umaxbitop3cascade.idiom.umin.umin, %fuzz.cfg.umaxbitop3cascade.idiom.b.shr
  %fuzz.cfg.umaxbitop3cascade.idiom.lane.xor = xor i32 %fuzz.cfg.umaxbitop3cascade.idiom.min.or.x, %fuzz.cfg.umaxbitop3cascade.idiom.not.b
  %fuzz.cfg.umaxbitop3cascade.idiom.acc.xor = xor i32 0, %fuzz.cfg.umaxbitop3cascade.idiom.lane.xor
  %fuzz.cfg.umaxbitop3cascade.idiom.acc.next = add i32 %fuzz.cfg.umaxbitop3cascade.idiom.acc.xor, 1
  %fuzz.cfg.umaxbitop3cascade.idiom.a.shl = shl i32 %fuzz.loop.acc, 2
  %fuzz.cfg.umaxbitop3cascade.idiom.umax.cmp1 = icmp ugt i32 %fuzz.cfg.umaxbitop3cascade.idiom.a.shl, %fuzz.cfg.umaxbitop3cascade.idiom.mix
  %fuzz.cfg.umaxbitop3cascade.idiom.umax.umax2 = select i1 %fuzz.cfg.umaxbitop3cascade.idiom.umax.cmp1, i32 %fuzz.cfg.umaxbitop3cascade.idiom.a.shl, i32 %fuzz.cfg.umaxbitop3cascade.idiom.mix
  %fuzz.cfg.umaxbitop3cascade.idiom.umin.cmp3 = icmp ugt i32 %fuzz.cfg.umaxbitop3cascade.idiom.a.shl, %fuzz.cfg.umaxbitop3cascade.idiom.mix
  %fuzz.cfg.umaxbitop3cascade.idiom.umin.umin4 = select i1 %fuzz.cfg.umaxbitop3cascade.idiom.umin.cmp3, i32 %fuzz.cfg.umaxbitop3cascade.idiom.mix, i32 %fuzz.cfg.umaxbitop3cascade.idiom.a.shl
  %fuzz.cfg.umaxbitop3cascade.idiom.max.xor.x = xor i32 %fuzz.cfg.umaxbitop3cascade.idiom.umax.umax2, %fuzz.cfg.umaxbitop3cascade.idiom.a.shl
  %fuzz.cfg.umaxbitop3cascade.idiom.lane.and = and i32 %fuzz.cfg.umaxbitop3cascade.idiom.max.xor.x, %fuzz.cfg.umaxbitop3cascade.idiom.mix
  %fuzz.cfg.umaxbitop3cascade.idiom.acc.xor5 = xor i32 %fuzz.cfg.umaxbitop3cascade.idiom.acc.next, %fuzz.cfg.umaxbitop3cascade.idiom.lane.and
  %fuzz.cfg.umaxbitop3cascade.idiom.acc.next6 = add i32 %fuzz.cfg.umaxbitop3cascade.idiom.acc.xor5, 59
  %fuzz.cfg.umaxbitop3cascade.idiom.mix.and.nota = and i32 %fuzz.cfg.umaxbitop3cascade.idiom.mix, %fuzz.cfg.umaxbitop3cascade.idiom.not.a
  %fuzz.cfg.umaxbitop3cascade.idiom.umax.cmp7 = icmp ugt i32 %fuzz.cfg.umaxbitop3cascade.idiom.mix.and.nota, %fuzz.bitrunmask.idiom.merge
  %fuzz.cfg.umaxbitop3cascade.idiom.umax.umax8 = select i1 %fuzz.cfg.umaxbitop3cascade.idiom.umax.cmp7, i32 %fuzz.cfg.umaxbitop3cascade.idiom.mix.and.nota, i32 %fuzz.bitrunmask.idiom.merge
  %fuzz.cfg.umaxbitop3cascade.idiom.umin.cmp9 = icmp ugt i32 %fuzz.cfg.umaxbitop3cascade.idiom.mix.and.nota, %fuzz.bitrunmask.idiom.merge
  %fuzz.cfg.umaxbitop3cascade.idiom.umin.umin10 = select i1 %fuzz.cfg.umaxbitop3cascade.idiom.umin.cmp9, i32 %fuzz.bitrunmask.idiom.merge, i32 %fuzz.cfg.umaxbitop3cascade.idiom.mix.and.nota
  %fuzz.cfg.umaxbitop3cascade.idiom.max.xor.y = xor i32 %fuzz.cfg.umaxbitop3cascade.idiom.umax.umax8, %fuzz.bitrunmask.idiom.merge
  %fuzz.cfg.umaxbitop3cascade.idiom.not.x = xor i32 %fuzz.cfg.umaxbitop3cascade.idiom.mix.and.nota, -1
  %fuzz.cfg.umaxbitop3cascade.idiom.max.xor.y.and.notx = and i32 %fuzz.cfg.umaxbitop3cascade.idiom.max.xor.y, %fuzz.cfg.umaxbitop3cascade.idiom.not.x
  %fuzz.cfg.umaxbitop3cascade.idiom.lane.xor2 = xor i32 %fuzz.cfg.umaxbitop3cascade.idiom.max.xor.y.and.notx, %fuzz.cfg.umaxbitop3cascade.idiom.umin.umin10
  %fuzz.cfg.umaxbitop3cascade.idiom.acc.xor11 = xor i32 %fuzz.cfg.umaxbitop3cascade.idiom.acc.next6, %fuzz.cfg.umaxbitop3cascade.idiom.lane.xor2
  %fuzz.cfg.umaxbitop3cascade.idiom.acc.next12 = add i32 %fuzz.cfg.umaxbitop3cascade.idiom.acc.xor11, 117
  %fuzz.cfg.umaxbitop3cascade.idiom.mix.shr = lshr i32 %fuzz.cfg.umaxbitop3cascade.idiom.mix, 14
  %fuzz.cfg.umaxbitop3cascade.idiom.a.or.mix = or i32 %fuzz.loop.acc, %fuzz.cfg.umaxbitop3cascade.idiom.mix.shr
  %fuzz.cfg.umaxbitop3cascade.idiom.umax.cmp13 = icmp ugt i32 %fuzz.cfg.umaxbitop3cascade.idiom.a.or.mix, %fuzz.cfg.umaxbitop3cascade.idiom.not.a
  %fuzz.cfg.umaxbitop3cascade.idiom.umax.umax14 = select i1 %fuzz.cfg.umaxbitop3cascade.idiom.umax.cmp13, i32 %fuzz.cfg.umaxbitop3cascade.idiom.a.or.mix, i32 %fuzz.cfg.umaxbitop3cascade.idiom.not.a
  %fuzz.cfg.umaxbitop3cascade.idiom.umin.cmp15 = icmp ugt i32 %fuzz.cfg.umaxbitop3cascade.idiom.a.or.mix, %fuzz.cfg.umaxbitop3cascade.idiom.not.a
  %fuzz.cfg.umaxbitop3cascade.idiom.umin.umin16 = select i1 %fuzz.cfg.umaxbitop3cascade.idiom.umin.cmp15, i32 %fuzz.cfg.umaxbitop3cascade.idiom.not.a, i32 %fuzz.cfg.umaxbitop3cascade.idiom.a.or.mix
  %fuzz.cfg.umaxbitop3cascade.idiom.min.and.max = and i32 %fuzz.cfg.umaxbitop3cascade.idiom.umin.umin16, %fuzz.cfg.umaxbitop3cascade.idiom.umax.umax14
  %fuzz.cfg.umaxbitop3cascade.idiom.x.xor.y = xor i32 %fuzz.cfg.umaxbitop3cascade.idiom.a.or.mix, %fuzz.cfg.umaxbitop3cascade.idiom.not.a
  %fuzz.cfg.umaxbitop3cascade.idiom.lane.or = or i32 %fuzz.cfg.umaxbitop3cascade.idiom.min.and.max, %fuzz.cfg.umaxbitop3cascade.idiom.x.xor.y
  %fuzz.cfg.umaxbitop3cascade.idiom.acc.xor17 = xor i32 %fuzz.cfg.umaxbitop3cascade.idiom.acc.next12, %fuzz.cfg.umaxbitop3cascade.idiom.lane.or
  %fuzz.cfg.umaxbitop3cascade.idiom.acc.next18 = add i32 %fuzz.cfg.umaxbitop3cascade.idiom.acc.xor17, 175
  %fuzz.cfg.umaxbitop3cascade.idiom.final.ashr = ashr i32 %fuzz.cfg.umaxbitop3cascade.idiom.acc.next18, 31
  %fuzz.loop.next = add i32 %fuzz.loop.iv, 1
  br label %fuzz.loop.header

fuzz.loop.exit:                                   ; preds = %fuzz.loop.header
  store i32 %fuzz.loop.acc, ptr addrspace(1) %out.ptr, align 4
  br label %exit

exit:                                             ; preds = %fuzz.loop.exit, %entry
  ret void
}

; Function Attrs: nocallback nofree nosync nounwind speculatable willreturn memory(none)
declare noundef i32 @llvm.amdgcn.workgroup.id.x() #1

; Function Attrs: nocallback nofree nosync nounwind speculatable willreturn memory(none)
declare noundef range(i32 0, 1024) i32 @llvm.amdgcn.workitem.id.x() #1

; Function Attrs: nocallback nocreateundeforpoison nofree nosync nounwind speculatable willreturn memory(none)
declare i32 @llvm.umin.i32(i32, i32) #2

; Function Attrs: nocallback nocreateundeforpoison nofree nosync nounwind speculatable willreturn memory(none)
declare i32 @llvm.bitreverse.i32(i32) #2

; Function Attrs: nocallback nocreateundeforpoison nofree nosync nounwind speculatable willreturn memory(none)
declare i32 @llvm.ctpop.i32(i32) #2

; Function Attrs: nocallback nocreateundeforpoison nofree nosync nounwind speculatable willreturn memory(none)
declare i32 @llvm.umax.i32(i32, i32) #2

attributes #0 = { convergent nounwind "amdgpu-flat-work-group-size"="1,256" "target-cpu"="gfx950" "uniform-work-group-size"="true" }
attributes #1 = { nocallback nofree nosync nounwind speculatable willreturn memory(none) }
attributes #2 = { nocallback nocreateundeforpoison nofree nosync nounwind speculatable willreturn memory(none) }

!llvm.module.flags = !{!0, !1, !2}

!0 = !{i32 1, !"amdhsa_code_object_version", i32 600}
!1 = !{i32 1, !"amdgpu_printf_kind", !"hostcall"}
!2 = !{i32 8, !"PIC Level", i32 2}
