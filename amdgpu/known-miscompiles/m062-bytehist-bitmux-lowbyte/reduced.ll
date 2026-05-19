; ModuleID = '<bc file>'
; RUN-INPUTS: 0x0 0x1
; RUN-LLVM-BUILD: build/llvm-fuzzer
source_filename = "/tmp/fuzzx-smoke-20260519-gen20-features/corpus/directed/.seed-244593.ll"
target datalayout = "e-m:e-p:64:64-p1:64:64-p2:32:32-p3:32:32-p4:64:64-p5:32:32-p6:32:32-p7:160:256:256:32-p8:128:128:128:48-p9:192:256:256:32-i64:64-v16:16-v24:32-v32:32-v48:64-v96:128-v192:256-v256:256-v512:512-v1024:1024-v2048:2048-n32:64-S32-A5-G1-ni:7:8:9"
target triple = "amdgcn-amd-amdhsa"

; Function Attrs: convergent nounwind
define amdgpu_kernel void @fuzz_kernel(ptr addrspace(1) %in, ptr addrspace(1) %out, i32 %fuzz.bitmux.idiom.fold.add55) #0 {
entry:
  %wi = call i32 @llvm.amdgcn.workitem.id.x()
  %idx64 = zext i32 1 to i64
  %in.ptr = getelementptr i32, ptr addrspace(1) %in, i64 %idx64
  %v = load i32, ptr addrspace(1) %in.ptr, align 4
  %salt = mul i32 1, -1640531527
  %mix = xor i32 %v, %salt
  %out.ptr = getelementptr i32, ptr addrspace(1) %out, i64 %idx64
  %fuzz.bytesatpack.idiom.x.byte.trunc = trunc i32 %mix to i8
  %fuzz.bytesatpack.idiom.x.byte.zext = zext i8 %fuzz.bytesatpack.idiom.x.byte.trunc to i32
  %fuzz.bytesatpack.idiom.lane.byte = and i32 %fuzz.bytesatpack.idiom.x.byte.zext, 255
  %fuzz.bytesatpack.idiom.x.byte.shr17 = lshr i32 %mix, 1
  %fuzz.bytesatpack.idiom.x.byte.trunc18 = trunc i32 %fuzz.bytesatpack.idiom.x.byte.shr17 to i8
  %fuzz.bytesatpack.idiom.x.byte.zext19 = zext i8 %fuzz.bytesatpack.idiom.x.byte.trunc18 to i32
  %fuzz.bytesatpack.idiom.lane.byte31 = and i32 %fuzz.bytesatpack.idiom.x.byte.zext19, 255
  %fuzz.bytesatpack.idiom.x.byte.shr35 = lshr i32 %mix, 1
  %fuzz.bytesatpack.idiom.x.byte.trunc36 = trunc i32 %fuzz.bytesatpack.idiom.x.byte.shr35 to i8
  %fuzz.bytesatpack.idiom.x.byte.zext37 = zext i8 %fuzz.bytesatpack.idiom.x.byte.trunc36 to i32
  %fuzz.bytesatpack.idiom.lane.byte51 = and i32 %fuzz.bytesatpack.idiom.x.byte.zext37, 255
  %fuzz.bytesatpack.idiom.pack.shift58 = shl i32 %fuzz.bytesatpack.idiom.lane.byte31, 16
  %fuzz.bytesatpack.idiom.pack.add59 = add i32 %fuzz.bytesatpack.idiom.lane.byte, %fuzz.bytesatpack.idiom.pack.shift58
  %fuzz.bytesatpack.idiom.pack.shift61 = shl i32 %fuzz.bytesatpack.idiom.lane.byte51, 8
  %fuzz.bytesatpack.idiom.pack.add62 = add i32 %fuzz.bytesatpack.idiom.pack.add59, %fuzz.bytesatpack.idiom.pack.shift61
  %fuzz.bitmux.idiom.ctpop = call i32 @llvm.ctpop.i32(i32 %fuzz.bytesatpack.idiom.pack.add62)
  %fuzz.bitmux.idiom.bswap = call i32 @llvm.bswap.i32(i32 %fuzz.bytesatpack.idiom.lane.byte)
  %fuzz.bitmux.idiom.x.next = xor i32 %fuzz.bitmux.idiom.bswap, %fuzz.bitmux.idiom.ctpop
  %fuzz.bitmux.idiom.y.next = add i32 0, 0
  %fuzz.bitmux.idiom.ctpop1 = call i32 @llvm.ctpop.i32(i32 %fuzz.bitmux.idiom.x.next)
  %fuzz.bitmux.idiom.y8 = and i32 0, 0
  %fuzz.bitmux.idiom.shift9 = shl i32 %fuzz.bitmux.idiom.x.next, 17
  %fuzz.bitmux.idiom.fold.add10 = add i32 %fuzz.bitmux.idiom.ctpop, %fuzz.bitmux.idiom.ctpop1
  %fuzz.bitmux.idiom.fold.next11 = xor i32 %fuzz.bitmux.idiom.fold.add10, %fuzz.bitmux.idiom.shift9
  %fuzz.bitmux.idiom.x.next12 = xor i32 %fuzz.bitmux.idiom.x.next, %fuzz.bitmux.idiom.fold.next11
  %fuzz.bitmux.idiom.ctpop14 = call i32 @llvm.ctpop.i32(i32 %fuzz.bitmux.idiom.x.next12)
  %fuzz.bitmux.idiom.fold.add25 = add i32 %fuzz.bitmux.idiom.fold.next11, %fuzz.bitmux.idiom.ctpop14
  %fuzz.bitmux.idiom.fold.next26 = xor i32 %fuzz.bitmux.idiom.fold.add25, %fuzz.bitmux.idiom.x.next12
  %fuzz.bitmux.idiom.ctpop29 = call i32 @llvm.ctpop.i32(i32 %fuzz.bitmux.idiom.fold.add25)
  %fuzz.bitmux.idiom.shift39 = lshr i32 %fuzz.bitmux.idiom.fold.add25, 16
  %fuzz.bitmux.idiom.fold.add40 = add i32 %fuzz.bitmux.idiom.fold.next26, %fuzz.bitmux.idiom.ctpop29
  %fuzz.bitmux.idiom.fold.next41 = xor i32 %fuzz.bitmux.idiom.fold.add40, %fuzz.bitmux.idiom.shift39
  %fuzz.bitmux.idiom.x.next42 = xor i32 %fuzz.bitmux.idiom.fold.add25, %fuzz.bitmux.idiom.fold.next41
  %fuzz.bitmux.idiom.ctpop44 = call i32 @llvm.ctpop.i32(i32 %fuzz.bitmux.idiom.x.next42)
  %fuzz.bitmux.idiom.shift54 = lshr i32 %fuzz.bitmux.idiom.x.next42, 0
  %fuzz.bitmux.idiom.fold.add551 = add i32 0, %fuzz.bitmux.idiom.ctpop44
  %fuzz.bitmux.idiom.fold.next56 = xor i32 %fuzz.bitmux.idiom.fold.add55, %fuzz.bitmux.idiom.shift54
  %fuzz.bitmux.idiom.x.next57 = xor i32 %fuzz.bitmux.idiom.x.next42, %fuzz.bitmux.idiom.fold.next56
  %fuzz.bitmux.idiom.x.xor = xor i32 %fuzz.bitmux.idiom.fold.next56, %fuzz.bitmux.idiom.x.next57
  %fuzz.bytehist.idiom.x.byte.trunc = trunc i32 %fuzz.bitmux.idiom.x.xor to i8
  %fuzz.bytehist.idiom.x.byte.zext = zext i8 %fuzz.bytehist.idiom.x.byte.trunc to i32
  %fuzz.bytehist.idiom.absdiff.hi.cmp = icmp ugt i32 %fuzz.bytehist.idiom.x.byte.zext, 0
  %fuzz.bytehist.idiom.absdiff.hi.umax = select i1 %fuzz.bytehist.idiom.absdiff.hi.cmp, i32 %fuzz.bytehist.idiom.x.byte.zext, i32 1
  %fuzz.bytehist.idiom.absdiff.lo.cmp = icmp ugt i32 %fuzz.bytehist.idiom.x.byte.zext, 0
  %fuzz.bytehist.idiom.absdiff.lo.umin = select i1 %fuzz.bytehist.idiom.absdiff.lo.cmp, i32 1, i32 0
  %fuzz.bytehist.idiom.absdiff.absdiff = sub i32 %fuzz.bytehist.idiom.absdiff.hi.umax, %fuzz.bytehist.idiom.absdiff.lo.umin
  %fuzz.bytehist.idiom.lane.byte = and i32 %fuzz.bytehist.idiom.absdiff.absdiff, 255
  %fuzz.bytehist.idiom.x.byte.shr30 = lshr i32 %fuzz.bitmux.idiom.x.xor, 16
  %fuzz.bytehist.idiom.x.byte.trunc31 = trunc i32 %fuzz.bytehist.idiom.x.byte.shr30 to i8
  %fuzz.bytehist.idiom.x.byte.zext32 = zext i8 %fuzz.bytehist.idiom.x.byte.trunc31 to i32
  %fuzz.bytehist.idiom.absdiff.hi.cmp51 = icmp ugt i32 0, 0
  %fuzz.bytehist.idiom.lane.byte61 = and i32 %fuzz.bytehist.idiom.x.byte.zext32, 255
  %fuzz.bytehist.idiom.y.byte.shr65 = lshr i32 %fuzz.bitmux.idiom.x.xor, 24
  %fuzz.bytehist.idiom.y.byte.trunc66 = trunc i32 %fuzz.bytehist.idiom.y.byte.shr65 to i8
  %fuzz.bytehist.idiom.y.byte.zext67 = zext i8 %fuzz.bytehist.idiom.y.byte.trunc66 to i32
  %fuzz.bytehist.idiom.lane.byte93 = and i32 %fuzz.bytehist.idiom.y.byte.zext67, 255
  %fuzz.bytehist.idiom.pack.shift97 = shl i32 %fuzz.bytehist.idiom.lane.byte61, 16
  %fuzz.bytehist.idiom.pack.add98 = add i32 %fuzz.bytehist.idiom.lane.byte, %fuzz.bytehist.idiom.pack.shift97
  %fuzz.bytehist.idiom.pack.shift100 = shl i32 %fuzz.bytehist.idiom.lane.byte93, 24
  %fuzz.bytehist.idiom.pack.add101 = add i32 %fuzz.bytehist.idiom.pack.add98, %fuzz.bytehist.idiom.pack.shift100
  store i32 %fuzz.bytehist.idiom.pack.add101, ptr addrspace(1) %out.ptr, align 4
  ret void
}

; Function Attrs: nocallback nofree nosync nounwind speculatable willreturn memory(none)
declare noundef i32 @llvm.amdgcn.workgroup.id.x() #1

; Function Attrs: nocallback nofree nosync nounwind speculatable willreturn memory(none)
declare noundef range(i32 0, 1024) i32 @llvm.amdgcn.workitem.id.x() #1

; Function Attrs: nocallback nofree nosync nounwind speculatable willreturn memory(none)
declare i32 @llvm.ctpop.i32(i32) #2

; Function Attrs: nocallback nofree nosync nounwind speculatable willreturn memory(none)
declare i32 @llvm.bswap.i32(i32) #2

; uselistorder directives
uselistorder ptr @llvm.ctpop.i32, { 4, 3, 2, 1, 0 }

attributes #0 = { convergent nounwind "amdgpu-flat-work-group-size"="1,256" "target-cpu"="gfx950" "uniform-work-group-size" }
attributes #1 = { nocallback nofree nosync nounwind speculatable willreturn memory(none) }
attributes #2 = { nocallback nofree nosync nounwind speculatable willreturn memory(none) }
