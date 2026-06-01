; ModuleID = '/Users/justinlebar/code/FuzzX/NVPTX/scratch/r4_03.ll'
source_filename = "/Users/justinlebar/code/FuzzX/NVPTX/scratch/r4_03.ll"
target datalayout = "e-p6:32:32-i64:64-i128:128-i256:256-v16:16-v32:32-n16:32:64"
target triple = "nvptx64-nvidia-cuda"

; Function Attrs: nocallback nocreateundeforpoison nofree nosync nounwind speculatable willreturn memory(none)
declare half @llvm.nvvm.fmin.f16(half, half) #0

; Function Attrs: denormal_fpenv(float: preservesign)
define half @t(half %a, half %b) #1 {
  %r = call half @llvm.minimumnum.f16(half %a, half %b)
  ret half %r
}

; Function Attrs: nocallback nocreateundeforpoison nofree nosync nounwind speculatable willreturn memory(none)
declare half @llvm.minimumnum.f16(half, half) #0

attributes #0 = { nocallback nocreateundeforpoison nofree nosync nounwind speculatable willreturn memory(none) }
attributes #1 = { denormal_fpenv(float: preservesign) }
