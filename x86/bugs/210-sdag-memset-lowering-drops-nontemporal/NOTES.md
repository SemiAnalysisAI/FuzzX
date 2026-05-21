# 210 — SelectionDAG `getMemsetStores` drops `!nontemporal` from `llvm.memset`

Component: `llvm/lib/CodeGen/SelectionDAG/SelectionDAG.cpp` lines ~9747-9751; sibling of #208/#209.

Per-chunk stores never receive MONonTemporal from the original intrinsic. The X86 selector therefore emits plain `MOVAPSmr`/`MOV*mr` instead of NT-class stores even when the source said `__builtin_memset_inline + nontemporal`.

## Reproducer / fix: same shape as #208.
