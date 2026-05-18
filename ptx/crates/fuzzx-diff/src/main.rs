//! Multi-worker differential fuzzer for ptxas.
//!
//! For each iteration:
//!   1. Pull the next seed from a shared atomic counter.
//!   2. Derive a byte buffer from the seed.
//!   3. Generate a PTX kernel.
//!   4. Compile + launch at both `-O0` and `-O3` on this worker's GPU.
//!   5. If outputs differ (or compile/launch is asymmetric), save the
//!      reproducer under `<out_dir>/div-<timestamp>-<seed>/`.
//!
//! Configured via env vars:
//!   DIV_OUT_DIR           default: `divergences`
//!   DIV_STARTING_SEED     default: nanos since epoch
//!   DIV_MAX_ITERS         default: unlimited
//!   DIV_PRINT_EVERY_SECS  default: 5
//!   DIV_PROGRAM_BYTES     default: 4096
//!   DIV_STRUCTURED_CONTROL_FLOW default: false; set 1/true/yes/on for
//!                                single-entry structured if/loop generation
//!   DIV_DISABLE_STRUCTURED_LOOPS default: false; set 1/true/yes/on to suppress
//!                                counted-loop shapes in structured mode
//!   DIV_DISABLE_ARBITRARY_LOOPS default: false; set 1/true/yes/on to suppress
//!                                backedge loop terminators in arbitrary CFG mode
//!   DIV_DISABLE_LOP3      default: false; set 1/true/yes/on to suppress
//!                         explicit PTX lop3.b32 generation
//!   DIV_DISABLE_PREDICATED_LOP3 default: false; set 1/true/yes/on to suppress
//!                         predicated lop3.b32 generation
//!   DIV_DISABLE_MINMAX    default: false; set 1/true/yes/on to suppress
//!                         PTX min.u32/max.u32/min.s32/max.s32 generation
//!   DIV_DISABLE_SELP      default: false; set 1/true/yes/on to suppress
//!                         PTX selp.b32 generation
//!   DIV_DISABLE_SUB       default: false; set 1/true/yes/on to suppress
//!                         random ALU PTX sub.u32 generation
//!   DIV_DISABLE_MUL_LO    default: false; set 1/true/yes/on to suppress
//!                         PTX mul.lo.u32 and mad.lo.u32 generation
//!   DIV_DISABLE_SIGNED_LO_ALU default: false; set 1/true/yes/on to suppress
//!                         PTX signed low-ALU spellings, including saturating add/sub
//!   DIV_DISABLE_SAT_ARITH default: false; set 1/true/yes/on to suppress
//!                         PTX add.sat.s32/sub.sat.s32 generation
//!   DIV_DISABLE_PACKED_ADD default: false; set 1/true/yes/on to suppress
//!                         PTX add.{u16x2,s16x2} generation
//!   DIV_DISABLE_SIGNED_PACKED_ADD default: false; set 1/true/yes/on to suppress
//!                         PTX add.s16x2 generation while retaining add.u16x2
//!   DIV_DISABLE_PREDICATED_PACKED_ADD default: false; set 1/true/yes/on to suppress
//!                         predicated add.{u16x2,s16x2} generation
//!   DIV_DISABLE_PACKED_MINMAX default: false; set 1/true/yes/on to suppress
//!                         PTX min/max.{u16x2,s16x2} generation
//!   DIV_DISABLE_SIGNED_PACKED_MINMAX default: false; set 1/true/yes/on to suppress
//!                         PTX min/max.s16x2 generation while retaining u16x2
//!   DIV_DISABLE_PREDICATED_PACKED_MINMAX default: false; set 1/true/yes/on to suppress
//!                         predicated min/max.{u16x2,s16x2} generation
//!   DIV_DISABLE_SCALAR_16BIT default: false; set 1/true/yes/on to suppress
//!                         scalar 16-bit ALU through .b16 scratch registers
//!   DIV_DISABLE_SIGNED_SCALAR_16BIT default: false; set 1/true/yes/on to suppress
//!                         signed scalar 16-bit ALU while retaining u16 ops
//!   DIV_DISABLE_SCALAR_16BIT_MIN default: false; set 1/true/yes/on to suppress
//!                         min.{u16,s16} scalar 16-bit ALU while retaining max and arithmetic ops
//!   DIV_DISABLE_SCALAR_16BIT_SIGNED_UNARY default: false; set 1/true/yes/on to suppress
//!                         abs.s16/neg.s16 scalar 16-bit ALU while retaining other scalar 16-bit ops
//!   DIV_DISABLE_SCALAR_16BIT_BITWISE default: false; set 1/true/yes/on to suppress
//!                         and/or/xor/not.b16 scalar 16-bit ALU
//!   DIV_DISABLE_SCALAR_16BIT_SHIFTS default: false; set 1/true/yes/on to suppress
//!                         shl.b16, shr.u16, and shr.s16 scalar 16-bit ALU
//!   DIV_DISABLE_SCALAR_16BIT_COMPARE default: false; set 1/true/yes/on to suppress
//!                         setp/set with scalar 16-bit operands
//!   DIV_DISABLE_SCALAR_16BIT_SELP default: false; set 1/true/yes/on to suppress
//!                         selp.{u16,s16} through scalar 16-bit scratch registers
//!   DIV_DISABLE_PREDICATED_SCALAR_16BIT default: false; set 1/true/yes/on to suppress
//!                         predicated scalar 16-bit ALU generation
//!   DIV_DISABLE_MULHI     default: false; set 1/true/yes/on to suppress
//!                         PTX mul.hi.u32/mul.hi.s32 generation
//!   DIV_DISABLE_SIGNED_MULHI default: false; set 1/true/yes/on to suppress
//!                         PTX mul.hi.s32 generation while retaining mul.hi.u32
//!   DIV_DISABLE_MAD_HI    default: false; set 1/true/yes/on to suppress
//!                         PTX mad.hi.{u32,s32} generation
//!   DIV_DISABLE_SIGNED_MAD_HI default: false; set 1/true/yes/on to suppress
//!                         PTX mad.hi.s32 generation while retaining mad.hi.u32
//!   DIV_DISABLE_BITWISE_BINOPS default: false; set 1/true/yes/on to suppress
//!                         PTX and.b32/or.b32/xor.b32 generation
//!   DIV_DISABLE_OR        default: false; set 1/true/yes/on to suppress
//!                         PTX or.b32 generation while retaining and.b32/xor.b32
//!   DIV_DISABLE_XOR       default: false; set 1/true/yes/on to suppress
//!                         PTX xor.b32 generation while retaining and.b32/or.b32
//!   DIV_DISABLE_PRMT      default: false; set 1/true/yes/on to suppress
//!                         PTX prmt.b32 generation
//!   DIV_DISABLE_PREDICATED_PRMT default: false; set 1/true/yes/on to suppress
//!                         predicated prmt.b32 generation
//!   DIV_DISABLE_REG_PRMT default: false; set 1/true/yes/on to suppress
//!                         register-control prmt.b32 generation
//!   DIV_DISABLE_PREDICATED_REG_PRMT default: false; set 1/true/yes/on to suppress
//!                         predicated register-control prmt.b32 generation
//!   DIV_DISABLE_PRMT_MODES default: false; set 1/true/yes/on to suppress
//!                         prmt.b32 mode variants such as .f4e and .rc8
//!   DIV_DISABLE_NOT       default: false; set 1/true/yes/on to suppress
//!                         PTX not.b32 generation and xor.b32-by-0xffffffff
//!   DIV_DISABLE_CLZ       default: false; set 1/true/yes/on to suppress
//!                         PTX clz.b32 generation
//!   DIV_DISABLE_BREV      default: false; set 1/true/yes/on to suppress
//!                         PTX brev.b32 generation
//!   DIV_DISABLE_CNOT      default: false; set 1/true/yes/on to suppress
//!                         PTX cnot.b32 generation
//!   DIV_DISABLE_POPC      default: false; set 1/true/yes/on to suppress
//!                         PTX popc.b32 generation
//!   DIV_DISABLE_ABS       default: false; set 1/true/yes/on to suppress
//!                         PTX abs.s32 generation
//!   DIV_DISABLE_SPECIAL_REGS default: false; set 1/true/yes/on to suppress
//!                         deterministic PTX special-register reads
//!   DIV_DISABLE_PREDICATED_SPECIAL_REGS default: false; set 1/true/yes/on to suppress
//!                         predicated deterministic special-register reads
//!   DIV_DISABLE_GLOBAL_LOADS default: false; set 1/true/yes/on to suppress
//!                         bounded read-only global loads from the input buffer
//!   DIV_DISABLE_GLOBAL_STORE_ROUNDTRIPS default: false; set 1/true/yes/on to suppress
//!                         per-thread global-memory store/load roundtrips
//!   DIV_DISABLE_CONST_MEMORY default: false; set 1/true/yes/on to suppress
//!                         bounded read-only constant-memory loads
//!   DIV_DISABLE_LOCAL_MEMORY default: false; set 1/true/yes/on to suppress
//!                         local-memory store/load roundtrips
//!   DIV_DISABLE_SHARED_MEMORY default: false; set 1/true/yes/on to suppress
//!                         race-free per-thread shared-memory store/load roundtrips
//!   DIV_DISABLE_PREDICATED_MEMORY default: false; set 1/true/yes/on to suppress
//!                         predicated scalar/vector memory loads and store/load roundtrips
//!   DIV_DISABLE_VECTOR_MEMORY default: false; set 1/true/yes/on to suppress
//!                         vectorized u32/u64 memory loads and store/load roundtrips
//!   DIV_DISABLE_WIDE_MEMORY default: false; set 1/true/yes/on to suppress
//!                         scalar 64-bit and v2.u64 memory loads and store/load roundtrips
//!   DIV_DISABLE_MEMORY_CACHE_OPS default: false; set 1/true/yes/on to suppress
//!                         global-memory load/store cache-policy variants
//!   DIV_DISABLE_F32_ARITH default: false; set 1/true/yes/on to suppress
//!                         sanitized f32 add/sub/mul/div/fma/copysign/min/max,
//!                         f32 sat arithmetic, and ftz min/max generation
//!   DIV_DISABLE_F32_ROUNDING default: false; set 1/true/yes/on to suppress
//!                         non-default rounding-mode and ftz f32 add/sub/mul/div/fma generation
//!   DIV_DISABLE_F32_UNARY default: false; set 1/true/yes/on to suppress
//!                         f32 abs/neg generation, including ftz forms
//!   DIV_DISABLE_F32_CVT default: false; set 1/true/yes/on to suppress
//!                         explicit signed/unsigned 32/64-bit f32/int,
//!                         saturating f32-to-int, f64-to-f32, and ftz conversion chains
//!   DIV_DISABLE_F32_SPECIAL_MATH default: false; set 1/true/yes/on to suppress
//!                         f32 rounded and ftz sqrt/rcp plus approx rcp/rsqrt/ex2/lg2/sin/cos
//!                         generation
//!   DIV_DISABLE_F32_COMPARE default: false; set 1/true/yes/on to suppress
//!                         sanitized ordered/unordered f32 compare, including ftz forms,
//!                         and testp generation
//!   DIV_DISABLE_F32_SELP default: false; set 1/true/yes/on to suppress
//!                         sanitized setp.f32, including ftz forms, + selp.f32 generation
//!   DIV_DISABLE_F64_ARITH default: false; set 1/true/yes/on to suppress
//!                         sanitized f64 add/sub/mul/div/fma/copysign/min/max generation
//!   DIV_DISABLE_F64_ROUNDING default: false; set 1/true/yes/on to suppress
//!                         non-default rounding-mode f64 add/sub/mul/div/fma generation
//!   DIV_DISABLE_F64_UNARY default: false; set 1/true/yes/on to suppress
//!                         f64 abs/neg generation
//!   DIV_DISABLE_F64_CVT default: false; set 1/true/yes/on to suppress
//!                         explicit signed/unsigned 32/64-bit f64/int,
//!                         saturating f64-to-int, and f32-to-f64 conversion chains
//!   DIV_DISABLE_F64_SPECIAL_MATH default: false; set 1/true/yes/on to suppress
//!                         rounded f64 sqrt/rcp generation
//!   DIV_DISABLE_F64_COMPARE default: false; set 1/true/yes/on to suppress
//!                         sanitized ordered/unordered f64 compare and testp generation
//!   DIV_DISABLE_F64_SELP default: false; set 1/true/yes/on to suppress
//!                         sanitized setp.f64 + selp.f64 generation
//!   DIV_DISABLE_SIGNED_CMP default: false; set 1/true/yes/on to suppress
//!                         PTX setp.{lt,le,gt,ge}.s32 generation
//!   DIV_DISABLE_SIGNED_DIVREM default: false; set 1/true/yes/on to suppress
//!                         PTX div.s32/rem.s32 generation
//!   DIV_DISABLE_REG_DIVREM default: false; set 1/true/yes/on to suppress
//!                         register-divisor div.u32/rem.u32 generation
//!   DIV_DISABLE_PREDICATED_REG_DIVREM default: false; set 1/true/yes/on to suppress
//!                         predicated register-divisor div.u32/rem.u32 generation
//!   DIV_DISABLE_PREDICATED_DIVREM default: false; set 1/true/yes/on to suppress
//!                         predicated div/rem generation
//!   DIV_DISABLE_FUNNEL    default: false; set 1/true/yes/on to suppress
//!                         PTX shf.{l,r}.{wrap,clamp}.b32 generation
//!   DIV_DISABLE_REG_FUNNEL default: false; set 1/true/yes/on to suppress
//!                         register-count shf.{l,r}.{wrap,clamp}.b32 generation
//!   DIV_DISABLE_PREDICATED_FUNNEL default: false; set 1/true/yes/on to suppress
//!                         predicated shf.{l,r}.{wrap,clamp}.b32 generation
//!   DIV_DISABLE_FUNNEL_CLAMP default: false; set 1/true/yes/on to suppress
//!                         shf.{l,r}.clamp.b32 generation
//!   DIV_DISABLE_NEG       default: false; set 1/true/yes/on to suppress
//!                         PTX neg.s32 generation
//!   DIV_DISABLE_SHL       default: false; set 1/true/yes/on to suppress
//!                         PTX shl.b32 generation
//!   DIV_DISABLE_SHR       default: false; set 1/true/yes/on to suppress
//!                         PTX shr.u32 generation
//!   DIV_DISABLE_SIGNED_SHR default: false; set 1/true/yes/on to suppress
//!                         PTX shr.s32 generation
//!   DIV_DISABLE_REG_SHIFTS default: false; set 1/true/yes/on to suppress
//!                         masked register-count shift generation
//!   DIV_DISABLE_PREDICATED_SHIFTS default: false; set 1/true/yes/on to suppress
//!                         predicated immediate shift generation
//!   DIV_DISABLE_PREDICATED_REG_SHIFTS default: false; set 1/true/yes/on to suppress
//!                         predicated masked register-count shift generation
//!   DIV_DISABLE_BFIND     default: false; set 1/true/yes/on to suppress
//!                         PTX bfind generation
//!   DIV_DISABLE_SIGNED_BFIND default: false; set 1/true/yes/on to suppress
//!                         PTX bfind.s32/bfind.shiftamt.s32 generation
//!   DIV_DISABLE_WIDE_BFIND default: false; set 1/true/yes/on to suppress
//!                         PTX bfind.{u64,s64}/bfind.shiftamt.{u64,s64} generation
//!   DIV_DISABLE_SIGNED_WIDE_BFIND default: false; set 1/true/yes/on to suppress
//!                         PTX bfind.s64/bfind.shiftamt.s64 generation
//!   DIV_DISABLE_PREDICATED_BFIND default: false; set 1/true/yes/on to suppress
//!                         predicated bfind generation
//!   DIV_DISABLE_PREDICATED_WIDE_BFIND default: false; set 1/true/yes/on to suppress
//!                         predicated 64-bit bfind generation
//!   DIV_DISABLE_FNS      default: false; set 1/true/yes/on to suppress
//!                         PTX fns.b32 generation
//!   DIV_DISABLE_REG_FNS  default: false; set 1/true/yes/on to suppress
//!                         fns.b32 with a sanitized register base/offset
//!   DIV_DISABLE_PREDICATED_FNS default: false; set 1/true/yes/on to suppress
//!                         predicated fns.b32 generation
//!   DIV_DISABLE_PREDICATED_REG_FNS default: false; set 1/true/yes/on to suppress
//!                         predicated fns.b32 with a sanitized register base/offset
//!   DIV_DISABLE_BFI       default: false; set 1/true/yes/on to suppress
//!                         PTX bfi.b32 generation
//!   DIV_DISABLE_BFE       default: false; set 1/true/yes/on to suppress
//!                         PTX bfe.{u32,s32} generation
//!   DIV_DISABLE_BMSK      default: false; set 1/true/yes/on to suppress
//!                         PTX bmsk.{clamp,wrap}.b32 generation
//!   DIV_DISABLE_BMSK_WRAP default: false; set 1/true/yes/on to suppress
//!                         PTX bmsk.wrap.b32 generation
//!   DIV_DISABLE_PREDICATED_BITFIELD default: false; set 1/true/yes/on to suppress
//!                         predicated bfe.{u32,s32}/bfi.b32/bmsk.b32 generation
//!   DIV_DISABLE_REG_BITFIELD default: false; set 1/true/yes/on to suppress
//!                         register pos/len operands for bfe/bfi/bmsk generation
//!   DIV_DISABLE_PREDICATED_REG_BITFIELD default: false; set 1/true/yes/on to suppress
//!                         predicated bfe/bfi/bmsk instructions with register pos/len operands
//!   DIV_DISABLE_WIDE_BFE default: false; set 1/true/yes/on to suppress
//!                         PTX bfe.{u64,s64} scratch-register generation
//!   DIV_DISABLE_SIGNED_WIDE_BFE default: false; set 1/true/yes/on to suppress
//!                         PTX bfe.s64 scratch-register generation
//!   DIV_DISABLE_WIDE_BFI default: false; set 1/true/yes/on to suppress
//!                         PTX bfi.b64 scratch-register generation
//!   DIV_DISABLE_PREDICATED_WIDE_BITFIELD default: false; set 1/true/yes/on to suppress
//!                         predicated 64-bit bfe/bfi generation
//!   DIV_DISABLE_REG_WIDE_BITFIELD default: false; set 1/true/yes/on to suppress
//!                         sanitized register pos/len operands for 64-bit bfe/bfi generation
//!   DIV_DISABLE_PREDICATED_REG_WIDE_BITFIELD default: false; set 1/true/yes/on to suppress
//!                         predicated 64-bit bfe/bfi instructions with register pos/len operands
//!   DIV_DISABLE_MAD24     default: false; set 1/true/yes/on to suppress
//!                         PTX mad24.lo.u32/mad24.hi.u32 generation
//!   DIV_DISABLE_MUL24     default: false; set 1/true/yes/on to suppress
//!                         PTX mul24.{lo,hi}.{u32,s32} generation
//!   DIV_DISABLE_PREDICATED_24BIT default: false; set 1/true/yes/on to suppress
//!                         predicated mad24/mul24 generation
//!   DIV_DISABLE_SUBWORD_WIDE default: false; set 1/true/yes/on to suppress
//!                         16-bit-source mul.wide/mad.wide generation
//!   DIV_DISABLE_SIGNED_SUBWORD_WIDE default: false; set 1/true/yes/on to suppress
//!                         signed 16-bit-source mul.wide/mad.wide generation
//!   DIV_DISABLE_PREDICATED_SUBWORD_WIDE default: false; set 1/true/yes/on to suppress
//!                         predicated 16-bit-source mul.wide/mad.wide generation
//!   DIV_DISABLE_MUL_WIDE  default: false; set 1/true/yes/on to suppress
//!                         PTX mul.wide.{u32,s32} generation
//!   DIV_DISABLE_PREDICATED_MUL_WIDE default: false; set 1/true/yes/on to suppress
//!                         predicated mul.wide.{u32,s32} generation
//!   DIV_DISABLE_MAD_WIDE  default: false; set 1/true/yes/on to suppress
//!                         PTX mad.wide.{u32,s32} generation
//!   DIV_DISABLE_SIGNED_MAD_WIDE default: false; set 1/true/yes/on to suppress
//!                         PTX mad.wide.s32 generation
//!   DIV_DISABLE_PREDICATED_MAD_WIDE default: false; set 1/true/yes/on to suppress
//!                         predicated mad.wide.{u32,s32} generation
//!   DIV_DISABLE_WIDE_HIGH_RESULT default: false; set 1/true/yes/on to suppress
//!                         high-half extraction from mul.wide/mad.wide results
//!   DIV_DISABLE_WIDE_INT  default: false; set 1/true/yes/on to suppress
//!                         PTX 64-bit ALU scratch-register generation
//!   DIV_DISABLE_WIDE_MINMAX default: false; set 1/true/yes/on to suppress
//!                         PTX min/max.{u64,s64} scratch-register generation
//!   DIV_DISABLE_WIDE_MULHI default: false; set 1/true/yes/on to suppress
//!                         PTX mul.hi.{u64,s64} scratch-register generation
//!   DIV_DISABLE_PREDICATED_WIDE_INT default: false; set 1/true/yes/on to suppress
//!                         predicated 64-bit ALU scratch-register generation
//!   DIV_DISABLE_WIDE_MAD64 default: false; set 1/true/yes/on to suppress
//!                         64-bit operand mad.{lo,hi}.{u64,s64} generation
//!   DIV_DISABLE_SIGNED_WIDE_MAD64 default: false; set 1/true/yes/on to suppress
//!                         64-bit operand mad.{lo,hi}.s64 generation
//!   DIV_DISABLE_PREDICATED_WIDE_MAD64 default: false; set 1/true/yes/on to suppress
//!                         predicated 64-bit operand mad generation
//!   DIV_DISABLE_WIDE_SET default: false; set 1/true/yes/on to suppress
//!                         64-bit set.{cmp}.u32.{u64,s64} materialization
//!   DIV_DISABLE_PREDICATED_WIDE_SET default: false; set 1/true/yes/on to suppress
//!                         predicated 64-bit set materialization
//!   DIV_DISABLE_WIDE_SETP default: false; set 1/true/yes/on to suppress
//!                         64-bit setp-fed guarded ALU generation
//!   DIV_DISABLE_WIDE_SETP_BOOL default: false; set 1/true/yes/on to suppress
//!                         64-bit setp.<cmp>.<and|or|xor> guarded ALU generation
//!   DIV_DISABLE_WIDE_SELP default: false; set 1/true/yes/on to suppress
//!                         64-bit scratch-register selp.b64 generation
//!   DIV_DISABLE_WIDE_UNARY default: false; set 1/true/yes/on to suppress
//!                         PTX not/cnot/popc/clz/brev.b64 generation
//!   DIV_DISABLE_PREDICATED_WIDE_UNARY default: false; set 1/true/yes/on to suppress
//!                         predicated wide unary generation
//!   DIV_DISABLE_WIDE_SHIFTS default: false; set 1/true/yes/on to suppress
//!                         PTX 64-bit scratch-register shift generation
//!   DIV_DISABLE_WIDE_REG_SHIFTS default: false; set 1/true/yes/on to suppress
//!                         masked register-count 64-bit shift generation
//!   DIV_DISABLE_PREDICATED_WIDE_SHIFTS default: false; set 1/true/yes/on to suppress
//!                         predicated 64-bit scratch-register shift generation
//!   DIV_DISABLE_PREDICATED_WIDE_REG_SHIFTS default: false; set 1/true/yes/on to suppress
//!                         predicated masked register-count 64-bit shift generation
//!   DIV_DISABLE_WIDE_DIVREM default: false; set 1/true/yes/on to suppress
//!                         PTX div/rem.{u64,s64} scratch-register generation
//!   DIV_DISABLE_SIGNED_WIDE_DIVREM default: false; set 1/true/yes/on to suppress
//!                         PTX div/rem.s64 scratch-register generation
//!   DIV_DISABLE_REG_WIDE_DIVREM default: false; set 1/true/yes/on to suppress
//!                         register-divisor div/rem.{u64,s64} scratch-register generation
//!   DIV_DISABLE_PREDICATED_REG_WIDE_DIVREM default: false; set 1/true/yes/on to suppress
//!                         predicated register-divisor 64-bit div/rem generation
//!   DIV_DISABLE_PREDICATED_WIDE_DIVREM default: false; set 1/true/yes/on to suppress
//!                         predicated 64-bit div/rem generation
//!   DIV_DISABLE_WIDE_ADDC default: false; set 1/true/yes/on to suppress
//!                         64-bit add.cc.u64/addc.u64 carry pair generation
//!   DIV_DISABLE_WIDE_SUBC default: false; set 1/true/yes/on to suppress
//!                         64-bit sub.cc.u64/subc.u64 carry pair generation
//!   DIV_DISABLE_PREDICATED_WIDE_CARRY default: false; set 1/true/yes/on to suppress
//!                         predicated 64-bit add/sub carry pair generation
//!   DIV_DISABLE_WIDE_CARRY_CHAIN default: false; set 1/true/yes/on to suppress
//!                         three-instruction 64-bit add/sub carry chain generation
//!   DIV_DISABLE_PREDICATED_WIDE_CARRY_CHAIN default: false; set 1/true/yes/on to suppress
//!                         predicated three-instruction 64-bit add/sub carry chain generation
//!   DIV_DISABLE_ADDC      default: false; set 1/true/yes/on to suppress
//!                         PTX add.cc.u32/addc.u32 pair generation
//!   DIV_DISABLE_SUBC      default: false; set 1/true/yes/on to suppress
//!                         PTX sub.cc.u32/subc.u32 pair generation
//!   DIV_DISABLE_PREDICATED_CARRY default: false; set 1/true/yes/on to suppress
//!                         predicated add/sub carry pair generation
//!   DIV_DISABLE_CARRY_CHAIN default: false; set 1/true/yes/on to suppress
//!                         three-instruction add/sub carry chain generation
//!   DIV_DISABLE_PREDICATED_CARRY_CHAIN default: false; set 1/true/yes/on to suppress
//!                         predicated three-instruction add/sub carry chain generation
//!   DIV_DISABLE_I32_BOUNDARY_IMMS default: false; set 1/true/yes/on to
//!                         suppress immediate 0x7fffffff/0x80000000 generation
//!   DIV_DISABLE_DP4A      default: false; set 1/true/yes/on to suppress
//!                         PTX dp4a generation
//!   DIV_DISABLE_DP2A      default: false; set 1/true/yes/on to suppress
//!                         PTX dp2a generation
//!   DIV_DISABLE_NEGATED_PREDICATES default: false; set 1/true/yes/on to suppress
//!                         @!%p instruction predicates
//!   DIV_DISABLE_PREDICATED_ALU default: false; set 1/true/yes/on to suppress
//!                         predicated integer ALU and floating-point arithmetic generation
//!   DIV_DISABLE_PREDICATED_UNARY default: false; set 1/true/yes/on to suppress
//!                         predicated integer unary, floating-point unary, and floating-point
//!                         special-math generation
//!   DIV_DISABLE_CVT default: false; set 1/true/yes/on to suppress
//!                         base cvt generation
//!   DIV_DISABLE_PREDICATED_CVT default: false; set 1/true/yes/on to suppress
//!                         predicated integer and floating-point cvt generation
//!   DIV_DISABLE_NARROW_CVT default: false; set 1/true/yes/on to suppress
//!                         narrow cvt round-trip generation
//!   DIV_DISABLE_SIGNED_NARROW_CVT default: false; set 1/true/yes/on to suppress
//!                         signed narrow cvt round-trip generation
//!   DIV_DISABLE_PREDICATED_NARROW_CVT default: false; set 1/true/yes/on to suppress
//!                         predicated narrow cvt round-trip generation
//!   DIV_DISABLE_WIDE_CVT default: false; set 1/true/yes/on to suppress
//!                         64-bit-source cvt round-trip generation
//!   DIV_DISABLE_SIGNED_WIDE_CVT default: false; set 1/true/yes/on to suppress
//!                         signed 64-bit-source cvt round-trip generation
//!   DIV_DISABLE_PREDICATED_WIDE_CVT default: false; set 1/true/yes/on to suppress
//!                         predicated 64-bit-source cvt round-trip generation
//!   DIV_DISABLE_SZEXT    default: false; set 1/true/yes/on to suppress
//!                         PTX szext.{wrap,clamp}.{u32,s32} generation
//!   DIV_DISABLE_SIGNED_SZEXT default: false; set 1/true/yes/on to suppress
//!                         PTX szext.*.s32 generation while retaining szext.*.u32
//!   DIV_DISABLE_PREDICATED_SZEXT default: false; set 1/true/yes/on to suppress
//!                         predicated szext generation
//!   DIV_DISABLE_SETP_BOOL default: false; set 1/true/yes/on to suppress
//!                         integer/floating setp.<cmp>.{and,or,xor} predicate-combiner generation
//!   DIV_DISABLE_SETP_DUAL default: false; set 1/true/yes/on to suppress
//!                         setp.<cmp> %p|%q complement-predicate generation
//!   DIV_DISABLE_PRED_LOGIC default: false; set 1/true/yes/on to suppress
//!                         and/or/xor/not.pred generation
//!   DIV_DISABLE_PREDICATED_MAD default: false; set 1/true/yes/on to suppress
//!                         predicated mad.lo.{u32,s32} generation
//!   DIV_DISABLE_PREDICATED_MAD_HI default: false; set 1/true/yes/on to suppress
//!                         predicated mad.hi.{u32,s32} generation
//!   DIV_DISABLE_MAD_CARRY default: false; set 1/true/yes/on to suppress
//!                         mad.cc/madc.cc/madc carry-chain generation
//!   DIV_DISABLE_SIGNED_MAD_CARRY default: false; set 1/true/yes/on to suppress
//!                         signed mad.cc/madc.cc/madc carry-chain generation
//!   DIV_DISABLE_PREDICATED_MAD_CARRY default: false; set 1/true/yes/on to suppress
//!                         predicated mad.cc/madc.cc/madc carry-chain generation
//!   DIV_DISABLE_PREDICATED_SET default: false; set 1/true/yes/on to suppress
//!                         predicated integer and floating-point set/setp/testp generation
//!   DIV_DISABLE_PREDICATED_SELP default: false; set 1/true/yes/on to suppress
//!                         instruction-predicated integer and floating-point selp generation
//!   DIV_DISABLE_SAD default: false; set 1/true/yes/on to suppress
//!                         sad.{u32,s32} generation
//!   DIV_DISABLE_SLCT default: false; set 1/true/yes/on to suppress
//!                         slct generation
//!   DIV_DISABLE_PREDICATED_SAD default: false; set 1/true/yes/on to suppress
//!                         predicated sad.{u32,s32} generation
//!   DIV_DISABLE_PREDICATED_SLCT default: false; set 1/true/yes/on to suppress
//!                         predicated slct generation
//!   DIV_DISABLE_PREDICATED_DP default: false; set 1/true/yes/on to suppress
//!                         predicated dp4a/dp2a generation
//!   DIV_DISABLE_PREDICATED_VIDEO default: false; set 1/true/yes/on to suppress
//!                         predicated video instruction generation
//!   DIV_DISABLE_SET       default: false; set 1/true/yes/on to suppress
//!                         PTX set.{cmp}.u32.{u32,s32} generation
//!   DIV_DISABLE_S32_SLCT  default: false; set 1/true/yes/on to suppress
//!                         PTX slct.s32.s32 generation
//!   DIV_DISABLE_VIDEO     default: false; set 1/true/yes/on to suppress
//!                         PTX video instruction generation
//!   DIV_DISABLE_VSUB4     default: false; set 1/true/yes/on to suppress
//!                         PTX vsub4.u32.u32.u32 generation
//!   DIV_MIN_BLOCKS        default: generator default
//!   DIV_MAX_BLOCKS        default: generator default
//!   DIV_MIN_INSTS_PER_BLOCK default: generator default
//!   DIV_MAX_INSTS_PER_BLOCK default: generator default
//!   DIV_WORKING_REGS      default: generator default
//!   DIV_MAX_LOOP_ITERS    default: generator default
//!   DIV_MAX_IMMEDIATE     default: generator default
//!   DIV_MAX_STRUCTURED_DEPTH default: generator default
//!   DIV_GPUS              default: all visible devices (e.g. "0,1,2")
//!   DIV_WORKERS_PER_GPU   default: 16  (B300 + 54-core sweep: knee at 48 total
//!                                       workers / 16 per GPU, ~2400 iter/s steady)
//!
//! Each worker thread owns its own CUDA context (pinned to one GPU) and a
//! single pair of pre-allocated input/output buffers — no per-iter
//! `cuMemAlloc`/`Free`. ptxas temp files go to `TMPDIR` (this binary sets
//! `TMPDIR=/dev/shm` by default if not already set).

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{Context as _, Result};
use clap::Parser;
use fuzzx_exec::{compile, Cuda, CudaBuffers, DiffOutcome};
use fuzzx_execgen::{
    bytes_from_seed, generate_from_bytes_with_config, input_for_seed, input_len, output_len,
    ControlFlowMode, GenConfig, KERNEL_NAME, N_THREADS, TARGET_ARCH,
};

#[derive(Debug, Parser)]
#[command(
    name = "fuzzx-diff",
    about = "Undirected differential fuzzer for NVIDIA ptxas"
)]
struct Args {
    #[arg(long, value_name = "PATH", help = "Target ptxas binary")]
    ptxas: Option<String>,
    #[arg(long, value_name = "DIR", help = "Temporary directory for ptxas files")]
    tmpdir: Option<String>,
    #[arg(long, value_name = "DIR", help = "Divergence output directory")]
    out_dir: Option<String>,
    #[arg(
        long,
        value_name = "N",
        help = "First seed in the deterministic stream"
    )]
    starting_seed: Option<u64>,
    #[arg(long, value_name = "N", help = "Stop after this many candidates")]
    max_iters: Option<u64>,
    #[arg(long, value_name = "SECS", help = "Progress-report interval")]
    print_every_secs: Option<u64>,
    #[arg(long, value_name = "N", help = "Generator input bytes per seed")]
    program_bytes: Option<usize>,
    #[arg(
        long,
        value_name = "LIST",
        help = "Comma-separated CUDA device ordinals"
    )]
    gpus: Option<String>,
    #[arg(long, value_name = "N", help = "Worker threads per selected GPU")]
    workers_per_gpu: Option<usize>,

    #[arg(long, help = "Use structured single-entry if/loop generation")]
    structured_control_flow: bool,
    #[arg(long, value_name = "N", help = "Minimum generated block count")]
    min_blocks: Option<usize>,
    #[arg(long, value_name = "N", help = "Maximum generated block count")]
    max_blocks: Option<usize>,
    #[arg(long, value_name = "N", help = "Minimum instructions per block")]
    min_insts_per_block: Option<usize>,
    #[arg(long, value_name = "N", help = "Maximum instructions per block")]
    max_insts_per_block: Option<usize>,
    #[arg(long, value_name = "N", help = "Number of working u32 registers")]
    working_regs: Option<u32>,
    #[arg(long, value_name = "N", help = "Maximum generated loop-trip count")]
    max_loop_iters: Option<u32>,
    #[arg(long, value_name = "N", help = "Maximum ordinary immediate")]
    max_immediate: Option<u32>,
    #[arg(
        long,
        value_name = "N",
        help = "Maximum structured-control nesting depth"
    )]
    max_structured_depth: Option<usize>,

    #[arg(long)]
    disable_structured_loops: bool,
    #[arg(long)]
    disable_arbitrary_loops: bool,
    #[arg(long)]
    disable_lop3: bool,
    #[arg(long)]
    disable_predicated_lop3: bool,
    #[arg(long)]
    disable_minmax: bool,
    #[arg(long)]
    disable_selp: bool,
    #[arg(long)]
    disable_sub: bool,
    #[arg(long)]
    disable_mul_lo: bool,
    #[arg(long)]
    disable_signed_lo_alu: bool,
    #[arg(long)]
    disable_sat_arith: bool,
    #[arg(long)]
    disable_packed_add: bool,
    #[arg(long)]
    disable_signed_packed_add: bool,
    #[arg(long)]
    disable_predicated_packed_add: bool,
    #[arg(long)]
    disable_packed_minmax: bool,
    #[arg(long)]
    disable_signed_packed_minmax: bool,
    #[arg(long)]
    disable_predicated_packed_minmax: bool,
    #[arg(long)]
    disable_scalar_16bit: bool,
    #[arg(long)]
    disable_signed_scalar_16bit: bool,
    #[arg(long)]
    disable_scalar_16bit_min: bool,
    #[arg(long)]
    disable_scalar_16bit_signed_unary: bool,
    #[arg(long)]
    disable_scalar_16bit_bitwise: bool,
    #[arg(long)]
    disable_scalar_16bit_shifts: bool,
    #[arg(long)]
    disable_scalar_16bit_compare: bool,
    #[arg(long)]
    disable_scalar_16bit_selp: bool,
    #[arg(long)]
    disable_predicated_scalar_16bit: bool,
    #[arg(long)]
    disable_mulhi: bool,
    #[arg(long)]
    disable_signed_mulhi: bool,
    #[arg(long)]
    disable_mad_hi: bool,
    #[arg(long)]
    disable_signed_mad_hi: bool,
    #[arg(long)]
    disable_bitwise_binops: bool,
    #[arg(long)]
    disable_or: bool,
    #[arg(long)]
    disable_xor: bool,
    #[arg(long)]
    disable_prmt: bool,
    #[arg(long)]
    disable_predicated_prmt: bool,
    #[arg(long)]
    disable_reg_prmt: bool,
    #[arg(long)]
    disable_predicated_reg_prmt: bool,
    #[arg(long)]
    disable_prmt_modes: bool,
    #[arg(long)]
    disable_not: bool,
    #[arg(long)]
    disable_clz: bool,
    #[arg(long)]
    disable_brev: bool,
    #[arg(long)]
    disable_cnot: bool,
    #[arg(long)]
    disable_popc: bool,
    #[arg(long)]
    disable_abs: bool,
    #[arg(long)]
    disable_special_regs: bool,
    #[arg(long)]
    disable_predicated_special_regs: bool,
    #[arg(long)]
    disable_global_loads: bool,
    #[arg(long)]
    disable_global_store_roundtrips: bool,
    #[arg(long)]
    disable_const_memory: bool,
    #[arg(long)]
    disable_local_memory: bool,
    #[arg(long)]
    disable_shared_memory: bool,
    #[arg(long)]
    disable_predicated_memory: bool,
    #[arg(long)]
    disable_vector_memory: bool,
    #[arg(long)]
    disable_wide_memory: bool,
    #[arg(long)]
    disable_memory_cache_ops: bool,
    #[arg(long)]
    disable_f32_arith: bool,
    #[arg(long)]
    disable_f32_rounding: bool,
    #[arg(long)]
    disable_f32_unary: bool,
    #[arg(long)]
    disable_f32_cvt: bool,
    #[arg(long)]
    disable_f32_special_math: bool,
    #[arg(long)]
    disable_f32_compare: bool,
    #[arg(long)]
    disable_f32_selp: bool,
    #[arg(long)]
    disable_f64_arith: bool,
    #[arg(long)]
    disable_f64_rounding: bool,
    #[arg(long)]
    disable_f64_unary: bool,
    #[arg(long)]
    disable_f64_cvt: bool,
    #[arg(long)]
    disable_f64_special_math: bool,
    #[arg(long)]
    disable_f64_compare: bool,
    #[arg(long)]
    disable_f64_selp: bool,
    #[arg(long)]
    disable_signed_cmp: bool,
    #[arg(long)]
    disable_signed_divrem: bool,
    #[arg(long)]
    disable_reg_divrem: bool,
    #[arg(long)]
    disable_predicated_reg_divrem: bool,
    #[arg(long)]
    disable_predicated_divrem: bool,
    #[arg(long)]
    disable_funnel: bool,
    #[arg(long)]
    disable_reg_funnel: bool,
    #[arg(long)]
    disable_predicated_funnel: bool,
    #[arg(long)]
    disable_funnel_clamp: bool,
    #[arg(long)]
    disable_neg: bool,
    #[arg(long)]
    disable_shl: bool,
    #[arg(long)]
    disable_shr: bool,
    #[arg(long)]
    disable_signed_shr: bool,
    #[arg(long)]
    disable_reg_shifts: bool,
    #[arg(long)]
    disable_predicated_shifts: bool,
    #[arg(long)]
    disable_predicated_reg_shifts: bool,
    #[arg(long)]
    disable_bfind: bool,
    #[arg(long)]
    disable_signed_bfind: bool,
    #[arg(long)]
    disable_wide_bfind: bool,
    #[arg(long)]
    disable_signed_wide_bfind: bool,
    #[arg(long)]
    disable_predicated_bfind: bool,
    #[arg(long)]
    disable_predicated_wide_bfind: bool,
    #[arg(long)]
    disable_fns: bool,
    #[arg(long)]
    disable_reg_fns: bool,
    #[arg(long)]
    disable_predicated_fns: bool,
    #[arg(long)]
    disable_predicated_reg_fns: bool,
    #[arg(long)]
    disable_bfi: bool,
    #[arg(long)]
    disable_bfe: bool,
    #[arg(long)]
    disable_bmsk: bool,
    #[arg(long)]
    disable_bmsk_wrap: bool,
    #[arg(long)]
    disable_predicated_bitfield: bool,
    #[arg(long)]
    disable_reg_bitfield: bool,
    #[arg(long)]
    disable_predicated_reg_bitfield: bool,
    #[arg(long)]
    disable_wide_bfe: bool,
    #[arg(long)]
    disable_signed_wide_bfe: bool,
    #[arg(long)]
    disable_wide_bfi: bool,
    #[arg(long)]
    disable_predicated_wide_bitfield: bool,
    #[arg(long)]
    disable_reg_wide_bitfield: bool,
    #[arg(long)]
    disable_predicated_reg_wide_bitfield: bool,
    #[arg(long)]
    disable_mad24: bool,
    #[arg(long)]
    disable_mul24: bool,
    #[arg(long)]
    disable_predicated_24bit: bool,
    #[arg(long)]
    disable_subword_wide: bool,
    #[arg(long)]
    disable_signed_subword_wide: bool,
    #[arg(long)]
    disable_predicated_subword_wide: bool,
    #[arg(long)]
    disable_mul_wide: bool,
    #[arg(long)]
    disable_mad_wide: bool,
    #[arg(long)]
    disable_signed_mad_wide: bool,
    #[arg(long)]
    disable_predicated_mul_wide: bool,
    #[arg(long)]
    disable_predicated_mad_wide: bool,
    #[arg(long)]
    disable_wide_high_result: bool,
    #[arg(long)]
    disable_wide_int: bool,
    #[arg(long)]
    disable_wide_minmax: bool,
    #[arg(long)]
    disable_wide_mulhi: bool,
    #[arg(long)]
    disable_predicated_wide_int: bool,
    #[arg(long)]
    disable_wide_mad64: bool,
    #[arg(long)]
    disable_signed_wide_mad64: bool,
    #[arg(long)]
    disable_predicated_wide_mad64: bool,
    #[arg(long)]
    disable_wide_set: bool,
    #[arg(long)]
    disable_predicated_wide_set: bool,
    #[arg(long)]
    disable_wide_setp: bool,
    #[arg(long)]
    disable_wide_setp_bool: bool,
    #[arg(long)]
    disable_wide_selp: bool,
    #[arg(long)]
    disable_wide_unary: bool,
    #[arg(long)]
    disable_predicated_wide_unary: bool,
    #[arg(long)]
    disable_wide_shifts: bool,
    #[arg(long)]
    disable_wide_reg_shifts: bool,
    #[arg(long)]
    disable_predicated_wide_shifts: bool,
    #[arg(long)]
    disable_predicated_wide_reg_shifts: bool,
    #[arg(long)]
    disable_wide_divrem: bool,
    #[arg(long)]
    disable_signed_wide_divrem: bool,
    #[arg(long)]
    disable_reg_wide_divrem: bool,
    #[arg(long)]
    disable_predicated_reg_wide_divrem: bool,
    #[arg(long)]
    disable_predicated_wide_divrem: bool,
    #[arg(long)]
    disable_wide_addc: bool,
    #[arg(long)]
    disable_wide_subc: bool,
    #[arg(long)]
    disable_predicated_wide_carry: bool,
    #[arg(long)]
    disable_wide_carry_chain: bool,
    #[arg(long)]
    disable_predicated_wide_carry_chain: bool,
    #[arg(long)]
    disable_addc: bool,
    #[arg(long)]
    disable_subc: bool,
    #[arg(long)]
    disable_predicated_carry: bool,
    #[arg(long)]
    disable_carry_chain: bool,
    #[arg(long)]
    disable_predicated_carry_chain: bool,
    #[arg(long)]
    disable_i32_boundary_imms: bool,
    #[arg(long)]
    disable_dp4a: bool,
    #[arg(long)]
    disable_dp2a: bool,
    #[arg(long)]
    disable_negated_predicates: bool,
    #[arg(long)]
    disable_predicated_alu: bool,
    #[arg(long)]
    disable_predicated_unary: bool,
    #[arg(long)]
    disable_cvt: bool,
    #[arg(long)]
    disable_predicated_cvt: bool,
    #[arg(long)]
    disable_narrow_cvt: bool,
    #[arg(long)]
    disable_signed_narrow_cvt: bool,
    #[arg(long)]
    disable_predicated_narrow_cvt: bool,
    #[arg(long)]
    disable_wide_cvt: bool,
    #[arg(long)]
    disable_signed_wide_cvt: bool,
    #[arg(long)]
    disable_predicated_wide_cvt: bool,
    #[arg(long)]
    disable_szext: bool,
    #[arg(long)]
    disable_signed_szext: bool,
    #[arg(long)]
    disable_predicated_szext: bool,
    #[arg(long)]
    disable_setp_bool: bool,
    #[arg(long)]
    disable_setp_dual: bool,
    #[arg(long)]
    disable_pred_logic: bool,
    #[arg(long)]
    disable_predicated_mad: bool,
    #[arg(long)]
    disable_predicated_mad_hi: bool,
    #[arg(long)]
    disable_mad_carry: bool,
    #[arg(long)]
    disable_signed_mad_carry: bool,
    #[arg(long)]
    disable_predicated_mad_carry: bool,
    #[arg(long)]
    disable_predicated_set: bool,
    #[arg(long)]
    disable_predicated_selp: bool,
    #[arg(long)]
    disable_sad: bool,
    #[arg(long)]
    disable_slct: bool,
    #[arg(long)]
    disable_predicated_sad: bool,
    #[arg(long)]
    disable_predicated_slct: bool,
    #[arg(long)]
    disable_predicated_dp: bool,
    #[arg(long)]
    disable_predicated_video: bool,
    #[arg(long)]
    disable_set: bool,
    #[arg(long)]
    disable_s32_slct: bool,
    #[arg(long)]
    disable_video: bool,
    #[arg(long)]
    disable_vsub4: bool,
}

impl Args {
    fn apply_env_overrides(self) {
        macro_rules! set_opt {
            ($field:expr, $key:literal) => {
                if let Some(v) = $field {
                    std::env::set_var($key, v.to_string());
                }
            };
        }
        macro_rules! set_bool {
            ($field:expr, $key:literal) => {
                if $field {
                    std::env::set_var($key, "1");
                }
            };
        }

        set_opt!(self.ptxas, "PTXAS");
        set_opt!(self.tmpdir, "TMPDIR");
        set_opt!(self.out_dir, "DIV_OUT_DIR");
        set_opt!(self.starting_seed, "DIV_STARTING_SEED");
        set_opt!(self.max_iters, "DIV_MAX_ITERS");
        set_opt!(self.print_every_secs, "DIV_PRINT_EVERY_SECS");
        set_opt!(self.program_bytes, "DIV_PROGRAM_BYTES");
        set_opt!(self.gpus, "DIV_GPUS");
        set_opt!(self.workers_per_gpu, "DIV_WORKERS_PER_GPU");
        set_opt!(self.min_blocks, "DIV_MIN_BLOCKS");
        set_opt!(self.max_blocks, "DIV_MAX_BLOCKS");
        set_opt!(self.min_insts_per_block, "DIV_MIN_INSTS_PER_BLOCK");
        set_opt!(self.max_insts_per_block, "DIV_MAX_INSTS_PER_BLOCK");
        set_opt!(self.working_regs, "DIV_WORKING_REGS");
        set_opt!(self.max_loop_iters, "DIV_MAX_LOOP_ITERS");
        set_opt!(self.max_immediate, "DIV_MAX_IMMEDIATE");
        set_opt!(self.max_structured_depth, "DIV_MAX_STRUCTURED_DEPTH");

        set_bool!(self.structured_control_flow, "DIV_STRUCTURED_CONTROL_FLOW");
        set_bool!(
            self.disable_structured_loops,
            "DIV_DISABLE_STRUCTURED_LOOPS"
        );
        set_bool!(self.disable_arbitrary_loops, "DIV_DISABLE_ARBITRARY_LOOPS");
        set_bool!(self.disable_lop3, "DIV_DISABLE_LOP3");
        set_bool!(self.disable_predicated_lop3, "DIV_DISABLE_PREDICATED_LOP3");
        set_bool!(self.disable_minmax, "DIV_DISABLE_MINMAX");
        set_bool!(self.disable_selp, "DIV_DISABLE_SELP");
        set_bool!(self.disable_sub, "DIV_DISABLE_SUB");
        set_bool!(self.disable_mul_lo, "DIV_DISABLE_MUL_LO");
        set_bool!(self.disable_signed_lo_alu, "DIV_DISABLE_SIGNED_LO_ALU");
        set_bool!(self.disable_sat_arith, "DIV_DISABLE_SAT_ARITH");
        set_bool!(self.disable_packed_add, "DIV_DISABLE_PACKED_ADD");
        set_bool!(
            self.disable_signed_packed_add,
            "DIV_DISABLE_SIGNED_PACKED_ADD"
        );
        set_bool!(
            self.disable_predicated_packed_add,
            "DIV_DISABLE_PREDICATED_PACKED_ADD"
        );
        set_bool!(self.disable_packed_minmax, "DIV_DISABLE_PACKED_MINMAX");
        set_bool!(
            self.disable_signed_packed_minmax,
            "DIV_DISABLE_SIGNED_PACKED_MINMAX"
        );
        set_bool!(
            self.disable_predicated_packed_minmax,
            "DIV_DISABLE_PREDICATED_PACKED_MINMAX"
        );
        set_bool!(self.disable_scalar_16bit, "DIV_DISABLE_SCALAR_16BIT");
        set_bool!(
            self.disable_signed_scalar_16bit,
            "DIV_DISABLE_SIGNED_SCALAR_16BIT"
        );
        set_bool!(
            self.disable_scalar_16bit_min,
            "DIV_DISABLE_SCALAR_16BIT_MIN"
        );
        set_bool!(
            self.disable_scalar_16bit_signed_unary,
            "DIV_DISABLE_SCALAR_16BIT_SIGNED_UNARY"
        );
        set_bool!(
            self.disable_scalar_16bit_bitwise,
            "DIV_DISABLE_SCALAR_16BIT_BITWISE"
        );
        set_bool!(
            self.disable_scalar_16bit_shifts,
            "DIV_DISABLE_SCALAR_16BIT_SHIFTS"
        );
        set_bool!(
            self.disable_scalar_16bit_compare,
            "DIV_DISABLE_SCALAR_16BIT_COMPARE"
        );
        set_bool!(
            self.disable_scalar_16bit_selp,
            "DIV_DISABLE_SCALAR_16BIT_SELP"
        );
        set_bool!(
            self.disable_predicated_scalar_16bit,
            "DIV_DISABLE_PREDICATED_SCALAR_16BIT"
        );
        set_bool!(self.disable_mulhi, "DIV_DISABLE_MULHI");
        set_bool!(self.disable_signed_mulhi, "DIV_DISABLE_SIGNED_MULHI");
        set_bool!(self.disable_mad_hi, "DIV_DISABLE_MAD_HI");
        set_bool!(self.disable_signed_mad_hi, "DIV_DISABLE_SIGNED_MAD_HI");
        set_bool!(self.disable_bitwise_binops, "DIV_DISABLE_BITWISE_BINOPS");
        set_bool!(self.disable_or, "DIV_DISABLE_OR");
        set_bool!(self.disable_xor, "DIV_DISABLE_XOR");
        set_bool!(self.disable_prmt, "DIV_DISABLE_PRMT");
        set_bool!(self.disable_predicated_prmt, "DIV_DISABLE_PREDICATED_PRMT");
        set_bool!(self.disable_reg_prmt, "DIV_DISABLE_REG_PRMT");
        set_bool!(
            self.disable_predicated_reg_prmt,
            "DIV_DISABLE_PREDICATED_REG_PRMT"
        );
        set_bool!(self.disable_prmt_modes, "DIV_DISABLE_PRMT_MODES");
        set_bool!(self.disable_not, "DIV_DISABLE_NOT");
        set_bool!(self.disable_clz, "DIV_DISABLE_CLZ");
        set_bool!(self.disable_brev, "DIV_DISABLE_BREV");
        set_bool!(self.disable_cnot, "DIV_DISABLE_CNOT");
        set_bool!(self.disable_popc, "DIV_DISABLE_POPC");
        set_bool!(self.disable_abs, "DIV_DISABLE_ABS");
        set_bool!(self.disable_special_regs, "DIV_DISABLE_SPECIAL_REGS");
        set_bool!(
            self.disable_predicated_special_regs,
            "DIV_DISABLE_PREDICATED_SPECIAL_REGS"
        );
        set_bool!(self.disable_global_loads, "DIV_DISABLE_GLOBAL_LOADS");
        set_bool!(
            self.disable_global_store_roundtrips,
            "DIV_DISABLE_GLOBAL_STORE_ROUNDTRIPS"
        );
        set_bool!(self.disable_const_memory, "DIV_DISABLE_CONST_MEMORY");
        set_bool!(self.disable_local_memory, "DIV_DISABLE_LOCAL_MEMORY");
        set_bool!(self.disable_shared_memory, "DIV_DISABLE_SHARED_MEMORY");
        set_bool!(
            self.disable_predicated_memory,
            "DIV_DISABLE_PREDICATED_MEMORY"
        );
        set_bool!(self.disable_vector_memory, "DIV_DISABLE_VECTOR_MEMORY");
        set_bool!(self.disable_wide_memory, "DIV_DISABLE_WIDE_MEMORY");
        set_bool!(
            self.disable_memory_cache_ops,
            "DIV_DISABLE_MEMORY_CACHE_OPS"
        );
        set_bool!(self.disable_f32_arith, "DIV_DISABLE_F32_ARITH");
        set_bool!(self.disable_f32_rounding, "DIV_DISABLE_F32_ROUNDING");
        set_bool!(self.disable_f32_unary, "DIV_DISABLE_F32_UNARY");
        set_bool!(self.disable_f32_cvt, "DIV_DISABLE_F32_CVT");
        set_bool!(
            self.disable_f32_special_math,
            "DIV_DISABLE_F32_SPECIAL_MATH"
        );
        set_bool!(self.disable_f32_compare, "DIV_DISABLE_F32_COMPARE");
        set_bool!(self.disable_f32_selp, "DIV_DISABLE_F32_SELP");
        set_bool!(self.disable_f64_arith, "DIV_DISABLE_F64_ARITH");
        set_bool!(self.disable_f64_rounding, "DIV_DISABLE_F64_ROUNDING");
        set_bool!(self.disable_f64_unary, "DIV_DISABLE_F64_UNARY");
        set_bool!(self.disable_f64_cvt, "DIV_DISABLE_F64_CVT");
        set_bool!(
            self.disable_f64_special_math,
            "DIV_DISABLE_F64_SPECIAL_MATH"
        );
        set_bool!(self.disable_f64_compare, "DIV_DISABLE_F64_COMPARE");
        set_bool!(self.disable_f64_selp, "DIV_DISABLE_F64_SELP");
        set_bool!(self.disable_signed_cmp, "DIV_DISABLE_SIGNED_CMP");
        set_bool!(self.disable_signed_divrem, "DIV_DISABLE_SIGNED_DIVREM");
        set_bool!(self.disable_reg_divrem, "DIV_DISABLE_REG_DIVREM");
        set_bool!(
            self.disable_predicated_reg_divrem,
            "DIV_DISABLE_PREDICATED_REG_DIVREM"
        );
        set_bool!(
            self.disable_predicated_divrem,
            "DIV_DISABLE_PREDICATED_DIVREM"
        );
        set_bool!(self.disable_funnel, "DIV_DISABLE_FUNNEL");
        set_bool!(self.disable_reg_funnel, "DIV_DISABLE_REG_FUNNEL");
        set_bool!(
            self.disable_predicated_funnel,
            "DIV_DISABLE_PREDICATED_FUNNEL"
        );
        set_bool!(self.disable_funnel_clamp, "DIV_DISABLE_FUNNEL_CLAMP");
        set_bool!(self.disable_neg, "DIV_DISABLE_NEG");
        set_bool!(self.disable_shl, "DIV_DISABLE_SHL");
        set_bool!(self.disable_shr, "DIV_DISABLE_SHR");
        set_bool!(self.disable_signed_shr, "DIV_DISABLE_SIGNED_SHR");
        set_bool!(self.disable_reg_shifts, "DIV_DISABLE_REG_SHIFTS");
        set_bool!(
            self.disable_predicated_shifts,
            "DIV_DISABLE_PREDICATED_SHIFTS"
        );
        set_bool!(
            self.disable_predicated_reg_shifts,
            "DIV_DISABLE_PREDICATED_REG_SHIFTS"
        );
        set_bool!(self.disable_bfind, "DIV_DISABLE_BFIND");
        set_bool!(self.disable_signed_bfind, "DIV_DISABLE_SIGNED_BFIND");
        set_bool!(self.disable_wide_bfind, "DIV_DISABLE_WIDE_BFIND");
        set_bool!(
            self.disable_signed_wide_bfind,
            "DIV_DISABLE_SIGNED_WIDE_BFIND"
        );
        set_bool!(
            self.disable_predicated_bfind,
            "DIV_DISABLE_PREDICATED_BFIND"
        );
        set_bool!(
            self.disable_predicated_wide_bfind,
            "DIV_DISABLE_PREDICATED_WIDE_BFIND"
        );
        set_bool!(self.disable_fns, "DIV_DISABLE_FNS");
        set_bool!(self.disable_reg_fns, "DIV_DISABLE_REG_FNS");
        set_bool!(self.disable_predicated_fns, "DIV_DISABLE_PREDICATED_FNS");
        set_bool!(
            self.disable_predicated_reg_fns,
            "DIV_DISABLE_PREDICATED_REG_FNS"
        );
        set_bool!(self.disable_bfi, "DIV_DISABLE_BFI");
        set_bool!(self.disable_bfe, "DIV_DISABLE_BFE");
        set_bool!(self.disable_bmsk, "DIV_DISABLE_BMSK");
        set_bool!(self.disable_bmsk_wrap, "DIV_DISABLE_BMSK_WRAP");
        set_bool!(
            self.disable_predicated_bitfield,
            "DIV_DISABLE_PREDICATED_BITFIELD"
        );
        set_bool!(self.disable_reg_bitfield, "DIV_DISABLE_REG_BITFIELD");
        set_bool!(
            self.disable_predicated_reg_bitfield,
            "DIV_DISABLE_PREDICATED_REG_BITFIELD"
        );
        set_bool!(self.disable_wide_bfe, "DIV_DISABLE_WIDE_BFE");
        set_bool!(self.disable_signed_wide_bfe, "DIV_DISABLE_SIGNED_WIDE_BFE");
        set_bool!(self.disable_wide_bfi, "DIV_DISABLE_WIDE_BFI");
        set_bool!(
            self.disable_predicated_wide_bitfield,
            "DIV_DISABLE_PREDICATED_WIDE_BITFIELD"
        );
        set_bool!(
            self.disable_reg_wide_bitfield,
            "DIV_DISABLE_REG_WIDE_BITFIELD"
        );
        set_bool!(
            self.disable_predicated_reg_wide_bitfield,
            "DIV_DISABLE_PREDICATED_REG_WIDE_BITFIELD"
        );
        set_bool!(self.disable_mad24, "DIV_DISABLE_MAD24");
        set_bool!(self.disable_mul24, "DIV_DISABLE_MUL24");
        set_bool!(
            self.disable_predicated_24bit,
            "DIV_DISABLE_PREDICATED_24BIT"
        );
        set_bool!(self.disable_subword_wide, "DIV_DISABLE_SUBWORD_WIDE");
        set_bool!(
            self.disable_signed_subword_wide,
            "DIV_DISABLE_SIGNED_SUBWORD_WIDE"
        );
        set_bool!(
            self.disable_predicated_subword_wide,
            "DIV_DISABLE_PREDICATED_SUBWORD_WIDE"
        );
        set_bool!(self.disable_mul_wide, "DIV_DISABLE_MUL_WIDE");
        set_bool!(self.disable_mad_wide, "DIV_DISABLE_MAD_WIDE");
        set_bool!(self.disable_signed_mad_wide, "DIV_DISABLE_SIGNED_MAD_WIDE");
        set_bool!(
            self.disable_predicated_mul_wide,
            "DIV_DISABLE_PREDICATED_MUL_WIDE"
        );
        set_bool!(
            self.disable_predicated_mad_wide,
            "DIV_DISABLE_PREDICATED_MAD_WIDE"
        );
        set_bool!(
            self.disable_wide_high_result,
            "DIV_DISABLE_WIDE_HIGH_RESULT"
        );
        set_bool!(self.disable_wide_int, "DIV_DISABLE_WIDE_INT");
        set_bool!(self.disable_wide_minmax, "DIV_DISABLE_WIDE_MINMAX");
        set_bool!(self.disable_wide_mulhi, "DIV_DISABLE_WIDE_MULHI");
        set_bool!(
            self.disable_predicated_wide_int,
            "DIV_DISABLE_PREDICATED_WIDE_INT"
        );
        set_bool!(self.disable_wide_mad64, "DIV_DISABLE_WIDE_MAD64");
        set_bool!(
            self.disable_signed_wide_mad64,
            "DIV_DISABLE_SIGNED_WIDE_MAD64"
        );
        set_bool!(
            self.disable_predicated_wide_mad64,
            "DIV_DISABLE_PREDICATED_WIDE_MAD64"
        );
        set_bool!(self.disable_wide_set, "DIV_DISABLE_WIDE_SET");
        set_bool!(
            self.disable_predicated_wide_set,
            "DIV_DISABLE_PREDICATED_WIDE_SET"
        );
        set_bool!(self.disable_wide_setp, "DIV_DISABLE_WIDE_SETP");
        set_bool!(self.disable_wide_setp_bool, "DIV_DISABLE_WIDE_SETP_BOOL");
        set_bool!(self.disable_wide_selp, "DIV_DISABLE_WIDE_SELP");
        set_bool!(self.disable_wide_unary, "DIV_DISABLE_WIDE_UNARY");
        set_bool!(
            self.disable_predicated_wide_unary,
            "DIV_DISABLE_PREDICATED_WIDE_UNARY"
        );
        set_bool!(self.disable_wide_shifts, "DIV_DISABLE_WIDE_SHIFTS");
        set_bool!(self.disable_wide_reg_shifts, "DIV_DISABLE_WIDE_REG_SHIFTS");
        set_bool!(
            self.disable_predicated_wide_shifts,
            "DIV_DISABLE_PREDICATED_WIDE_SHIFTS"
        );
        set_bool!(
            self.disable_predicated_wide_reg_shifts,
            "DIV_DISABLE_PREDICATED_WIDE_REG_SHIFTS"
        );
        set_bool!(self.disable_wide_divrem, "DIV_DISABLE_WIDE_DIVREM");
        set_bool!(
            self.disable_signed_wide_divrem,
            "DIV_DISABLE_SIGNED_WIDE_DIVREM"
        );
        set_bool!(self.disable_reg_wide_divrem, "DIV_DISABLE_REG_WIDE_DIVREM");
        set_bool!(
            self.disable_predicated_reg_wide_divrem,
            "DIV_DISABLE_PREDICATED_REG_WIDE_DIVREM"
        );
        set_bool!(
            self.disable_predicated_wide_divrem,
            "DIV_DISABLE_PREDICATED_WIDE_DIVREM"
        );
        set_bool!(self.disable_wide_addc, "DIV_DISABLE_WIDE_ADDC");
        set_bool!(self.disable_wide_subc, "DIV_DISABLE_WIDE_SUBC");
        set_bool!(
            self.disable_predicated_wide_carry,
            "DIV_DISABLE_PREDICATED_WIDE_CARRY"
        );
        set_bool!(
            self.disable_wide_carry_chain,
            "DIV_DISABLE_WIDE_CARRY_CHAIN"
        );
        set_bool!(
            self.disable_predicated_wide_carry_chain,
            "DIV_DISABLE_PREDICATED_WIDE_CARRY_CHAIN"
        );
        set_bool!(self.disable_addc, "DIV_DISABLE_ADDC");
        set_bool!(self.disable_subc, "DIV_DISABLE_SUBC");
        set_bool!(
            self.disable_predicated_carry,
            "DIV_DISABLE_PREDICATED_CARRY"
        );
        set_bool!(self.disable_carry_chain, "DIV_DISABLE_CARRY_CHAIN");
        set_bool!(
            self.disable_predicated_carry_chain,
            "DIV_DISABLE_PREDICATED_CARRY_CHAIN"
        );
        set_bool!(
            self.disable_i32_boundary_imms,
            "DIV_DISABLE_I32_BOUNDARY_IMMS"
        );
        set_bool!(self.disable_dp4a, "DIV_DISABLE_DP4A");
        set_bool!(self.disable_dp2a, "DIV_DISABLE_DP2A");
        set_bool!(
            self.disable_negated_predicates,
            "DIV_DISABLE_NEGATED_PREDICATES"
        );
        set_bool!(self.disable_predicated_alu, "DIV_DISABLE_PREDICATED_ALU");
        set_bool!(
            self.disable_predicated_unary,
            "DIV_DISABLE_PREDICATED_UNARY"
        );
        set_bool!(self.disable_cvt, "DIV_DISABLE_CVT");
        set_bool!(self.disable_predicated_cvt, "DIV_DISABLE_PREDICATED_CVT");
        set_bool!(self.disable_narrow_cvt, "DIV_DISABLE_NARROW_CVT");
        set_bool!(
            self.disable_signed_narrow_cvt,
            "DIV_DISABLE_SIGNED_NARROW_CVT"
        );
        set_bool!(
            self.disable_predicated_narrow_cvt,
            "DIV_DISABLE_PREDICATED_NARROW_CVT"
        );
        set_bool!(self.disable_wide_cvt, "DIV_DISABLE_WIDE_CVT");
        set_bool!(self.disable_signed_wide_cvt, "DIV_DISABLE_SIGNED_WIDE_CVT");
        set_bool!(
            self.disable_predicated_wide_cvt,
            "DIV_DISABLE_PREDICATED_WIDE_CVT"
        );
        set_bool!(self.disable_szext, "DIV_DISABLE_SZEXT");
        set_bool!(self.disable_signed_szext, "DIV_DISABLE_SIGNED_SZEXT");
        set_bool!(
            self.disable_predicated_szext,
            "DIV_DISABLE_PREDICATED_SZEXT"
        );
        set_bool!(self.disable_setp_bool, "DIV_DISABLE_SETP_BOOL");
        set_bool!(self.disable_setp_dual, "DIV_DISABLE_SETP_DUAL");
        set_bool!(self.disable_pred_logic, "DIV_DISABLE_PRED_LOGIC");
        set_bool!(self.disable_predicated_mad, "DIV_DISABLE_PREDICATED_MAD");
        set_bool!(
            self.disable_predicated_mad_hi,
            "DIV_DISABLE_PREDICATED_MAD_HI"
        );
        set_bool!(self.disable_mad_carry, "DIV_DISABLE_MAD_CARRY");
        set_bool!(
            self.disable_signed_mad_carry,
            "DIV_DISABLE_SIGNED_MAD_CARRY"
        );
        set_bool!(
            self.disable_predicated_mad_carry,
            "DIV_DISABLE_PREDICATED_MAD_CARRY"
        );
        set_bool!(self.disable_predicated_set, "DIV_DISABLE_PREDICATED_SET");
        set_bool!(self.disable_predicated_selp, "DIV_DISABLE_PREDICATED_SELP");
        set_bool!(self.disable_sad, "DIV_DISABLE_SAD");
        set_bool!(self.disable_slct, "DIV_DISABLE_SLCT");
        set_bool!(self.disable_predicated_sad, "DIV_DISABLE_PREDICATED_SAD");
        set_bool!(self.disable_predicated_slct, "DIV_DISABLE_PREDICATED_SLCT");
        set_bool!(self.disable_predicated_dp, "DIV_DISABLE_PREDICATED_DP");
        set_bool!(
            self.disable_predicated_video,
            "DIV_DISABLE_PREDICATED_VIDEO"
        );
        set_bool!(self.disable_set, "DIV_DISABLE_SET");
        set_bool!(self.disable_s32_slct, "DIV_DISABLE_S32_SLCT");
        set_bool!(self.disable_video, "DIV_DISABLE_VIDEO");
        set_bool!(self.disable_vsub4, "DIV_DISABLE_VSUB4");
    }
}

struct Config {
    out_dir: PathBuf,
    starting_seed: u64,
    max_iters: Option<u64>,
    print_every: Duration,
    program_bytes: usize,
    gen_config: GenConfig,
    gpus: Vec<i32>,
    workers_per_gpu: usize,
}

impl Config {
    fn from_env() -> Result<Self> {
        fn env<T: std::str::FromStr>(key: &str) -> Result<Option<T>>
        where
            T::Err: std::fmt::Display,
        {
            match std::env::var(key) {
                Ok(v) => v
                    .parse()
                    .map(Some)
                    .map_err(|e| anyhow::anyhow!("env {key}={v:?} parse error: {e}")),
                Err(_) => Ok(None),
            }
        }
        fn env_bool(key: &str) -> Result<Option<bool>> {
            match std::env::var(key) {
                Ok(v) => {
                    let parsed = match v.as_str() {
                        "1" | "true" | "TRUE" | "True" | "yes" | "YES" | "on" | "ON" => true,
                        "0" | "false" | "FALSE" | "False" | "no" | "NO" | "off" | "OFF" => false,
                        _ => anyhow::bail!("env {key}={v:?} must be a boolean"),
                    };
                    Ok(Some(parsed))
                }
                Err(_) => Ok(None),
            }
        }
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0);
        let gpus = match std::env::var("DIV_GPUS") {
            Ok(s) => s
                .split(',')
                .map(|t| {
                    t.trim()
                        .parse::<i32>()
                        .map_err(|e| anyhow::anyhow!("DIV_GPUS entry {t:?}: {e}"))
                })
                .collect::<Result<Vec<_>>>()?,
            Err(_) => {
                let n = Cuda::device_count().context("Cuda::device_count")?;
                if n <= 0 {
                    anyhow::bail!("no CUDA devices visible");
                }
                (0..n).collect()
            }
        };
        let structured_control_flow = env_bool("DIV_STRUCTURED_CONTROL_FLOW")?.unwrap_or(false);
        let disable_structured_loops = env_bool("DIV_DISABLE_STRUCTURED_LOOPS")?.unwrap_or(false);
        let disable_arbitrary_loops = env_bool("DIV_DISABLE_ARBITRARY_LOOPS")?.unwrap_or(false);
        let disable_lop3 = env_bool("DIV_DISABLE_LOP3")?.unwrap_or(false);
        let disable_predicated_lop3 = env_bool("DIV_DISABLE_PREDICATED_LOP3")?.unwrap_or(false);
        let disable_minmax = env_bool("DIV_DISABLE_MINMAX")?.unwrap_or(false);
        let disable_selp = env_bool("DIV_DISABLE_SELP")?.unwrap_or(false);
        let disable_sub = env_bool("DIV_DISABLE_SUB")?.unwrap_or(false);
        let disable_mul_lo = env_bool("DIV_DISABLE_MUL_LO")?.unwrap_or(false);
        let disable_signed_lo_alu = env_bool("DIV_DISABLE_SIGNED_LO_ALU")?.unwrap_or(false);
        let disable_sat_arith = env_bool("DIV_DISABLE_SAT_ARITH")?.unwrap_or(false);
        let disable_packed_add = env_bool("DIV_DISABLE_PACKED_ADD")?.unwrap_or(false);
        let disable_signed_packed_add = env_bool("DIV_DISABLE_SIGNED_PACKED_ADD")?.unwrap_or(false);
        let disable_predicated_packed_add =
            env_bool("DIV_DISABLE_PREDICATED_PACKED_ADD")?.unwrap_or(false);
        let disable_packed_minmax = env_bool("DIV_DISABLE_PACKED_MINMAX")?.unwrap_or(false);
        let disable_signed_packed_minmax =
            env_bool("DIV_DISABLE_SIGNED_PACKED_MINMAX")?.unwrap_or(false);
        let disable_predicated_packed_minmax =
            env_bool("DIV_DISABLE_PREDICATED_PACKED_MINMAX")?.unwrap_or(false);
        let disable_scalar_16bit = env_bool("DIV_DISABLE_SCALAR_16BIT")?.unwrap_or(false);
        let disable_signed_scalar_16bit =
            env_bool("DIV_DISABLE_SIGNED_SCALAR_16BIT")?.unwrap_or(false);
        let disable_scalar_16bit_min = env_bool("DIV_DISABLE_SCALAR_16BIT_MIN")?.unwrap_or(false);
        let disable_scalar_16bit_signed_unary =
            env_bool("DIV_DISABLE_SCALAR_16BIT_SIGNED_UNARY")?.unwrap_or(false);
        let disable_scalar_16bit_bitwise =
            env_bool("DIV_DISABLE_SCALAR_16BIT_BITWISE")?.unwrap_or(false);
        let disable_scalar_16bit_shifts =
            env_bool("DIV_DISABLE_SCALAR_16BIT_SHIFTS")?.unwrap_or(false);
        let disable_scalar_16bit_compare =
            env_bool("DIV_DISABLE_SCALAR_16BIT_COMPARE")?.unwrap_or(false);
        let disable_scalar_16bit_selp = env_bool("DIV_DISABLE_SCALAR_16BIT_SELP")?.unwrap_or(false);
        let disable_predicated_scalar_16bit =
            env_bool("DIV_DISABLE_PREDICATED_SCALAR_16BIT")?.unwrap_or(false);
        let disable_mulhi = env_bool("DIV_DISABLE_MULHI")?.unwrap_or(false);
        let disable_signed_mulhi = env_bool("DIV_DISABLE_SIGNED_MULHI")?.unwrap_or(false);
        let disable_mad_hi = env_bool("DIV_DISABLE_MAD_HI")?.unwrap_or(false);
        let disable_signed_mad_hi = env_bool("DIV_DISABLE_SIGNED_MAD_HI")?.unwrap_or(false);
        let disable_bitwise_binops = env_bool("DIV_DISABLE_BITWISE_BINOPS")?.unwrap_or(false);
        let disable_or = env_bool("DIV_DISABLE_OR")?.unwrap_or(false);
        let disable_xor = env_bool("DIV_DISABLE_XOR")?.unwrap_or(false);
        let disable_prmt = env_bool("DIV_DISABLE_PRMT")?.unwrap_or(false);
        let disable_predicated_prmt = env_bool("DIV_DISABLE_PREDICATED_PRMT")?.unwrap_or(false);
        let disable_reg_prmt = env_bool("DIV_DISABLE_REG_PRMT")?.unwrap_or(false);
        let disable_predicated_reg_prmt =
            env_bool("DIV_DISABLE_PREDICATED_REG_PRMT")?.unwrap_or(false);
        let disable_prmt_modes = env_bool("DIV_DISABLE_PRMT_MODES")?.unwrap_or(false);
        let disable_not = env_bool("DIV_DISABLE_NOT")?.unwrap_or(false);
        let disable_clz = env_bool("DIV_DISABLE_CLZ")?.unwrap_or(false);
        let disable_brev = env_bool("DIV_DISABLE_BREV")?.unwrap_or(false);
        let disable_cnot = env_bool("DIV_DISABLE_CNOT")?.unwrap_or(false);
        let disable_popc = env_bool("DIV_DISABLE_POPC")?.unwrap_or(false);
        let disable_abs = env_bool("DIV_DISABLE_ABS")?.unwrap_or(false);
        let disable_special_regs = env_bool("DIV_DISABLE_SPECIAL_REGS")?.unwrap_or(false);
        let disable_predicated_special_regs =
            env_bool("DIV_DISABLE_PREDICATED_SPECIAL_REGS")?.unwrap_or(false);
        let disable_global_loads = env_bool("DIV_DISABLE_GLOBAL_LOADS")?.unwrap_or(false);
        let disable_global_store_roundtrips =
            env_bool("DIV_DISABLE_GLOBAL_STORE_ROUNDTRIPS")?.unwrap_or(false);
        let disable_const_memory = env_bool("DIV_DISABLE_CONST_MEMORY")?.unwrap_or(false);
        let disable_local_memory = env_bool("DIV_DISABLE_LOCAL_MEMORY")?.unwrap_or(false);
        let disable_shared_memory = env_bool("DIV_DISABLE_SHARED_MEMORY")?.unwrap_or(false);
        let disable_predicated_memory = env_bool("DIV_DISABLE_PREDICATED_MEMORY")?.unwrap_or(false);
        let disable_vector_memory = env_bool("DIV_DISABLE_VECTOR_MEMORY")?.unwrap_or(false);
        let disable_wide_memory = env_bool("DIV_DISABLE_WIDE_MEMORY")?.unwrap_or(false);
        let disable_memory_cache_ops = env_bool("DIV_DISABLE_MEMORY_CACHE_OPS")?.unwrap_or(false);
        let disable_f32_arith = env_bool("DIV_DISABLE_F32_ARITH")?.unwrap_or(false);
        let disable_f32_rounding = env_bool("DIV_DISABLE_F32_ROUNDING")?.unwrap_or(false);
        let disable_f32_unary = env_bool("DIV_DISABLE_F32_UNARY")?.unwrap_or(false);
        let disable_f32_cvt = env_bool("DIV_DISABLE_F32_CVT")?.unwrap_or(false);
        let disable_f32_special_math = env_bool("DIV_DISABLE_F32_SPECIAL_MATH")?.unwrap_or(false);
        let disable_f32_compare = env_bool("DIV_DISABLE_F32_COMPARE")?.unwrap_or(false);
        let disable_f32_selp = env_bool("DIV_DISABLE_F32_SELP")?.unwrap_or(false);
        let disable_f64_arith = env_bool("DIV_DISABLE_F64_ARITH")?.unwrap_or(false);
        let disable_f64_rounding = env_bool("DIV_DISABLE_F64_ROUNDING")?.unwrap_or(false);
        let disable_f64_unary = env_bool("DIV_DISABLE_F64_UNARY")?.unwrap_or(false);
        let disable_f64_cvt = env_bool("DIV_DISABLE_F64_CVT")?.unwrap_or(false);
        let disable_f64_special_math = env_bool("DIV_DISABLE_F64_SPECIAL_MATH")?.unwrap_or(false);
        let disable_f64_compare = env_bool("DIV_DISABLE_F64_COMPARE")?.unwrap_or(false);
        let disable_f64_selp = env_bool("DIV_DISABLE_F64_SELP")?.unwrap_or(false);
        let disable_signed_cmp = env_bool("DIV_DISABLE_SIGNED_CMP")?.unwrap_or(false);
        let disable_signed_divrem = env_bool("DIV_DISABLE_SIGNED_DIVREM")?.unwrap_or(false);
        let disable_reg_divrem = env_bool("DIV_DISABLE_REG_DIVREM")?.unwrap_or(false);
        let disable_predicated_reg_divrem =
            env_bool("DIV_DISABLE_PREDICATED_REG_DIVREM")?.unwrap_or(false);
        let disable_predicated_divrem = env_bool("DIV_DISABLE_PREDICATED_DIVREM")?.unwrap_or(false);
        let disable_funnel = env_bool("DIV_DISABLE_FUNNEL")?.unwrap_or(false);
        let disable_reg_funnel = env_bool("DIV_DISABLE_REG_FUNNEL")?.unwrap_or(false);
        let disable_predicated_funnel = env_bool("DIV_DISABLE_PREDICATED_FUNNEL")?.unwrap_or(false);
        let disable_funnel_clamp = env_bool("DIV_DISABLE_FUNNEL_CLAMP")?.unwrap_or(false);
        let disable_neg = env_bool("DIV_DISABLE_NEG")?.unwrap_or(false);
        let disable_shl = env_bool("DIV_DISABLE_SHL")?.unwrap_or(false);
        let disable_shr = env_bool("DIV_DISABLE_SHR")?.unwrap_or(false);
        let disable_signed_shr = env_bool("DIV_DISABLE_SIGNED_SHR")?.unwrap_or(false);
        let disable_reg_shifts = env_bool("DIV_DISABLE_REG_SHIFTS")?.unwrap_or(false);
        let disable_predicated_shifts = env_bool("DIV_DISABLE_PREDICATED_SHIFTS")?.unwrap_or(false);
        let disable_predicated_reg_shifts =
            env_bool("DIV_DISABLE_PREDICATED_REG_SHIFTS")?.unwrap_or(false);
        let disable_bfind = env_bool("DIV_DISABLE_BFIND")?.unwrap_or(false);
        let disable_signed_bfind = env_bool("DIV_DISABLE_SIGNED_BFIND")?.unwrap_or(false);
        let disable_wide_bfind = env_bool("DIV_DISABLE_WIDE_BFIND")?.unwrap_or(false);
        let disable_signed_wide_bfind = env_bool("DIV_DISABLE_SIGNED_WIDE_BFIND")?.unwrap_or(false);
        let disable_predicated_bfind = env_bool("DIV_DISABLE_PREDICATED_BFIND")?.unwrap_or(false);
        let disable_predicated_wide_bfind =
            env_bool("DIV_DISABLE_PREDICATED_WIDE_BFIND")?.unwrap_or(false);
        let disable_fns = env_bool("DIV_DISABLE_FNS")?.unwrap_or(false);
        let disable_reg_fns = env_bool("DIV_DISABLE_REG_FNS")?.unwrap_or(false);
        let disable_predicated_fns = env_bool("DIV_DISABLE_PREDICATED_FNS")?.unwrap_or(false);
        let disable_predicated_reg_fns =
            env_bool("DIV_DISABLE_PREDICATED_REG_FNS")?.unwrap_or(false);
        let disable_bfi = env_bool("DIV_DISABLE_BFI")?.unwrap_or(false);
        let disable_bfe = env_bool("DIV_DISABLE_BFE")?.unwrap_or(false);
        let disable_bmsk = env_bool("DIV_DISABLE_BMSK")?.unwrap_or(false);
        let disable_bmsk_wrap = env_bool("DIV_DISABLE_BMSK_WRAP")?.unwrap_or(false);
        let disable_predicated_bitfield =
            env_bool("DIV_DISABLE_PREDICATED_BITFIELD")?.unwrap_or(false);
        let disable_reg_bitfield = env_bool("DIV_DISABLE_REG_BITFIELD")?.unwrap_or(false);
        let disable_predicated_reg_bitfield =
            env_bool("DIV_DISABLE_PREDICATED_REG_BITFIELD")?.unwrap_or(false);
        let disable_wide_bfe = env_bool("DIV_DISABLE_WIDE_BFE")?.unwrap_or(false);
        let disable_signed_wide_bfe = env_bool("DIV_DISABLE_SIGNED_WIDE_BFE")?.unwrap_or(false);
        let disable_wide_bfi = env_bool("DIV_DISABLE_WIDE_BFI")?.unwrap_or(false);
        let disable_predicated_wide_bitfield =
            env_bool("DIV_DISABLE_PREDICATED_WIDE_BITFIELD")?.unwrap_or(false);
        let disable_reg_wide_bitfield = env_bool("DIV_DISABLE_REG_WIDE_BITFIELD")?.unwrap_or(false);
        let disable_predicated_reg_wide_bitfield =
            env_bool("DIV_DISABLE_PREDICATED_REG_WIDE_BITFIELD")?.unwrap_or(false);
        let disable_mad24 = env_bool("DIV_DISABLE_MAD24")?.unwrap_or(false);
        let disable_mul24 = env_bool("DIV_DISABLE_MUL24")?.unwrap_or(false);
        let disable_predicated_24bit = env_bool("DIV_DISABLE_PREDICATED_24BIT")?.unwrap_or(false);
        let disable_subword_wide = env_bool("DIV_DISABLE_SUBWORD_WIDE")?.unwrap_or(false);
        let disable_signed_subword_wide =
            env_bool("DIV_DISABLE_SIGNED_SUBWORD_WIDE")?.unwrap_or(false);
        let disable_predicated_subword_wide =
            env_bool("DIV_DISABLE_PREDICATED_SUBWORD_WIDE")?.unwrap_or(false);
        let disable_mul_wide = env_bool("DIV_DISABLE_MUL_WIDE")?.unwrap_or(false);
        let disable_mad_wide = env_bool("DIV_DISABLE_MAD_WIDE")?.unwrap_or(false);
        let disable_signed_mad_wide = env_bool("DIV_DISABLE_SIGNED_MAD_WIDE")?.unwrap_or(false);
        let disable_predicated_mul_wide =
            env_bool("DIV_DISABLE_PREDICATED_MUL_WIDE")?.unwrap_or(false);
        let disable_predicated_mad_wide =
            env_bool("DIV_DISABLE_PREDICATED_MAD_WIDE")?.unwrap_or(false);
        let disable_wide_high_result = env_bool("DIV_DISABLE_WIDE_HIGH_RESULT")?.unwrap_or(false);
        let disable_wide_int = env_bool("DIV_DISABLE_WIDE_INT")?.unwrap_or(false);
        let disable_wide_minmax = env_bool("DIV_DISABLE_WIDE_MINMAX")?.unwrap_or(false);
        let disable_wide_mulhi = env_bool("DIV_DISABLE_WIDE_MULHI")?.unwrap_or(false);
        let disable_predicated_wide_int =
            env_bool("DIV_DISABLE_PREDICATED_WIDE_INT")?.unwrap_or(false);
        let disable_wide_mad64 = env_bool("DIV_DISABLE_WIDE_MAD64")?.unwrap_or(false);
        let disable_signed_wide_mad64 = env_bool("DIV_DISABLE_SIGNED_WIDE_MAD64")?.unwrap_or(false);
        let disable_predicated_wide_mad64 =
            env_bool("DIV_DISABLE_PREDICATED_WIDE_MAD64")?.unwrap_or(false);
        let disable_wide_set = env_bool("DIV_DISABLE_WIDE_SET")?.unwrap_or(false);
        let disable_predicated_wide_set =
            env_bool("DIV_DISABLE_PREDICATED_WIDE_SET")?.unwrap_or(false);
        let disable_wide_setp = env_bool("DIV_DISABLE_WIDE_SETP")?.unwrap_or(false);
        let disable_wide_setp_bool = env_bool("DIV_DISABLE_WIDE_SETP_BOOL")?.unwrap_or(false);
        let disable_wide_selp = env_bool("DIV_DISABLE_WIDE_SELP")?.unwrap_or(false);
        let disable_wide_unary = env_bool("DIV_DISABLE_WIDE_UNARY")?.unwrap_or(false);
        let disable_predicated_wide_unary =
            env_bool("DIV_DISABLE_PREDICATED_WIDE_UNARY")?.unwrap_or(false);
        let disable_wide_shifts = env_bool("DIV_DISABLE_WIDE_SHIFTS")?.unwrap_or(false);
        let disable_wide_reg_shifts = env_bool("DIV_DISABLE_WIDE_REG_SHIFTS")?.unwrap_or(false);
        let disable_predicated_wide_shifts =
            env_bool("DIV_DISABLE_PREDICATED_WIDE_SHIFTS")?.unwrap_or(false);
        let disable_predicated_wide_reg_shifts =
            env_bool("DIV_DISABLE_PREDICATED_WIDE_REG_SHIFTS")?.unwrap_or(false);
        let disable_wide_divrem = env_bool("DIV_DISABLE_WIDE_DIVREM")?.unwrap_or(false);
        let disable_signed_wide_divrem =
            env_bool("DIV_DISABLE_SIGNED_WIDE_DIVREM")?.unwrap_or(false);
        let disable_reg_wide_divrem = env_bool("DIV_DISABLE_REG_WIDE_DIVREM")?.unwrap_or(false);
        let disable_predicated_reg_wide_divrem =
            env_bool("DIV_DISABLE_PREDICATED_REG_WIDE_DIVREM")?.unwrap_or(false);
        let disable_predicated_wide_divrem =
            env_bool("DIV_DISABLE_PREDICATED_WIDE_DIVREM")?.unwrap_or(false);
        let disable_wide_addc = env_bool("DIV_DISABLE_WIDE_ADDC")?.unwrap_or(false);
        let disable_wide_subc = env_bool("DIV_DISABLE_WIDE_SUBC")?.unwrap_or(false);
        let disable_predicated_wide_carry =
            env_bool("DIV_DISABLE_PREDICATED_WIDE_CARRY")?.unwrap_or(false);
        let disable_wide_carry_chain = env_bool("DIV_DISABLE_WIDE_CARRY_CHAIN")?.unwrap_or(false);
        let disable_predicated_wide_carry_chain =
            env_bool("DIV_DISABLE_PREDICATED_WIDE_CARRY_CHAIN")?.unwrap_or(false);
        let disable_addc = env_bool("DIV_DISABLE_ADDC")?.unwrap_or(false);
        let disable_subc = env_bool("DIV_DISABLE_SUBC")?.unwrap_or(false);
        let disable_predicated_carry = env_bool("DIV_DISABLE_PREDICATED_CARRY")?.unwrap_or(false);
        let disable_carry_chain = env_bool("DIV_DISABLE_CARRY_CHAIN")?.unwrap_or(false);
        let disable_predicated_carry_chain =
            env_bool("DIV_DISABLE_PREDICATED_CARRY_CHAIN")?.unwrap_or(false);
        let disable_i32_boundary_imms = env_bool("DIV_DISABLE_I32_BOUNDARY_IMMS")?.unwrap_or(false);
        let disable_dp4a = env_bool("DIV_DISABLE_DP4A")?.unwrap_or(false);
        let disable_dp2a = env_bool("DIV_DISABLE_DP2A")?.unwrap_or(false);
        let disable_negated_predicates =
            env_bool("DIV_DISABLE_NEGATED_PREDICATES")?.unwrap_or(false);
        let disable_predicated_alu = env_bool("DIV_DISABLE_PREDICATED_ALU")?.unwrap_or(false);
        let disable_predicated_unary = env_bool("DIV_DISABLE_PREDICATED_UNARY")?.unwrap_or(false);
        let disable_cvt = env_bool("DIV_DISABLE_CVT")?.unwrap_or(false);
        let disable_predicated_cvt = env_bool("DIV_DISABLE_PREDICATED_CVT")?.unwrap_or(false);
        let disable_narrow_cvt = env_bool("DIV_DISABLE_NARROW_CVT")?.unwrap_or(false);
        let disable_signed_narrow_cvt = env_bool("DIV_DISABLE_SIGNED_NARROW_CVT")?.unwrap_or(false);
        let disable_predicated_narrow_cvt =
            env_bool("DIV_DISABLE_PREDICATED_NARROW_CVT")?.unwrap_or(false);
        let disable_wide_cvt = env_bool("DIV_DISABLE_WIDE_CVT")?.unwrap_or(false);
        let disable_signed_wide_cvt = env_bool("DIV_DISABLE_SIGNED_WIDE_CVT")?.unwrap_or(false);
        let disable_predicated_wide_cvt =
            env_bool("DIV_DISABLE_PREDICATED_WIDE_CVT")?.unwrap_or(false);
        let disable_szext = env_bool("DIV_DISABLE_SZEXT")?.unwrap_or(false);
        let disable_signed_szext = env_bool("DIV_DISABLE_SIGNED_SZEXT")?.unwrap_or(false);
        let disable_predicated_szext = env_bool("DIV_DISABLE_PREDICATED_SZEXT")?.unwrap_or(false);
        let disable_setp_bool = env_bool("DIV_DISABLE_SETP_BOOL")?.unwrap_or(false);
        let disable_setp_dual = env_bool("DIV_DISABLE_SETP_DUAL")?.unwrap_or(false);
        let disable_pred_logic = env_bool("DIV_DISABLE_PRED_LOGIC")?.unwrap_or(false);
        let disable_predicated_mad = env_bool("DIV_DISABLE_PREDICATED_MAD")?.unwrap_or(false);
        let disable_predicated_mad_hi = env_bool("DIV_DISABLE_PREDICATED_MAD_HI")?.unwrap_or(false);
        let disable_mad_carry = env_bool("DIV_DISABLE_MAD_CARRY")?.unwrap_or(false);
        let disable_signed_mad_carry = env_bool("DIV_DISABLE_SIGNED_MAD_CARRY")?.unwrap_or(false);
        let disable_predicated_mad_carry =
            env_bool("DIV_DISABLE_PREDICATED_MAD_CARRY")?.unwrap_or(false);
        let disable_predicated_set = env_bool("DIV_DISABLE_PREDICATED_SET")?.unwrap_or(false);
        let disable_predicated_selp = env_bool("DIV_DISABLE_PREDICATED_SELP")?.unwrap_or(false);
        let disable_sad = env_bool("DIV_DISABLE_SAD")?.unwrap_or(false);
        let disable_slct = env_bool("DIV_DISABLE_SLCT")?.unwrap_or(false);
        let disable_predicated_sad = env_bool("DIV_DISABLE_PREDICATED_SAD")?.unwrap_or(false);
        let disable_predicated_slct = env_bool("DIV_DISABLE_PREDICATED_SLCT")?.unwrap_or(false);
        let disable_predicated_dp = env_bool("DIV_DISABLE_PREDICATED_DP")?.unwrap_or(false);
        let disable_predicated_video = env_bool("DIV_DISABLE_PREDICATED_VIDEO")?.unwrap_or(false);
        let disable_set = env_bool("DIV_DISABLE_SET")?.unwrap_or(false);
        let disable_s32_slct = env_bool("DIV_DISABLE_S32_SLCT")?.unwrap_or(false);
        let disable_video = env_bool("DIV_DISABLE_VIDEO")?.unwrap_or(false);
        let disable_vsub4 = env_bool("DIV_DISABLE_VSUB4")?.unwrap_or(false);
        let mut gen_config = GenConfig {
            control_flow: if structured_control_flow {
                ControlFlowMode::Structured
            } else {
                ControlFlowMode::Arbitrary
            },
            emit_structured_loops: !disable_structured_loops,
            emit_arbitrary_loops: !disable_arbitrary_loops,
            emit_lop3: !disable_lop3,
            emit_predicated_lop3: !disable_predicated_lop3 && !disable_lop3,
            emit_minmax: !disable_minmax,
            emit_selp: !disable_selp,
            emit_sub: !disable_sub,
            emit_mul_lo: !disable_mul_lo,
            emit_signed_lo_alu: !disable_signed_lo_alu,
            emit_sat_arith: !disable_sat_arith && !disable_signed_lo_alu,
            emit_packed_add: !disable_packed_add,
            emit_signed_packed_add: !disable_signed_packed_add,
            emit_predicated_packed_add: !disable_predicated_packed_add
                && !disable_packed_add
                && !disable_predicated_alu,
            emit_packed_minmax: !disable_packed_minmax,
            emit_signed_packed_minmax: !disable_signed_packed_minmax && !disable_packed_minmax,
            emit_predicated_packed_minmax: !disable_predicated_packed_minmax
                && !disable_packed_minmax
                && !disable_predicated_alu,
            emit_scalar_16bit: !disable_scalar_16bit,
            emit_signed_scalar_16bit: !disable_signed_scalar_16bit && !disable_scalar_16bit,
            emit_scalar_16bit_min: !disable_scalar_16bit_min && !disable_scalar_16bit,
            emit_scalar_16bit_signed_unary: !disable_scalar_16bit_signed_unary
                && !disable_signed_scalar_16bit
                && !disable_scalar_16bit,
            emit_scalar_16bit_bitwise: !disable_scalar_16bit_bitwise && !disable_scalar_16bit,
            emit_scalar_16bit_shifts: !disable_scalar_16bit_shifts && !disable_scalar_16bit,
            emit_scalar_16bit_compare: !disable_scalar_16bit_compare && !disable_scalar_16bit,
            emit_scalar_16bit_selp: !disable_scalar_16bit_selp
                && !disable_scalar_16bit_compare
                && !disable_scalar_16bit,
            emit_predicated_scalar_16bit: !disable_predicated_scalar_16bit
                && !disable_scalar_16bit
                && !disable_predicated_alu,
            emit_mulhi: !disable_mulhi,
            emit_signed_mulhi: !disable_signed_mulhi,
            emit_mad_hi: !disable_mad_hi,
            emit_signed_mad_hi: !disable_signed_mad_hi,
            emit_bitwise_binops: !disable_bitwise_binops,
            emit_or: !disable_or,
            emit_xor: !disable_xor,
            emit_prmt: !disable_prmt,
            emit_predicated_prmt: !disable_predicated_prmt && !disable_prmt,
            emit_reg_prmt: !disable_reg_prmt && !disable_prmt && !disable_bitwise_binops,
            emit_predicated_reg_prmt: !disable_predicated_reg_prmt
                && !disable_reg_prmt
                && !disable_predicated_prmt
                && !disable_prmt
                && !disable_bitwise_binops,
            emit_prmt_modes: !disable_prmt_modes && !disable_prmt,
            emit_not: !disable_not,
            emit_clz: !disable_clz,
            emit_brev: !disable_brev,
            emit_cnot: !disable_cnot,
            emit_popc: !disable_popc,
            emit_abs: !disable_abs,
            emit_special_regs: !disable_special_regs,
            emit_predicated_special_regs: !disable_predicated_special_regs
                && !disable_special_regs
                && !disable_predicated_unary,
            emit_global_loads: !disable_global_loads,
            emit_global_store_roundtrips: !disable_global_store_roundtrips
                && !disable_mul_wide
                && !disable_wide_int,
            emit_const_memory: !disable_const_memory,
            emit_local_memory: !disable_local_memory,
            emit_shared_memory: !disable_shared_memory && !disable_mul_wide && !disable_wide_int,
            emit_predicated_memory: !disable_predicated_memory,
            emit_vector_memory: !disable_vector_memory,
            emit_wide_memory: !disable_wide_memory,
            emit_memory_cache_ops: !disable_memory_cache_ops,
            emit_f32_arith: !disable_f32_arith && !disable_bitwise_binops,
            emit_f32_rounding: !disable_f32_rounding
                && !disable_f32_arith
                && !disable_bitwise_binops,
            emit_f32_unary: !disable_f32_unary && !disable_bitwise_binops,
            emit_f32_cvt: !disable_f32_cvt && !disable_bitwise_binops,
            emit_f32_special_math: !disable_f32_special_math && !disable_bitwise_binops,
            emit_f32_compare: !disable_f32_compare && !disable_bitwise_binops,
            emit_f32_selp: !disable_f32_selp && !disable_f32_compare && !disable_bitwise_binops,
            emit_f64_arith: !disable_f64_arith && !disable_bitwise_binops,
            emit_f64_rounding: !disable_f64_rounding
                && !disable_f64_arith
                && !disable_bitwise_binops,
            emit_f64_unary: !disable_f64_unary && !disable_bitwise_binops,
            emit_f64_cvt: !disable_f64_cvt && !disable_bitwise_binops,
            emit_f64_special_math: !disable_f64_special_math && !disable_bitwise_binops,
            emit_f64_compare: !disable_f64_compare && !disable_bitwise_binops,
            emit_f64_selp: !disable_f64_selp && !disable_f64_compare && !disable_bitwise_binops,
            emit_signed_cmp: !disable_signed_cmp,
            emit_signed_divrem: !disable_signed_divrem,
            emit_reg_divrem: !disable_reg_divrem && !disable_bitwise_binops && !disable_or,
            emit_predicated_reg_divrem: !disable_predicated_reg_divrem
                && !disable_reg_divrem
                && !disable_bitwise_binops
                && !disable_or,
            emit_predicated_divrem: !disable_predicated_divrem,
            emit_funnel: !disable_funnel,
            emit_reg_funnel: !disable_reg_funnel && !disable_funnel,
            emit_predicated_funnel: !disable_predicated_funnel && !disable_funnel,
            emit_funnel_clamp: !disable_funnel_clamp && !disable_funnel,
            emit_neg: !disable_neg,
            emit_shl: !disable_shl,
            emit_shr: !disable_shr,
            emit_signed_shr: !disable_signed_shr,
            emit_reg_shifts: !disable_reg_shifts && !disable_bitwise_binops,
            emit_predicated_shifts: !disable_predicated_shifts,
            emit_predicated_reg_shifts: !disable_predicated_reg_shifts
                && !disable_reg_shifts
                && !disable_bitwise_binops,
            emit_bfind: !disable_bfind,
            emit_signed_bfind: !disable_signed_bfind,
            emit_wide_bfind: !disable_wide_bfind,
            emit_signed_wide_bfind: !disable_signed_wide_bfind,
            emit_predicated_bfind: !disable_predicated_bfind && !disable_bfind,
            emit_predicated_wide_bfind: !disable_predicated_wide_bfind
                && !disable_predicated_bfind
                && !disable_wide_bfind
                && !disable_bfind,
            emit_fns: !disable_fns,
            emit_reg_fns: !disable_reg_fns && !disable_fns && !disable_bitwise_binops,
            emit_predicated_fns: !disable_predicated_fns && !disable_fns,
            emit_predicated_reg_fns: !disable_predicated_reg_fns
                && !disable_reg_fns
                && !disable_predicated_fns
                && !disable_fns
                && !disable_bitwise_binops,
            emit_bfi: !disable_bfi,
            emit_bfe: !disable_bfe,
            emit_bmsk: !disable_bmsk,
            emit_bmsk_wrap: !disable_bmsk_wrap && !disable_bmsk,
            emit_predicated_bitfield: !disable_predicated_bitfield,
            emit_reg_bitfield: !disable_reg_bitfield,
            emit_predicated_reg_bitfield: !disable_predicated_reg_bitfield
                && !disable_reg_bitfield
                && !disable_predicated_bitfield,
            emit_wide_bfe: !disable_wide_bfe,
            emit_signed_wide_bfe: !disable_signed_wide_bfe,
            emit_wide_bfi: !disable_wide_bfi,
            emit_predicated_wide_bitfield: !disable_predicated_wide_bitfield
                && !disable_predicated_bitfield
                && (!disable_wide_bfe || !disable_wide_bfi),
            emit_reg_wide_bitfield: !disable_reg_wide_bitfield
                && !disable_bitwise_binops
                && (!disable_wide_bfe || !disable_wide_bfi),
            emit_predicated_reg_wide_bitfield: !disable_predicated_reg_wide_bitfield
                && !disable_reg_wide_bitfield
                && !disable_predicated_wide_bitfield
                && !disable_predicated_bitfield
                && !disable_bitwise_binops
                && (!disable_wide_bfe || !disable_wide_bfi),
            emit_mad24: !disable_mad24,
            emit_mul24: !disable_mul24,
            emit_predicated_24bit: !disable_predicated_24bit,
            emit_subword_wide: !disable_subword_wide,
            emit_signed_subword_wide: !disable_signed_subword_wide && !disable_subword_wide,
            emit_predicated_subword_wide: !disable_predicated_subword_wide && !disable_subword_wide,
            emit_mul_wide: !disable_mul_wide,
            emit_mad_wide: !disable_mad_wide,
            emit_signed_mad_wide: !disable_signed_mad_wide,
            emit_predicated_mul_wide: !disable_predicated_mul_wide && !disable_mul_wide,
            emit_predicated_mad_wide: !disable_predicated_mad_wide && !disable_mad_wide,
            emit_wide_high_result: !disable_wide_high_result,
            emit_wide_int: !disable_wide_int,
            emit_wide_minmax: !disable_wide_minmax && !disable_wide_int,
            emit_wide_mulhi: !disable_wide_mulhi && !disable_wide_int,
            emit_predicated_wide_int: !disable_predicated_wide_int && !disable_wide_int,
            emit_wide_mad64: !disable_wide_mad64,
            emit_signed_wide_mad64: !disable_signed_wide_mad64,
            emit_predicated_wide_mad64: !disable_predicated_wide_mad64 && !disable_wide_mad64,
            emit_wide_set: !disable_wide_set && !disable_set,
            emit_predicated_wide_set: !disable_predicated_wide_set
                && !disable_wide_set
                && !disable_predicated_set
                && !disable_set,
            emit_wide_setp: !disable_wide_setp && !disable_predicated_alu,
            emit_wide_setp_bool: !disable_wide_setp_bool && !disable_predicated_alu,
            emit_wide_selp: !disable_wide_selp,
            emit_wide_unary: !disable_wide_unary,
            emit_predicated_wide_unary: !disable_predicated_wide_unary && !disable_wide_unary,
            emit_wide_shifts: !disable_wide_shifts,
            emit_wide_reg_shifts: !disable_wide_reg_shifts
                && !disable_wide_shifts
                && !disable_bitwise_binops,
            emit_predicated_wide_shifts: !disable_predicated_wide_shifts && !disable_wide_shifts,
            emit_predicated_wide_reg_shifts: !disable_predicated_wide_reg_shifts
                && !disable_wide_reg_shifts
                && !disable_wide_shifts
                && !disable_bitwise_binops,
            emit_wide_divrem: !disable_wide_divrem,
            emit_signed_wide_divrem: !disable_signed_wide_divrem,
            emit_reg_wide_divrem: !disable_reg_wide_divrem
                && !disable_wide_divrem
                && !disable_bitwise_binops
                && !disable_or,
            emit_predicated_reg_wide_divrem: !disable_predicated_reg_wide_divrem
                && !disable_reg_wide_divrem
                && !disable_predicated_wide_divrem
                && !disable_wide_divrem
                && !disable_bitwise_binops
                && !disable_or,
            emit_predicated_wide_divrem: !disable_predicated_wide_divrem && !disable_wide_divrem,
            emit_wide_addc: !disable_wide_addc,
            emit_wide_subc: !disable_wide_subc,
            emit_predicated_wide_carry: !disable_predicated_wide_carry
                && (!disable_wide_addc || !disable_wide_subc),
            emit_wide_carry_chain: !disable_wide_carry_chain
                && (!disable_wide_addc || !disable_wide_subc),
            emit_predicated_wide_carry_chain: !disable_predicated_wide_carry_chain
                && !disable_wide_carry_chain
                && !disable_predicated_wide_carry
                && (!disable_wide_addc || !disable_wide_subc),
            emit_addc: !disable_addc,
            emit_subc: !disable_subc,
            emit_predicated_carry: !disable_predicated_carry && (!disable_addc || !disable_subc),
            emit_carry_chain: !disable_carry_chain && (!disable_addc || !disable_subc),
            emit_predicated_carry_chain: !disable_predicated_carry_chain
                && !disable_carry_chain
                && !disable_predicated_carry
                && (!disable_addc || !disable_subc),
            emit_i32_boundary_immediates: !disable_i32_boundary_imms,
            emit_dp4a: !disable_dp4a,
            emit_dp2a: !disable_dp2a,
            emit_negated_predicates: !disable_negated_predicates,
            emit_predicated_alu: !disable_predicated_alu,
            emit_predicated_unary: !disable_predicated_unary,
            emit_cvt: !disable_cvt,
            emit_predicated_cvt: !disable_predicated_cvt && !disable_cvt,
            emit_narrow_cvt: !disable_narrow_cvt,
            emit_signed_narrow_cvt: !disable_signed_narrow_cvt,
            emit_predicated_narrow_cvt: !disable_predicated_narrow_cvt
                && !disable_narrow_cvt
                && !disable_predicated_cvt,
            emit_wide_cvt: !disable_wide_cvt,
            emit_signed_wide_cvt: !disable_signed_wide_cvt,
            emit_predicated_wide_cvt: !disable_predicated_wide_cvt
                && !disable_wide_cvt
                && !disable_predicated_cvt,
            emit_szext: !disable_szext,
            emit_signed_szext: !disable_signed_szext,
            emit_predicated_szext: !disable_predicated_szext && !disable_szext,
            emit_setp_bool: !disable_setp_bool && !disable_predicated_alu,
            emit_setp_dual: !disable_setp_dual && !disable_predicated_alu,
            emit_pred_logic: !disable_pred_logic && !disable_predicated_alu,
            emit_predicated_mad: !disable_predicated_mad && !disable_mul_lo,
            emit_predicated_mad_hi: !disable_predicated_mad_hi && !disable_mad_hi,
            emit_mad_carry: !disable_mad_carry,
            emit_signed_mad_carry: !disable_signed_mad_carry && !disable_mad_carry,
            emit_predicated_mad_carry: !disable_predicated_mad_carry && !disable_mad_carry,
            emit_predicated_set: !disable_predicated_set && !disable_set,
            emit_predicated_selp: !disable_predicated_selp && !disable_selp,
            emit_sad: !disable_sad,
            emit_slct: !disable_slct,
            emit_predicated_sad: !disable_predicated_sad && !disable_sad,
            emit_predicated_slct: !disable_predicated_slct && !disable_slct,
            emit_predicated_dp: !disable_predicated_dp,
            emit_predicated_video: !disable_predicated_video && !disable_video,
            emit_set: !disable_set,
            emit_s32_slct: !disable_s32_slct && !disable_slct,
            emit_video: !disable_video,
            emit_vsub4: !disable_vsub4,
            ..GenConfig::default()
        };
        if let Some(v) = env("DIV_MIN_BLOCKS")? {
            gen_config.min_blocks = v;
        }
        if let Some(v) = env("DIV_MAX_BLOCKS")? {
            gen_config.max_blocks = v;
        }
        if let Some(v) = env("DIV_MIN_INSTS_PER_BLOCK")? {
            gen_config.min_insts_per_block = v;
        }
        if let Some(v) = env("DIV_MAX_INSTS_PER_BLOCK")? {
            gen_config.max_insts_per_block = v;
        }
        if let Some(v) = env("DIV_WORKING_REGS")? {
            gen_config.n_working_regs = v;
        }
        if let Some(v) = env("DIV_MAX_LOOP_ITERS")? {
            gen_config.max_loop_iters = v;
        }
        if let Some(v) = env("DIV_MAX_IMMEDIATE")? {
            gen_config.max_immediate = v;
        }
        if let Some(v) = env("DIV_MAX_STRUCTURED_DEPTH")? {
            gen_config.max_structured_depth = v;
        }
        Ok(Config {
            out_dir: env::<String>("DIV_OUT_DIR")?
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("divergences")),
            starting_seed: env("DIV_STARTING_SEED")?.unwrap_or(nanos),
            max_iters: env("DIV_MAX_ITERS")?,
            print_every: Duration::from_secs(env("DIV_PRINT_EVERY_SECS")?.unwrap_or(5)),
            program_bytes: env("DIV_PROGRAM_BYTES")?.unwrap_or(4096),
            gen_config,
            gpus,
            workers_per_gpu: env("DIV_WORKERS_PER_GPU")?.unwrap_or(16),
        })
    }
}

/// Atomic counters shared across all worker threads.
struct Stats {
    next_seed: AtomicU64,
    iters: AtomicU64,
    divergences: AtomicU64,
    both_failed: AtomicU64,
    skipped: AtomicU64,
}

fn save_divergence(
    out_dir: &Path,
    log_lock: &Mutex<()>,
    seed: u64,
    bytes: &[u8],
    ptx: &str,
    input: &[u8],
    outcome: &DiffOutcome,
) -> Result<PathBuf> {
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let dir = out_dir.join(format!("div-{ts}-{seed:016x}"));
    std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
    std::fs::write(dir.join("seed.bin"), bytes)?;
    std::fs::write(dir.join("program.ptx"), ptx)?;
    std::fs::write(dir.join("input.bin"), input)?;
    match &outcome.o0 {
        Ok(b) => std::fs::write(dir.join("output_o0.bin"), b)?,
        Err(e) => std::fs::write(dir.join("output_o0.err"), format!("{e:#}"))?,
    }
    match &outcome.o3 {
        Ok(b) => std::fs::write(dir.join("output_o3.bin"), b)?,
        Err(e) => std::fs::write(dir.join("output_o3.err"), format!("{e:#}"))?,
    }
    let summary = format!(
        "seed: {seed}\nseed_hex: {seed:016x}\no0_ok: {}\no3_ok: {}\nverdict: {}\n",
        outcome.o0.is_ok(),
        outcome.o3.is_ok(),
        match (&outcome.o0, &outcome.o3) {
            (Ok(a), Ok(b)) if a == b => "MATCH",
            (Ok(_), Ok(_)) => "OUTPUT_MISMATCH",
            (Ok(_), Err(_)) => "O3_FAILED_O0_OK",
            (Err(_), Ok(_)) => "O0_FAILED_O3_OK",
            (Err(_), Err(_)) => "BOTH_FAILED",
        }
    );
    std::fs::write(dir.join("summary.txt"), summary)?;
    // Serialize the announcement so concurrent workers don't interleave lines.
    let _g = log_lock.lock().unwrap_or_else(|p| p.into_inner());
    eprintln!("DIVERGENCE seed=0x{seed:016x} saved={}", dir.display());
    Ok(dir)
}

/// One worker thread: owns a CUDA context on `gpu` and a single pair of
/// reusable in/out buffers. Loops, pulling seeds from `stats.next_seed`,
/// until either `max_iters` or `stats.stop_at_iter` is hit (we hit the
/// stop sentinel by setting it to 0 on Ctrl-C — see main).
fn worker_loop(
    worker_id: usize,
    gpu: i32,
    cfg: &Config,
    arch: &str,
    stats: &Stats,
    log_lock: &Mutex<()>,
) -> Result<()> {
    let cuda = Cuda::init(gpu).with_context(|| format!("Cuda::init gpu={gpu}"))?;
    let bufs: CudaBuffers = cuda
        .alloc_buffers(input_len(), output_len())
        .context("alloc_buffers")?;

    loop {
        let i = stats.next_seed.fetch_add(1, Ordering::Relaxed);
        let i = i.wrapping_sub(cfg.starting_seed);
        if let Some(max) = cfg.max_iters {
            if i >= max {
                return Ok(());
            }
        }
        let seed = cfg.starting_seed.wrapping_add(i);
        let bytes = bytes_from_seed(seed, cfg.program_bytes);
        let ptx = match generate_from_bytes_with_config(&bytes, &cfg.gen_config) {
            Ok(p) => p,
            Err(_) => {
                stats.skipped.fetch_add(1, Ordering::Relaxed);
                stats.iters.fetch_add(1, Ordering::Relaxed);
                continue;
            }
        };
        let input = input_for_seed(seed);

        let run_at = |opt: &str| -> Result<Vec<u8>> {
            let cubin = compile(&ptx, &[arch, opt])?;
            cuda.launch_with(
                &bufs,
                &cubin,
                KERNEL_NAME,
                (1, 1, 1),
                (N_THREADS, 1, 1),
                &input,
                output_len(),
                N_THREADS,
            )
        };
        let outcome = DiffOutcome {
            o0: run_at("-O0"),
            o3: run_at("-O3"),
        };

        if outcome.diverged() {
            stats.divergences.fetch_add(1, Ordering::Relaxed);
            if let Err(e) =
                save_divergence(&cfg.out_dir, log_lock, seed, &bytes, &ptx, &input, &outcome)
            {
                let _g = log_lock.lock().unwrap_or_else(|p| p.into_inner());
                eprintln!("worker {worker_id}: save_divergence failed: {e:#}");
            }
        } else if !outcome.matches() {
            stats.both_failed.fetch_add(1, Ordering::Relaxed);
        }

        stats.iters.fetch_add(1, Ordering::Relaxed);
    }
}

fn main() -> Result<()> {
    Args::parse().apply_env_overrides();

    // ptxas writes temp files; /dev/shm is tmpfs on this box, /tmp is real disk.
    // Only override if the caller didn't already set TMPDIR.
    if std::env::var_os("TMPDIR").is_none() {
        let shm = Path::new("/dev/shm");
        if shm.is_dir() {
            std::env::set_var("TMPDIR", shm);
        }
    }

    let cfg = Config::from_env()?;
    std::fs::create_dir_all(&cfg.out_dir)?;
    let arch = format!("-arch={TARGET_ARCH}");

    let total_workers = cfg.gpus.len() * cfg.workers_per_gpu;
    eprintln!(
        "fuzzx-diff: starting_seed=0x{:016x} out={} program_bytes={} max_iters={} control_flow={:?} blocks={}..{} insts_per_block={}..{} regs={} max_loop_iters={} max_immediate={} max_structured_depth={} emit_structured_loops={} emit_arbitrary_loops={} emit_lop3={} emit_predicated_lop3={} emit_minmax={} emit_selp={} emit_predicated_selp={} emit_sub={} emit_mul_lo={} emit_signed_lo_alu={} emit_sat_arith={} emit_packed_add={} emit_signed_packed_add={} emit_predicated_packed_add={} emit_packed_minmax={} emit_signed_packed_minmax={} emit_predicated_packed_minmax={} emit_scalar_16bit={} emit_signed_scalar_16bit={} emit_scalar_16bit_min={} emit_scalar_16bit_signed_unary={} emit_scalar_16bit_bitwise={} emit_scalar_16bit_shifts={} emit_scalar_16bit_compare={} emit_scalar_16bit_selp={} emit_predicated_scalar_16bit={} emit_mulhi={} emit_signed_mulhi={} emit_mad_hi={} emit_signed_mad_hi={} emit_bitwise_binops={} emit_or={} emit_xor={} emit_prmt={} emit_predicated_prmt={} emit_reg_prmt={} emit_predicated_reg_prmt={} emit_prmt_modes={} emit_not={} emit_clz={} emit_brev={} emit_cnot={} emit_popc={} emit_abs={} emit_special_regs={} emit_predicated_special_regs={} emit_global_loads={} emit_global_store_roundtrips={} emit_const_memory={} emit_local_memory={} emit_shared_memory={} emit_predicated_memory={} emit_vector_memory={} emit_wide_memory={} emit_memory_cache_ops={} emit_f32_arith={} emit_f32_rounding={} emit_f32_unary={} emit_f32_cvt={} emit_f32_special_math={} emit_f32_compare={} emit_f32_selp={} emit_f64_arith={} emit_f64_rounding={} emit_f64_unary={} emit_f64_cvt={} emit_f64_special_math={} emit_f64_compare={} emit_f64_selp={} emit_signed_cmp={} emit_signed_divrem={} emit_reg_divrem={} emit_predicated_reg_divrem={} emit_predicated_divrem={} emit_funnel={} emit_reg_funnel={} emit_predicated_funnel={} emit_funnel_clamp={} emit_neg={} emit_shl={} emit_shr={} emit_signed_shr={} emit_reg_shifts={} emit_predicated_shifts={} emit_predicated_reg_shifts={} emit_bfind={} emit_signed_bfind={} emit_wide_bfind={} emit_signed_wide_bfind={} emit_predicated_bfind={} emit_predicated_wide_bfind={} emit_fns={} emit_reg_fns={} emit_predicated_fns={} emit_predicated_reg_fns={} emit_bfi={} emit_bfe={} emit_bmsk={} emit_bmsk_wrap={} emit_predicated_bitfield={} emit_reg_bitfield={} emit_predicated_reg_bitfield={} emit_wide_bfe={} emit_signed_wide_bfe={} emit_wide_bfi={} emit_predicated_wide_bitfield={} emit_reg_wide_bitfield={} emit_predicated_reg_wide_bitfield={} emit_mad24={} emit_mul24={} emit_predicated_24bit={} emit_subword_wide={} emit_signed_subword_wide={} emit_predicated_subword_wide={} emit_mul_wide={} emit_mad_wide={} emit_signed_mad_wide={} emit_predicated_mul_wide={} emit_predicated_mad_wide={} emit_wide_high_result={} emit_wide_int={} emit_wide_minmax={} emit_wide_mulhi={} emit_predicated_wide_int={} emit_wide_mad64={} emit_signed_wide_mad64={} emit_predicated_wide_mad64={} emit_wide_set={} emit_predicated_wide_set={} emit_wide_setp={} emit_wide_setp_bool={} emit_wide_selp={} emit_wide_unary={} emit_predicated_wide_unary={} emit_wide_shifts={} emit_wide_reg_shifts={} emit_predicated_wide_shifts={} emit_predicated_wide_reg_shifts={} emit_wide_divrem={} emit_signed_wide_divrem={} emit_reg_wide_divrem={} emit_predicated_reg_wide_divrem={} emit_predicated_wide_divrem={} emit_wide_addc={} emit_wide_subc={} emit_predicated_wide_carry={} emit_wide_carry_chain={} emit_predicated_wide_carry_chain={} emit_addc={} emit_subc={} emit_predicated_carry={} emit_carry_chain={} emit_predicated_carry_chain={} emit_i32_boundary_immediates={} emit_dp4a={} emit_dp2a={} emit_negated_predicates={} emit_predicated_alu={} emit_predicated_unary={} emit_cvt={} emit_predicated_cvt={} emit_narrow_cvt={} emit_signed_narrow_cvt={} emit_predicated_narrow_cvt={} emit_wide_cvt={} emit_signed_wide_cvt={} emit_predicated_wide_cvt={} emit_szext={} emit_signed_szext={} emit_predicated_szext={} emit_setp_bool={} emit_setp_dual={} emit_pred_logic={} emit_predicated_mad={} emit_predicated_mad_hi={} emit_mad_carry={} emit_signed_mad_carry={} emit_predicated_mad_carry={} emit_predicated_set={} emit_sad={} emit_slct={} emit_predicated_sad={} emit_predicated_slct={} emit_predicated_dp={} emit_predicated_video={} emit_set={} emit_s32_slct={} emit_video={} emit_vsub4={} gpus={:?} workers_per_gpu={} (total={})",
        cfg.starting_seed,
        cfg.out_dir.display(),
        cfg.program_bytes,
        cfg.max_iters
            .map(|n| n.to_string())
            .unwrap_or_else(|| "∞".to_string()),
        cfg.gen_config.control_flow,
        cfg.gen_config.min_blocks,
        cfg.gen_config.max_blocks,
        cfg.gen_config.min_insts_per_block,
        cfg.gen_config.max_insts_per_block,
        cfg.gen_config.n_working_regs,
        cfg.gen_config.max_loop_iters,
        cfg.gen_config.max_immediate,
        cfg.gen_config.max_structured_depth,
        cfg.gen_config.emit_structured_loops,
        cfg.gen_config.emit_arbitrary_loops,
        cfg.gen_config.emit_lop3,
        cfg.gen_config.emit_predicated_lop3,
        cfg.gen_config.emit_minmax,
        cfg.gen_config.emit_selp,
        cfg.gen_config.emit_predicated_selp,
        cfg.gen_config.emit_sub,
        cfg.gen_config.emit_mul_lo,
        cfg.gen_config.emit_signed_lo_alu,
        cfg.gen_config.emit_sat_arith,
        cfg.gen_config.emit_packed_add,
        cfg.gen_config.emit_signed_packed_add,
        cfg.gen_config.emit_predicated_packed_add,
        cfg.gen_config.emit_packed_minmax,
        cfg.gen_config.emit_signed_packed_minmax,
        cfg.gen_config.emit_predicated_packed_minmax,
        cfg.gen_config.emit_scalar_16bit,
        cfg.gen_config.emit_signed_scalar_16bit,
        cfg.gen_config.emit_scalar_16bit_min,
        cfg.gen_config.emit_scalar_16bit_signed_unary,
        cfg.gen_config.emit_scalar_16bit_bitwise,
        cfg.gen_config.emit_scalar_16bit_shifts,
        cfg.gen_config.emit_scalar_16bit_compare,
        cfg.gen_config.emit_scalar_16bit_selp,
        cfg.gen_config.emit_predicated_scalar_16bit,
        cfg.gen_config.emit_mulhi,
        cfg.gen_config.emit_signed_mulhi,
        cfg.gen_config.emit_mad_hi,
        cfg.gen_config.emit_signed_mad_hi,
        cfg.gen_config.emit_bitwise_binops,
        cfg.gen_config.emit_or,
        cfg.gen_config.emit_xor,
        cfg.gen_config.emit_prmt,
        cfg.gen_config.emit_predicated_prmt,
        cfg.gen_config.emit_reg_prmt,
        cfg.gen_config.emit_predicated_reg_prmt,
        cfg.gen_config.emit_prmt_modes,
        cfg.gen_config.emit_not,
        cfg.gen_config.emit_clz,
        cfg.gen_config.emit_brev,
        cfg.gen_config.emit_cnot,
        cfg.gen_config.emit_popc,
        cfg.gen_config.emit_abs,
        cfg.gen_config.emit_special_regs,
        cfg.gen_config.emit_predicated_special_regs,
        cfg.gen_config.emit_global_loads,
        cfg.gen_config.emit_global_store_roundtrips,
        cfg.gen_config.emit_const_memory,
        cfg.gen_config.emit_local_memory,
        cfg.gen_config.emit_shared_memory,
        cfg.gen_config.emit_predicated_memory,
        cfg.gen_config.emit_vector_memory,
        cfg.gen_config.emit_wide_memory,
        cfg.gen_config.emit_memory_cache_ops,
        cfg.gen_config.emit_f32_arith,
        cfg.gen_config.emit_f32_rounding,
        cfg.gen_config.emit_f32_unary,
        cfg.gen_config.emit_f32_cvt,
        cfg.gen_config.emit_f32_special_math,
        cfg.gen_config.emit_f32_compare,
        cfg.gen_config.emit_f32_selp,
        cfg.gen_config.emit_f64_arith,
        cfg.gen_config.emit_f64_rounding,
        cfg.gen_config.emit_f64_unary,
        cfg.gen_config.emit_f64_cvt,
        cfg.gen_config.emit_f64_special_math,
        cfg.gen_config.emit_f64_compare,
        cfg.gen_config.emit_f64_selp,
        cfg.gen_config.emit_signed_cmp,
        cfg.gen_config.emit_signed_divrem,
        cfg.gen_config.emit_reg_divrem,
        cfg.gen_config.emit_predicated_reg_divrem,
        cfg.gen_config.emit_predicated_divrem,
        cfg.gen_config.emit_funnel,
        cfg.gen_config.emit_reg_funnel,
        cfg.gen_config.emit_predicated_funnel,
        cfg.gen_config.emit_funnel_clamp,
        cfg.gen_config.emit_neg,
        cfg.gen_config.emit_shl,
        cfg.gen_config.emit_shr,
        cfg.gen_config.emit_signed_shr,
        cfg.gen_config.emit_reg_shifts,
        cfg.gen_config.emit_predicated_shifts,
        cfg.gen_config.emit_predicated_reg_shifts,
        cfg.gen_config.emit_bfind,
        cfg.gen_config.emit_signed_bfind,
        cfg.gen_config.emit_wide_bfind,
        cfg.gen_config.emit_signed_wide_bfind,
        cfg.gen_config.emit_predicated_bfind,
        cfg.gen_config.emit_predicated_wide_bfind,
        cfg.gen_config.emit_fns,
        cfg.gen_config.emit_reg_fns,
        cfg.gen_config.emit_predicated_fns,
        cfg.gen_config.emit_predicated_reg_fns,
        cfg.gen_config.emit_bfi,
        cfg.gen_config.emit_bfe,
        cfg.gen_config.emit_bmsk,
        cfg.gen_config.emit_bmsk_wrap,
        cfg.gen_config.emit_predicated_bitfield,
        cfg.gen_config.emit_reg_bitfield,
        cfg.gen_config.emit_predicated_reg_bitfield,
        cfg.gen_config.emit_wide_bfe,
        cfg.gen_config.emit_signed_wide_bfe,
        cfg.gen_config.emit_wide_bfi,
        cfg.gen_config.emit_predicated_wide_bitfield,
        cfg.gen_config.emit_reg_wide_bitfield,
        cfg.gen_config.emit_predicated_reg_wide_bitfield,
        cfg.gen_config.emit_mad24,
        cfg.gen_config.emit_mul24,
        cfg.gen_config.emit_predicated_24bit,
        cfg.gen_config.emit_subword_wide,
        cfg.gen_config.emit_signed_subword_wide,
        cfg.gen_config.emit_predicated_subword_wide,
        cfg.gen_config.emit_mul_wide,
        cfg.gen_config.emit_mad_wide,
        cfg.gen_config.emit_signed_mad_wide,
        cfg.gen_config.emit_predicated_mul_wide,
        cfg.gen_config.emit_predicated_mad_wide,
        cfg.gen_config.emit_wide_high_result,
        cfg.gen_config.emit_wide_int,
        cfg.gen_config.emit_wide_minmax,
        cfg.gen_config.emit_wide_mulhi,
        cfg.gen_config.emit_predicated_wide_int,
        cfg.gen_config.emit_wide_mad64,
        cfg.gen_config.emit_signed_wide_mad64,
        cfg.gen_config.emit_predicated_wide_mad64,
        cfg.gen_config.emit_wide_set,
        cfg.gen_config.emit_predicated_wide_set,
        cfg.gen_config.emit_wide_setp,
        cfg.gen_config.emit_wide_setp_bool,
        cfg.gen_config.emit_wide_selp,
        cfg.gen_config.emit_wide_unary,
        cfg.gen_config.emit_predicated_wide_unary,
        cfg.gen_config.emit_wide_shifts,
        cfg.gen_config.emit_wide_reg_shifts,
        cfg.gen_config.emit_predicated_wide_shifts,
        cfg.gen_config.emit_predicated_wide_reg_shifts,
        cfg.gen_config.emit_wide_divrem,
        cfg.gen_config.emit_signed_wide_divrem,
        cfg.gen_config.emit_reg_wide_divrem,
        cfg.gen_config.emit_predicated_reg_wide_divrem,
        cfg.gen_config.emit_predicated_wide_divrem,
        cfg.gen_config.emit_wide_addc,
        cfg.gen_config.emit_wide_subc,
        cfg.gen_config.emit_predicated_wide_carry,
        cfg.gen_config.emit_wide_carry_chain,
        cfg.gen_config.emit_predicated_wide_carry_chain,
        cfg.gen_config.emit_addc,
        cfg.gen_config.emit_subc,
        cfg.gen_config.emit_predicated_carry,
        cfg.gen_config.emit_carry_chain,
        cfg.gen_config.emit_predicated_carry_chain,
        cfg.gen_config.emit_i32_boundary_immediates,
        cfg.gen_config.emit_dp4a,
        cfg.gen_config.emit_dp2a,
        cfg.gen_config.emit_negated_predicates,
        cfg.gen_config.emit_predicated_alu,
        cfg.gen_config.emit_predicated_unary,
        cfg.gen_config.emit_cvt,
        cfg.gen_config.emit_predicated_cvt,
        cfg.gen_config.emit_narrow_cvt,
        cfg.gen_config.emit_signed_narrow_cvt,
        cfg.gen_config.emit_predicated_narrow_cvt,
        cfg.gen_config.emit_wide_cvt,
        cfg.gen_config.emit_signed_wide_cvt,
        cfg.gen_config.emit_predicated_wide_cvt,
        cfg.gen_config.emit_szext,
        cfg.gen_config.emit_signed_szext,
        cfg.gen_config.emit_predicated_szext,
        cfg.gen_config.emit_setp_bool,
        cfg.gen_config.emit_setp_dual,
        cfg.gen_config.emit_pred_logic,
        cfg.gen_config.emit_predicated_mad,
        cfg.gen_config.emit_predicated_mad_hi,
        cfg.gen_config.emit_mad_carry,
        cfg.gen_config.emit_signed_mad_carry,
        cfg.gen_config.emit_predicated_mad_carry,
        cfg.gen_config.emit_predicated_set,
        cfg.gen_config.emit_sad,
        cfg.gen_config.emit_slct,
        cfg.gen_config.emit_predicated_sad,
        cfg.gen_config.emit_predicated_slct,
        cfg.gen_config.emit_predicated_dp,
        cfg.gen_config.emit_predicated_video,
        cfg.gen_config.emit_set,
        cfg.gen_config.emit_s32_slct,
        cfg.gen_config.emit_video,
        cfg.gen_config.emit_vsub4,
        cfg.gpus,
        cfg.workers_per_gpu,
        total_workers,
    );

    let stats = Arc::new(Stats {
        next_seed: AtomicU64::new(cfg.starting_seed),
        iters: AtomicU64::new(0),
        divergences: AtomicU64::new(0),
        both_failed: AtomicU64::new(0),
        skipped: AtomicU64::new(0),
    });
    let log_lock = Arc::new(Mutex::new(()));
    let cfg = Arc::new(cfg);

    let start = Instant::now();

    let mut handles = Vec::with_capacity(total_workers);
    let mut worker_id = 0usize;
    for &gpu in &cfg.gpus {
        for _ in 0..cfg.workers_per_gpu {
            let cfg_w = Arc::clone(&cfg);
            let stats_w = Arc::clone(&stats);
            let log_w = Arc::clone(&log_lock);
            let arch_w = arch.clone();
            let id = worker_id;
            handles.push(thread::spawn(move || {
                if let Err(e) = worker_loop(id, gpu, &cfg_w, &arch_w, &stats_w, &log_w) {
                    let _g = log_w.lock().unwrap_or_else(|p| p.into_inner());
                    eprintln!("worker {id} (gpu {gpu}) exited with error: {e:#}");
                }
            }));
            worker_id += 1;
        }
    }

    // Reporter loop on the main thread. Polls stats; exits when all workers
    // finish (only possible under DIV_MAX_ITERS).
    let mut last_iters: u64 = 0;
    let mut last_print = Instant::now();
    loop {
        thread::sleep(Duration::from_millis(250));
        let all_done = handles.iter().all(|h| h.is_finished());
        if last_print.elapsed() >= cfg.print_every || all_done {
            let iters = stats.iters.load(Ordering::Relaxed);
            let divergences = stats.divergences.load(Ordering::Relaxed);
            let both_failed = stats.both_failed.load(Ordering::Relaxed);
            let skipped = stats.skipped.load(Ordering::Relaxed);
            let elapsed = start.elapsed().as_secs_f64();
            let rate_total = iters as f64 / elapsed.max(1e-6);
            let rate_recent =
                (iters - last_iters) as f64 / last_print.elapsed().as_secs_f64().max(1e-6);
            let _g = log_lock.lock().unwrap_or_else(|p| p.into_inner());
            eprintln!(
                "iter {iters}  {rate_recent:.1} iter/s (avg {rate_total:.1})  divergences {divergences}  both_failed {both_failed}  skipped {skipped}  elapsed {elapsed:.0}s",
            );
            drop(_g);
            last_iters = iters;
            last_print = Instant::now();
        }
        if all_done {
            break;
        }
    }

    for h in handles {
        let _ = h.join();
    }

    let elapsed = start.elapsed().as_secs_f64();
    let iters = stats.iters.load(Ordering::Relaxed);
    eprintln!(
        "done. iter={iters} divergences={} both_failed={} skipped={} elapsed={elapsed:.1}s rate={:.1} iter/s",
        stats.divergences.load(Ordering::Relaxed),
        stats.both_failed.load(Ordering::Relaxed),
        stats.skipped.load(Ordering::Relaxed),
        iters as f64 / elapsed.max(1e-6),
    );
    Ok(())
}
