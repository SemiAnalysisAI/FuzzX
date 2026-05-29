# 258 — X86 `copyPhysReg` emits BWI-only `KMOVQkk_EVEX` for VK16 copies on `+avx512f,+egpr` (no BWI)

Component: `llvm/lib/Target/X86/X86InstrInfo.cpp` `X86InstrInfo::copyPhysReg`, VK16
mask-copy branch (~line 4367).

For a `$k -> $k` (VK16) physical-register COPY, the EGPR (APX) arm with BWI
off selects `KMOVQkk_EVEX`:
```cpp
Opc = HasEGPR ? X86::KMOVQkk_EVEX : X86::KMOVQkk;   // <-- wrong: Q form
```
`KMOVQkk_EVEX` is gated on `Predicates = [HasBWI, HasEGPR, In64BitMode]`
(X86InstrAVX512.td), but this subtarget has EGPR without BWI. The sibling
GPR<->mask arms (KMOVWrk_EVEX / KMOVWkr_EVEX at ~4239/4254) correctly use the
**W** form for the no-BWI+EGPR case; line 4367 should be `KMOVWkk_EVEX`.

## Result (HEAD 023e7decf625)
`-mattr=+avx512f,+egpr` (BWI off):
```
$k2 = KMOVQkk_EVEX killed $k1     # -> "kmovq %k1, %k2"
```
A BWI-only instruction is silently emitted for a legal `-mattr` config (EGPR/APX
is independent of BWI). `-filetype=obj` succeeds, so it is encoded into the
binary; on a CPU without AVX512BW it would `#UD`. Controls: `+avx512f` (no egpr)
→ `kmovw` (correct); `+avx512f,+bwi,+egpr` → `kmovq` (legitimate).

The COPY between physical k-regs is inserted by the register allocator and
lowered by copyPhysReg (not hand-crafted MIR); the trigger here pins mask values
to specific k-registers via inline asm (standard AVX-512 usage).

## Severity
Wrong / target-illegal instruction selection. Latent: a functional fault needs
APX + AVX512F-without-BWI hardware (not expected to ship), but the codegen defect
is unambiguous and reproduces at HEAD. Clear one-line typo fix.
