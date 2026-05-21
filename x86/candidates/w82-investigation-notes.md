# worker-82 investigation notes (2026-05-21)

No confirmed reproducible miscompiles in ~12 minute window for
`llvm/lib/Target/X86/X86InstCombineIntrinsic.cpp`. Patterns investigated and
verified correct (rule-outs to spare future workers re-deriving):

## 1. simplifyTernarylogic (lines 669-1734)

Wrote a Python verifier that parses each of the 256 `case 0xNN:` entries and
evaluates the expression against the canonical A=0xf0, B=0xcc, C=0xaa truth-
table constants. ALL entries verify (script: /tmp/w82/verify_ternlog.py). The
in-source assertion at line 1732 already enforces this at runtime; the table is
correct by construction.

## 2. simplifyX86vpermilvar (lines 2068-2113)

For PS: keeps bits [1:0] of each i32 mask element. For PD: shifts right by 1
(extracting bit 1 of each i64 element). The +lane_offset adjustment yields
correct global shuffle indices. Tested all-bits-set mask (yields max index per
lane), all-zero mask (yields lane base), and "bit 1 vs bit 0" PD variants. All
results match hardware semantics. The SimplifyDemandedBits calls at 3098 (mask
0b00011 for PS) and 3111 (mask 0b00010 for PD) are also correct.

## 3. simplifyX86pshufb (lines 2024-2065) and demand-bits mask 0b10001111

Per-lane low-nibble index + sign-bit-zero behavior matches hardware. Demand
mask 0x8F (bits 0,1,2,3,7) ignores the always-ignored bits 4,5,6. Tested with
0x70 (mid-bits set, no sign) → select index 0; 0xF0 (sign + mid-bits) → zero.

## 4. simplifyX86varShift (lines 297-431) - psllv/psrlv/psrav with OOR/undef

Tested constant shift vectors with all-OOR, all-undef, and mixed OOR + in-range
for arithmetic and logical variants. Arithmetic shifts clamp OOR amounts to
BitWidth-1 (correct sign-bit splat). Logical shifts return zero or bail on
mixed cases (correct). The lambda `OutOfRange = [Idx<0 || BitWidth<=Idx]` is
correct for both branches given how arithmetic clamps to BitWidth-1.

## 5. simplifyX86immShift constant-vector path (lines 247-291)

For PSLLW, the low 64 bits of the shift vector are read as a 64-bit count. The
code correctly concatenates elements [0..3] for i16 lanes, [0..1] for i32 lanes,
and uses [0] alone for i64 lanes. Verified that Count.uge(BitWidth) handles all
out-of-range cases (returns 0 for logical, AShr by BitWidth-1 for arithmetic).

## 6. simplifyX86pmadd (lines 557-609) PMADDWD and PMADDUBSW

PMADDWD signed-overflow case (e.g., 32768 inputs giving sum = 2^31) wraps in
i32, matching hardware. PMADDUBSW uses sadd_sat for saturating addition,
matching hardware. Undef-element propagation (whole-vector undef → zero per
policy) matches code comment.

## 7. simplifyX86pmulh (lines 499-555) PMULH, PMULHU, PMULHRSW

The `LShr(Mul, 14)` + `Trunc i18` trick for PMULHRSW correctly preserves sign
behavior via integer wraparound — verified manually for several negative inputs
(-1, -16384, -16385) giving the same low-16 result as hardware arithmetic
shift. The m_One signed/unsigned paths produce correct AShr by 15 / zero.
m_One does NOT match `<1, ..., undef, undef>` mixed vectors because undef ≠
poison and getSplatValue(false) requires exact match — verified.

## 8. simplifyX86FPMaxMin (lines 1737-1783)

The Forbidden0/Forbidden1 with NaN|Inf|Subnormal (+NegZero on Arg1 for max,
Arg0 for min) correctly handles all x86_max/min vs LLVM maxnum/minnum
differences. Subnormal forbidden is needed for DAZ-input case (subnormal → 0
on input flushes the comparison). Confirmed equivalence in each case.

## 9. simplifyX86insertps (lines 1785-1840) - INSERTPS with ZMask & arg0==arg1

The "shuffle with zero vector" path correctly handles both arg0==arg1 (where
arg0[SourceLane] == arg1[SourceLane]) and the case where ZMask zeros out the
destination lane (in which case the inserted value is immediately overridden
to 0 in the ZMask loop). Verified by tracing through all branch combinations.

## 10. simplifyX86pack (lines 433-497) PACKSS/PACKUS

Saturation logic: PACKSS uses signed clamp [SIntMin, SIntMax] of dst type.
PACKUS uses [0, UIntMax of dst type]. Both use signed comparisons for input
clamping (matching hardware semantics where negative→0 for unsigned and
input>maxint→maxint). Per-lane pack mask: PackMask shuffles in
(X[lo..hi], Y[lo..hi]) per 128-bit lane, then truncates. Correct.

## 11. simplifyX86VPERMMask demand-bits (lines 2186-2199)

IdxSizeInBits = Log2_32(IsBinary ? 2*NumElts : NumElts). For permvar_qi_512
(NumElts=64): bottom 6 bits demanded. For vpermi2var_qi_512 (binary, NumElts=
64): bottom 7 bits demanded. Cross-checked against simplifyX86vpermv masking
(`Index &= Size - 1`) and simplifyX86vpermv3 (`Index &= 2*Size - 1`).
Consistent with hardware.

## 12. PCLMULQDQ demand-elt (lines 2765-2807)

DemandedElts1 = getSplat(VWidth, APInt(2, bit_for_op1)) where bit_for_op1 is
01 (low qword) or 10 (high qword). Splatting a 2-bit value across VWidth
yields the per-128-bit-lane pattern. For 256-bit: 0101 or 1010. For 512-bit:
01010101 or 10101010. Correct (per-lane qword selection).

## 13. simplifyX86movmsk (lines 611-640)

For 4-doubles → i32 result: NumElts=4, IntegerTy=i4, bitcast through <4 x i64>,
isneg, bitcast to i4, zext to i32. Correct (bits [3:0] of i32 hold sign bits).
Similar for all sizes (16, 32 bytes; 4 floats, 8 floats; 2 doubles, 4 doubles).

## 14. BMI BEXTR/BZHI/PEXT/PDEP folds (lines 2212-2349)

- BEXTR: Length=0 or Shift>=BitWidth → zero. Constant fold uses
  `(Src >> Shift) & maskTrailingOnes(min(Length, BitWidth))`. Correct.
- BZHI: Index>=BitWidth → Arg0 pass-through (matches Intel SDM: "if index >=
  operand size, DEST = SRC"). Index=0 → zero. Constant fold correct.
- PEXT shifted-mask: (Input & mask) >> MaskIdx is correct expression for
  shifted-mask layout.
- PDEP shifted-mask: (Input << MaskIdx) & mask correctly deposits Input's low
  bits at mask positions.

## 15. SSE4A EXTRQ/EXTRQI/INSERTQ/INSERTQI (lines 1842-2021)

Byte-aligned (Length%8==0 && Index%8==0) → byte shuffle. Constant-fold of
bit field extraction/insertion using APInt arithmetic. Index+Length>64 →
undef (matches AMD spec "results undefined"). Length=0 → Length=64 (matches
AMD spec "field length 0 means 64"). All correct.

## Summary

12 minutes of careful analysis covered the major fold paths in the file. No
reproducible miscompile was found. The simplifications I examined are all
correct under careful semantic analysis. The Python ternlog verifier confirms
the 256-entry table is structurally sound.

Areas where I could not find issues but also could not exhaustively cover:
- Per-element undef propagation in PMULH (m_One mixed-undef does not match,
  so that path is safe).
- KnownBits-based folding for variable shifts when shift amounts come from
  complex computations (would require runtime testing).
- AVX-512 mask-register operations are handled in target-independent IR
  (plain `and i16` etc.), not in this file.

## Confidence

No new candidates submitted.
