# 036 — Non-sync vote.{all,any,uni,ballot} has no upper-arch predicate -> emits invalid PTX (removed on sm_70+/PTX6.4+), while the sibling non-sync shfl is correctly guarded by hasSHFL

- **Kind:** other (invalid PTX)
- **Reachable via:** llc -mcpu=sm_70+
- **Component:** NVPTXIntrinsics.td 236-246  (round-6 area `W13-feature-predicate-sweep`)
- **Note:** "invalid PTX" entries are validated against the PTX ISA + strong in-tree corroboration (sibling guards / orderings); no local `ptxas` was available to execute the rejection.

## Summary

non-`.sync` `vote.{all,any,uni,ballot}` patterns lack an upper-arch predicate (sibling non-sync `shfl` has `hasSHFL`); `vote.ballot.b32` is emitted on sm_70+/PTX≥6.4 where it was removed

## Mechanism / root cause

The non-sync vote instructions are defined under:

  // vote.{all,any,uni,ballot}
  let Predicates = [hasPTX<60>, hasSM<30>] in {
    multiclass VOTE<...> { def : BasicNVPTXInst<..., "vote." # mode # "." # t.PtxType, ...>; }
    defm VOTE_ALL : VOTE<"all", I1RT, int_nvvm_vote_all>;
    defm VOTE_ANY : VOTE<"any", I1RT, int_nvvm_vote_any>;
    defm VOTE_UNI : VOTE<"uni", I1RT, int_nvvm_vote_uni>;
    defm VOTE_BALLOT : VOTE<"ballot", I32RT, int_nvvm_vote_ballot>;
  }

The predicate is only a *lower* bound (PTX>=6.0, SM>=3.0); there is NO upper bound. Per the PTX ISA, support for shfl and vote WITHOUT the .sync qualifier was deprecated in PTX ISA 6.0 and REMOVED for .target sm_70 and higher starting at PTX ISA version 6.4. ptxas emits a hard error: "Instruction 'vote' without '.sync' is not supported on .target sm_70 and higher from PTX ISA version 6.4". The backend already knows this: the sibling non-sync shfl patterns (NVPTXInstrInfo.td:226) use Requires<...[hasSM<30>, hasSHFL]>, where hasSHFL = Predicate<"!(Subtarget->getSmVersion() >= 70 && Subtarget->getPTXVersion() >= 64)"> (NVPTXInstrInfo.td:166-168, with the explicit comment "non-sync shfl instructions are not available on sm_70+ in PTX6.4+"). The non-sync vote block was given no equivalent guard, so at sm_70+ with PTX>=6.4 (which is the *default* emitted PTX version for sm_80/sm_90 etc.) llc happily emits vote.ballot.b32 / vote.all.pred / vote.any.pred / vote.uni.pred, which ptxas rejects. Contrast with shfl, where the same situation correctly yields 'Cannot select' rather than bad PTX. Note vote.sync (lines 249-261) is unaffected and correct.

## Trigger

Call the public NVVM intrinsic llvm.nvvm.vote.ballot (or .vote.all/.any/.uni) and compile for sm_70 or higher, where the default emitted .version is >= 6.4 (sm_80 -> 7.x, sm_90 -> 7.8). The emitted vote.ballot.b32 / vote.*.pred is removed for that target and ptxas errors out.

## Reproducer

```
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
```

Command: `llc -mtriple=nvptx64 -mcpu=sm_90 -o - repro.ll`

## Verification

Reproduced with the built llc (emitted PTX / crash matches the claim; finder confidence 0.9, confirmed_with_llc=True).
