# Candidate: visitSIGN_EXTEND_INREG combines extload to sextload despite multiple uses

File: llvm/lib/CodeGen/SelectionDAG/DAGCombiner.cpp:16843-16857

## Reasoning

```cpp
if (ISD::isEXTLoad(N0.getNode()) && ISD::isUNINDEXEDLoad(N0.getNode())) {
  auto *LN0 = cast<LoadSDNode>(N0);
  if (ExtVT == LN0->getMemoryVT() &&
      ((!LegalOperations && LN0->isSimple() && N0.hasOneUse()) ||
       TLI.isLoadLegal(VT, ExtVT, ..., ISD::SEXTLOAD, false))) {
    SDValue ExtLoad = DAG.getExtLoad(ISD::SEXTLOAD, ...);
    CombineTo(N, ExtLoad);
    CombineTo(N0.getNode(), ExtLoad, ExtLoad.getValue(1));
    ...
```

When the OR-branch fires (target says SEXTLOAD is legal), `N0.hasOneUse()` is
not checked. The original is an `EXTLOAD` (any-extend semantics — high bits are
undef). The combine replaces it with `SEXTLOAD` everywhere. For one consumer
this is fine because high bits are undef, but `CombineTo(N0, ExtLoad, ...)`
rewrites *all* users of the original extload's value to the new sextload. If
some other user is `and X, mask` that previously knew the high bits could be
freely zero (an `and` would have been treated as no-op via known bits since
EXTLOAD's high bits are undef-treated-as-zero by some analyses), the
downstream KnownBits analysis may treat the sextload's high bits as sign-bit
copies of the loaded value, changing the result of subsequent simplifications.

The bug is not "wrong result" but "lost optimization" in most cases, but
combined with multi-result combines this can introduce inconsistencies.
Stronger version: another user might be cast to a different extension
(zextload), and the rewrite to sextload changes its observable bits.

## Candidate IR (llc -mtriple=x86_64)

```ll
define i64 @t(ptr %p, ptr %q) {
  %ld = load i16, ptr %p
  %s32 = sext i16 %ld to i32
  store i32 %s32, ptr %q          ; uses extload result via sext_inreg
  %z64 = zext i16 %ld to i64       ; second use sees the EXTLOAD value
  ret i64 %z64
}
```

After the SEXT_INREG combine substitutes the zextload with sextload (or
swap), the second user's zext semantics may produce sign-extended bits
rather than zero-extended bits.

## Expected wrong outcome

`%z64` may be observed as the sign-extended value rather than zero-extended
for negative-valued `%ld`. Hard to trigger without target-specific knobs
but worth fuzzing.
