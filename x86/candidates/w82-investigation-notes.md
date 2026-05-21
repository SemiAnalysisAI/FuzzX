# worker-82 investigation notes (2026-05-21)

No confirmed reproducible miscompiles in ~12 minute window for
`llvm/lib/Target/X86/X86InstCombineIntrinsic.cpp` (3540 lines). 

## Approach
Wrote python verifier `/tmp/w82/verify_ternlog.py` parsing the 256-entry
`simplifyTernarylogic` table and evaluating each expression against the
canonical A=0xf0/B=0xcc/C=0xaa truth-table constants. ALL 256 entries verify.
Also ran differential opt+llc tests against ~30 IR samples covering edge cases.

## Patterns investigated (all verified correct - rule-outs)

1. **simplifyTernarylogic** (669-1734): 256-entry table verified correct via
   Python evaluator. In-source assertion at line 1732 already enforces.

2. **simplifyX86vpermilvar** (2068-2113): PS keeps bits[1:0], PD shifts right
   by 1 (extracting bit 1). Lane offset adjustment correct. SimplifyDemandedBits
   masks 0b00011 (PS) / 0b00010 (PD) correctly match hardware.

3. **simplifyX86pshufb** (2024-2065): Tested 0x70 (mid-bits, no sign) → src[0];
   0xF0 (sign+mid) → zero; 0x83 (sign+low nibble) → zero. All match hardware.
   Demand mask 0x8F (bits 0,1,2,3,7) correctly ignores ignored bits 4-6.

4. **simplifyX86varShift** (297-431): Tested all-OOR (logical=0, arith=sign-
   splat), all-undef (return undef vec), mixed (logical bails, arith clamps).
   OutOfRange lambda correct for both branches given how arith clamps to
   BitWidth-1.

5. **simplifyX86immShift constant-vec path** (247-291): Concatenates low 64
   bits across elements correctly. PSLLW: 4 elts of i16. PSLLD: 2 elts of i32.
   PSLLQ: 1 elt of i64. uge(BitWidth) handles OOR correctly.

6. **simplifyX86pmadd** (557-609): PMADDWD with overflow (-32768 * -32768
   doubled = 0x80000000) matches hardware (wraps in i32 add). PMADDUBSW uses
   sadd_sat correctly.

7. **simplifyX86pmulh** (499-555): PMULHRSW's `LShr(Mul, 14)` + `Trunc i18`
   trick preserves sign behavior via wraparound. Manually verified for Mul=-1,
   -16384, -16385. m_One mixed-undef does NOT match (getSplatValue(false)
   requires exact match; undef != poison).

8. **simplifyX86FPMaxMin** (1737-1783): Forbidden0/Forbidden1 with NaN|Inf|
   Subnormal +NegZero on Arg1 (max) / Arg0 (min) correctly handles all x86 vs
   IEEE differences including DAZ subnormal flushing.

9. **simplifyX86insertps** (1785-1840): Verified both arg0==arg1 and
   ZMask-overrides-DestLane paths. ZMask loop correctly overrides the
   ShuffleMask[DestLane] assignment when bit is set.

10. **simplifyX86pack** (433-497): Signed/unsigned saturation matches hardware
    semantics. Cross-lane pack mask correct.

11. **simplifyX86VPERMMask** (2186-2199): IdxSizeInBits correct for unary
    (Log2_32(NumElts)) and binary (Log2_32(2*NumElts)).

12. **PCLMULQDQ demand-elt** (2765-2807): getSplat(VWidth, APInt(2, ...))
    correctly produces per-128-bit-lane qword pattern.

13. **simplifyX86movmsk** (611-640): Correct for all sizes. ZExtOrTrunc from
    iN (where N=NumElts) to result type.

14. **BMI BEXTR/BZHI/PEXT/PDEP** (2212-2349): All paths correct. BZHI
    Index>=BitWidth returns Arg0 (matches SDM "DEST=SRC"). PEXT shifted-mask
    = (Input&mask)>>MaskIdx. PDEP shifted-mask = (Input<<MaskIdx)&mask.

15. **SSE4A EXTRQ/EXTRQI/INSERTQ/INSERTQI** (1842-2021): Byte-aligned shuffle
    path, bit-precise constant fold, INSERTQ→INSERTQI conversion, undef high
    half setting all correct.

## Tested intrinsic invocations (snapshot)
- vpermilvar.ps/pd with all-bits-set, all-zero, bit-flip
- pshufb with 0x70/0xF0/0x83/0x05 masks (sign bit + ignored bits)
- psllv/psrlv/psrav with OOR/in-range/undef mixes
- pslli/psrli/psrai with shift=BitWidth, BitWidth+1, large
- pmadd.wd with overflow cases (32768*32768*2 = wrap)
- pmaddubsw with sadd_sat saturation extremes
- pmulh/pmulhu with splat(1), mixed-undef (correctly NOT folded)
- BMI bextr Length=0/Shift=OOR
- BMI bzhi Index=0/Index>=BW
- SSE4A insertqi byte-aligned, OOR

## Conclusion
This file appears to be well-tested. The constant-fold paths I examined all
match hardware semantics under careful analysis. The Python ternlog verifier
confirms the largest table is structurally sound. No new candidates submitted.
