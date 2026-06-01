target triple = "nvptx64-nvidia-cuda"

define i32 @vote_ballot(i1 %pred) {
  %r = call i32 @llvm.nvvm.vote.ballot(i1 %pred)
  ret i32 %r
}
define i1 @vote_all(i1 %pred) {
  %r = call i1 @llvm.nvvm.vote.all(i1 %pred)
  ret i1 %r
}
declare i32 @llvm.nvvm.vote.ballot(i1)
declare i1 @llvm.nvvm.vote.all(i1)
