; ModuleID = '/Users/justinlebar/code/FuzzX/NVPTX/scratch/r4_03-refute.ll'
source_filename = "/Users/justinlebar/code/FuzzX/NVPTX/scratch/r4_03-refute.ll"
target datalayout = "e-p6:32:32-i64:64-i128:128-i256:256-v16:16-v32:32-n16:32:64"
target triple = "nvptx64-nvidia-cuda"

; Function Attrs: mustprogress nofree norecurse nosync nounwind willreturn denormal_fpenv(float: preservesign) memory(none)
define half @t(half %a, half %b) local_unnamed_addr #0 {
  %r = tail call half @llvm.minimumnum.f16(half %a, half %b)
  ret half %r
}

; Function Attrs: nocallback nocreateundeforpoison nofree nosync nounwind speculatable willreturn memory(none)
declare half @llvm.minimumnum.f16(half, half) #1

attributes #0 = { mustprogress nofree norecurse nosync nounwind willreturn denormal_fpenv(float: preservesign) memory(none) }
attributes #1 = { nocallback nocreateundeforpoison nofree nosync nounwind speculatable willreturn memory(none) }
