# 036 - IV simplification exposes redundant-assume GEP `nuw` inference

Component: `llvm/test/Transforms/InstCombine/assume-redundant.ll`

The first iteration simplifies the induction variable shape. Only after that
does InstCombine infer `nuw` on a GEP from the alignment assumptions and
canonical IV facts. The upstream test suppresses fixpoint verification for
this function; this standalone reproducer keeps it enabled and aborts.

Verifier: Singer (019e9951-5384-7a91-925d-83b08c3f3d36) returned YES.
