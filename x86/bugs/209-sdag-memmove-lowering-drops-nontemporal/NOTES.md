# 209 — SelectionDAG `getMemmoveLoadsAndStores` drops `!nontemporal` from `llvm.memmove`

Component: `llvm/lib/CodeGen/SelectionDAG/SelectionDAG.cpp` lines ~9520-9521; sibling of #208.

Same defect as #208 in the memmove path. Per-chunk loads/stores never receive the MONonTemporal flag set on the original intrinsic call.

## Reproducer / fix: same shape as #208.
