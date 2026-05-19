//! Generates PTX kernels for differential testing of ptxas.
//!
//! Kernel ABI matches `fuzzx_exec`:
//! ```text
//! .visible .entry fuzz_kernel(.param .u64 in, .param .u64 out, .param .u32 n)
//! ```
//! Each of `N_THREADS` (32) threads reads one u32 from `in[tid]`, runs the
//! generated body, and writes `N_OUTPUTS` (4) u32 results to
//! `out[tid*16 .. tid*16 + 16]`. Slices are disjoint → race-free.
//!
//! Invariants the generator enforces — so that any `-O0` vs `-O3` divergence
//! is a ptxas bug, not a generator bug:
//!
//!   * Every working `u32` register is initialized in the entry prologue.
//!   * Loops use a per-edge countdown register, decremented on each back-edge
//!     firing. Counters initialize once in entry and only decrease, so
//!     total back-edge firings ≤ sum of initial counter values.
//!   * Each thread writes only to its own disjoint output slice, private local
//!     memory, and private shared-memory slot; no atomics, no warp intrinsics,
//!     no `bar.sync`.
//!   * Variable shift counts are masked or use `.wrap` semantics. Divisors are
//!     nonzero. Floating-point inputs are sanitized to a small finite range.

use std::fmt::Write;

use arbitrary::{Result, Unstructured};

pub const KERNEL_NAME: &str = "fuzz_kernel";
pub const N_THREADS: u32 = 32;
pub const N_OUTPUTS: u32 = 4;
pub const TARGET_ARCH: &str = "sm_103";

/// Total output bytes the kernel produces (`N_THREADS * N_OUTPUTS * 4`).
pub const fn output_len() -> usize {
    (N_THREADS as usize) * (N_OUTPUTS as usize) * 4
}

/// Total input bytes the kernel reads (`N_THREADS * 4`).
pub const fn input_len() -> usize {
    (N_THREADS as usize) * 4
}

const LOCAL_MEM_BYTES: u32 = 64;
const SHARED_SLOT_BYTES: u32 = 16;
const SHARED_MEM_BYTES: u32 = N_THREADS * SHARED_SLOT_BYTES;
const CONST_MEM_BYTES: u32 = 64;
const FLOAT_INPUT_MASK: u32 = 1023;

#[derive(Debug, Clone)]
pub struct GenConfig {
    pub min_blocks: usize,
    pub max_blocks: usize,
    pub min_insts_per_block: usize,
    pub max_insts_per_block: usize,
    /// Total `u32` working regs. First `N_OUTPUTS` are the kernel's output regs.
    pub n_working_regs: u32,
    pub max_loop_iters: u32,
    pub max_immediate: u32,
    pub max_structured_depth: usize,
    pub emit_structured_loops: bool,
    pub emit_arbitrary_loops: bool,
    pub control_flow: ControlFlowMode,
    pub emit_lop3: bool,
    pub emit_predicated_lop3: bool,
    pub emit_minmax: bool,
    pub emit_selp: bool,
    pub emit_typed_selp: bool,
    pub emit_sub: bool,
    pub emit_mul_lo: bool,
    pub emit_signed_lo_alu: bool,
    pub emit_sat_arith: bool,
    pub emit_packed_add: bool,
    pub emit_signed_packed_add: bool,
    pub emit_predicated_packed_add: bool,
    pub emit_packed_minmax: bool,
    pub emit_signed_packed_minmax: bool,
    pub emit_predicated_packed_minmax: bool,
    pub emit_scalar_16bit: bool,
    pub emit_signed_scalar_16bit: bool,
    pub emit_scalar_16bit_min: bool,
    pub emit_scalar_16bit_signed_unary: bool,
    pub emit_scalar_16bit_bitwise: bool,
    pub emit_scalar_16bit_shifts: bool,
    pub emit_scalar_16bit_compare: bool,
    pub emit_scalar_16bit_selp: bool,
    pub emit_predicated_scalar_16bit: bool,
    pub emit_mulhi: bool,
    pub emit_signed_mulhi: bool,
    pub emit_mad_hi: bool,
    pub emit_signed_mad_hi: bool,
    pub emit_bitwise_binops: bool,
    pub emit_or: bool,
    pub emit_xor: bool,
    pub emit_prmt: bool,
    pub emit_predicated_prmt: bool,
    pub emit_reg_prmt: bool,
    pub emit_predicated_reg_prmt: bool,
    pub emit_prmt_modes: bool,
    pub emit_not: bool,
    pub emit_clz: bool,
    pub emit_brev: bool,
    pub emit_cnot: bool,
    pub emit_popc: bool,
    pub emit_abs: bool,
    pub emit_special_regs: bool,
    pub emit_predicated_special_regs: bool,
    pub emit_global_loads: bool,
    pub emit_uniform_global_loads: bool,
    pub emit_global_store_roundtrips: bool,
    pub emit_const_memory: bool,
    pub emit_local_memory: bool,
    pub emit_shared_memory: bool,
    pub emit_predicated_memory: bool,
    pub emit_vector_memory: bool,
    pub emit_wide_memory: bool,
    pub emit_memory_cache_ops: bool,
    pub emit_volatile_memory: bool,
    pub emit_bit_memory: bool,
    pub emit_f32_arith: bool,
    pub emit_f32_rounding: bool,
    pub emit_f32_unary: bool,
    pub emit_f32_cvt: bool,
    pub emit_f32_special_math: bool,
    pub emit_f32_compare: bool,
    pub emit_f32_selp: bool,
    pub emit_f64_arith: bool,
    pub emit_f64_rounding: bool,
    pub emit_f64_unary: bool,
    pub emit_f64_cvt: bool,
    pub emit_f64_special_math: bool,
    pub emit_f64_compare: bool,
    pub emit_f64_selp: bool,
    pub emit_signed_cmp: bool,
    pub emit_signed_divrem: bool,
    pub emit_reg_divrem: bool,
    pub emit_predicated_reg_divrem: bool,
    pub emit_funnel: bool,
    pub emit_reg_funnel: bool,
    pub emit_predicated_funnel: bool,
    pub emit_funnel_clamp: bool,
    pub emit_neg: bool,
    pub emit_shl: bool,
    pub emit_shr: bool,
    pub emit_signed_shr: bool,
    pub emit_reg_shifts: bool,
    pub emit_predicated_shifts: bool,
    pub emit_predicated_reg_shifts: bool,
    pub emit_bfind: bool,
    pub emit_signed_bfind: bool,
    pub emit_wide_bfind: bool,
    pub emit_signed_wide_bfind: bool,
    pub emit_predicated_bfind: bool,
    pub emit_predicated_wide_bfind: bool,
    pub emit_fns: bool,
    pub emit_reg_fns: bool,
    pub emit_predicated_fns: bool,
    pub emit_predicated_reg_fns: bool,
    pub emit_bfi: bool,
    pub emit_bfe: bool,
    pub emit_bmsk: bool,
    pub emit_bmsk_wrap: bool,
    pub emit_predicated_bitfield: bool,
    pub emit_reg_bitfield: bool,
    pub emit_predicated_reg_bitfield: bool,
    pub emit_wide_bfe: bool,
    pub emit_signed_wide_bfe: bool,
    pub emit_wide_bfi: bool,
    pub emit_predicated_wide_bitfield: bool,
    pub emit_reg_wide_bitfield: bool,
    pub emit_predicated_reg_wide_bitfield: bool,
    pub emit_mad24: bool,
    pub emit_mul24: bool,
    pub emit_predicated_24bit: bool,
    pub emit_subword_wide: bool,
    pub emit_signed_subword_wide: bool,
    pub emit_predicated_subword_wide: bool,
    pub emit_mul_wide: bool,
    pub emit_mad_wide: bool,
    pub emit_signed_mad_wide: bool,
    pub emit_predicated_mul_wide: bool,
    pub emit_predicated_mad_wide: bool,
    pub emit_wide_high_result: bool,
    pub emit_wide_int: bool,
    pub emit_wide_minmax: bool,
    pub emit_wide_mulhi: bool,
    pub emit_predicated_wide_int: bool,
    pub emit_wide_mad64: bool,
    pub emit_signed_wide_mad64: bool,
    pub emit_predicated_wide_mad64: bool,
    pub emit_wide_set: bool,
    pub emit_predicated_wide_set: bool,
    pub emit_wide_setp: bool,
    pub emit_wide_setp_bool: bool,
    pub emit_wide_selp: bool,
    pub emit_wide_unary: bool,
    pub emit_signed_wide_unary: bool,
    pub emit_predicated_wide_unary: bool,
    pub emit_wide_shifts: bool,
    pub emit_wide_reg_shifts: bool,
    pub emit_predicated_wide_shifts: bool,
    pub emit_predicated_wide_reg_shifts: bool,
    pub emit_wide_divrem: bool,
    pub emit_signed_wide_divrem: bool,
    pub emit_reg_wide_divrem: bool,
    pub emit_predicated_reg_wide_divrem: bool,
    pub emit_predicated_wide_divrem: bool,
    pub emit_wide_addc: bool,
    pub emit_wide_subc: bool,
    pub emit_predicated_wide_carry: bool,
    pub emit_wide_carry_chain: bool,
    pub emit_predicated_wide_carry_chain: bool,
    pub emit_addc: bool,
    pub emit_subc: bool,
    pub emit_predicated_carry: bool,
    pub emit_carry_chain: bool,
    pub emit_predicated_carry_chain: bool,
    pub emit_i32_boundary_immediates: bool,
    pub emit_dp4a: bool,
    pub emit_dp2a: bool,
    pub emit_negated_predicates: bool,
    pub emit_predicated_alu: bool,
    pub emit_predicated_unary: bool,
    pub emit_cvt: bool,
    pub emit_predicated_cvt: bool,
    pub emit_narrow_cvt: bool,
    pub emit_signed_narrow_cvt: bool,
    pub emit_predicated_narrow_cvt: bool,
    pub emit_wide_cvt: bool,
    pub emit_signed_wide_cvt: bool,
    pub emit_predicated_wide_cvt: bool,
    pub emit_szext: bool,
    pub emit_signed_szext: bool,
    pub emit_predicated_szext: bool,
    pub emit_setp_bool: bool,
    pub emit_setp_dual: bool,
    pub emit_pred_logic: bool,
    pub emit_predicated_mad: bool,
    pub emit_predicated_mad_hi: bool,
    pub emit_mad_carry: bool,
    pub emit_signed_mad_carry: bool,
    pub emit_predicated_mad_carry: bool,
    pub emit_predicated_set: bool,
    pub emit_predicated_selp: bool,
    pub emit_predicated_divrem: bool,
    pub emit_sad: bool,
    pub emit_slct: bool,
    pub emit_predicated_sad: bool,
    pub emit_predicated_slct: bool,
    pub emit_predicated_dp: bool,
    pub emit_predicated_video: bool,
    pub emit_set: bool,
    pub emit_s32_slct: bool,
    pub emit_f32_slct: bool,
    pub emit_wide_slct: bool,
    pub emit_f64_slct: bool,
    pub emit_video: bool,
    pub emit_signed_video: bool,
    pub emit_video_sat: bool,
    pub emit_vsub4: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlFlowMode {
    /// Random forward branches plus bounded backedges. This is the historical
    /// mode and can build irreducible or multi-entry-looking CFG shapes.
    Arbitrary,
    /// Directly emit single-entry structured if/else and counted-loop shapes.
    Structured,
}

impl Default for GenConfig {
    fn default() -> Self {
        Self {
            min_blocks: 1,
            max_blocks: 10,
            min_insts_per_block: 1,
            max_insts_per_block: 6,
            n_working_regs: 8,
            max_loop_iters: 16,
            max_immediate: 32,
            max_structured_depth: 3,
            emit_structured_loops: true,
            emit_arbitrary_loops: true,
            control_flow: ControlFlowMode::Arbitrary,
            emit_lop3: true,
            emit_predicated_lop3: true,
            emit_minmax: true,
            emit_selp: true,
            emit_typed_selp: true,
            emit_sub: true,
            emit_mul_lo: true,
            emit_signed_lo_alu: true,
            emit_sat_arith: true,
            emit_packed_add: true,
            emit_signed_packed_add: true,
            emit_predicated_packed_add: true,
            emit_packed_minmax: true,
            emit_signed_packed_minmax: true,
            emit_predicated_packed_minmax: true,
            emit_scalar_16bit: true,
            emit_signed_scalar_16bit: true,
            emit_scalar_16bit_min: true,
            emit_scalar_16bit_signed_unary: true,
            emit_scalar_16bit_bitwise: true,
            emit_scalar_16bit_shifts: true,
            emit_scalar_16bit_compare: true,
            emit_scalar_16bit_selp: true,
            emit_predicated_scalar_16bit: true,
            emit_mulhi: true,
            emit_signed_mulhi: true,
            emit_mad_hi: true,
            emit_signed_mad_hi: true,
            emit_bitwise_binops: true,
            emit_or: true,
            emit_xor: true,
            emit_prmt: true,
            emit_predicated_prmt: true,
            emit_reg_prmt: true,
            emit_predicated_reg_prmt: true,
            emit_prmt_modes: true,
            emit_not: true,
            emit_clz: true,
            emit_brev: true,
            emit_cnot: true,
            emit_popc: true,
            emit_abs: true,
            emit_special_regs: true,
            emit_predicated_special_regs: true,
            emit_global_loads: true,
            emit_uniform_global_loads: true,
            emit_global_store_roundtrips: true,
            emit_const_memory: true,
            emit_local_memory: true,
            emit_shared_memory: true,
            emit_predicated_memory: true,
            emit_vector_memory: true,
            emit_wide_memory: true,
            emit_memory_cache_ops: true,
            emit_volatile_memory: true,
            emit_bit_memory: true,
            emit_f32_arith: true,
            emit_f32_rounding: true,
            emit_f32_unary: true,
            emit_f32_cvt: true,
            emit_f32_special_math: true,
            emit_f32_compare: true,
            emit_f32_selp: true,
            emit_f64_arith: true,
            emit_f64_rounding: true,
            emit_f64_unary: true,
            emit_f64_cvt: true,
            emit_f64_special_math: true,
            emit_f64_compare: true,
            emit_f64_selp: true,
            emit_signed_cmp: true,
            emit_signed_divrem: true,
            emit_reg_divrem: true,
            emit_predicated_reg_divrem: true,
            emit_funnel: true,
            emit_reg_funnel: true,
            emit_predicated_funnel: true,
            emit_funnel_clamp: true,
            emit_neg: true,
            emit_shl: true,
            emit_shr: true,
            emit_signed_shr: true,
            emit_reg_shifts: true,
            emit_predicated_shifts: true,
            emit_predicated_reg_shifts: true,
            emit_bfind: true,
            emit_signed_bfind: true,
            emit_wide_bfind: true,
            emit_signed_wide_bfind: true,
            emit_predicated_bfind: true,
            emit_predicated_wide_bfind: true,
            emit_fns: true,
            emit_reg_fns: true,
            emit_predicated_fns: true,
            emit_predicated_reg_fns: true,
            emit_bfi: true,
            emit_bfe: true,
            emit_bmsk: true,
            emit_bmsk_wrap: true,
            emit_predicated_bitfield: true,
            emit_reg_bitfield: true,
            emit_predicated_reg_bitfield: true,
            emit_wide_bfe: true,
            emit_signed_wide_bfe: true,
            emit_wide_bfi: true,
            emit_predicated_wide_bitfield: true,
            emit_reg_wide_bitfield: true,
            emit_predicated_reg_wide_bitfield: true,
            emit_mad24: true,
            emit_mul24: true,
            emit_predicated_24bit: true,
            emit_subword_wide: true,
            emit_signed_subword_wide: true,
            emit_predicated_subword_wide: true,
            emit_mul_wide: true,
            emit_mad_wide: true,
            emit_signed_mad_wide: true,
            emit_predicated_mul_wide: true,
            emit_predicated_mad_wide: true,
            emit_wide_high_result: true,
            emit_wide_int: true,
            emit_wide_minmax: true,
            emit_wide_mulhi: true,
            emit_predicated_wide_int: true,
            emit_wide_mad64: true,
            emit_signed_wide_mad64: true,
            emit_predicated_wide_mad64: true,
            emit_wide_set: true,
            emit_predicated_wide_set: true,
            emit_wide_setp: true,
            emit_wide_setp_bool: true,
            emit_wide_selp: true,
            emit_wide_unary: true,
            emit_signed_wide_unary: true,
            emit_predicated_wide_unary: true,
            emit_wide_shifts: true,
            emit_wide_reg_shifts: true,
            emit_predicated_wide_shifts: true,
            emit_predicated_wide_reg_shifts: true,
            emit_wide_divrem: true,
            emit_signed_wide_divrem: true,
            emit_reg_wide_divrem: true,
            emit_predicated_reg_wide_divrem: true,
            emit_predicated_wide_divrem: true,
            emit_wide_addc: true,
            emit_wide_subc: true,
            emit_predicated_wide_carry: true,
            emit_wide_carry_chain: true,
            emit_predicated_wide_carry_chain: true,
            emit_addc: true,
            emit_subc: true,
            emit_predicated_carry: true,
            emit_carry_chain: true,
            emit_predicated_carry_chain: true,
            emit_i32_boundary_immediates: true,
            emit_dp4a: true,
            emit_dp2a: true,
            emit_negated_predicates: true,
            emit_predicated_alu: true,
            emit_predicated_unary: true,
            emit_cvt: true,
            emit_predicated_cvt: true,
            emit_narrow_cvt: true,
            emit_signed_narrow_cvt: true,
            emit_predicated_narrow_cvt: true,
            emit_wide_cvt: true,
            emit_signed_wide_cvt: true,
            emit_predicated_wide_cvt: true,
            emit_szext: true,
            emit_signed_szext: true,
            emit_predicated_szext: true,
            emit_setp_bool: true,
            emit_setp_dual: true,
            emit_pred_logic: true,
            emit_predicated_mad: true,
            emit_predicated_mad_hi: true,
            emit_mad_carry: true,
            emit_signed_mad_carry: true,
            emit_predicated_mad_carry: true,
            emit_predicated_set: true,
            emit_predicated_selp: true,
            emit_predicated_divrem: true,
            emit_sad: true,
            emit_slct: true,
            emit_predicated_sad: true,
            emit_predicated_slct: true,
            emit_predicated_dp: true,
            emit_predicated_video: true,
            emit_set: true,
            emit_s32_slct: true,
            emit_f32_slct: true,
            emit_wide_slct: true,
            emit_f64_slct: true,
            emit_video: true,
            emit_signed_video: true,
            emit_video_sat: true,
            emit_vsub4: true,
        }
    }
}

/// Generate a PTX kernel from an `Unstructured` byte source.
pub fn generate(u: &mut Unstructured, cfg: &GenConfig) -> Result<String> {
    Generator::new(cfg).build(u)
}

/// Convenience: build an `Unstructured` from raw bytes and generate.
pub fn generate_from_bytes(bytes: &[u8]) -> Result<String> {
    generate_from_bytes_with_config(bytes, &GenConfig::default())
}

/// Convenience: build an `Unstructured` from raw bytes and a caller-supplied
/// config, then generate.
pub fn generate_from_bytes_with_config(bytes: &[u8], cfg: &GenConfig) -> Result<String> {
    let mut u = Unstructured::new(bytes);
    generate(&mut u, cfg)
}

/// Deterministic byte buffer derived from a 64-bit seed; suitable as the
/// `Unstructured` source for `generate_from_bytes`. SplitMix64-style PRNG so
/// adjacent seeds produce wildly different outputs.
pub fn bytes_from_seed(seed: u64, len: usize) -> Vec<u8> {
    let mut state = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15);
    let mut out = Vec::with_capacity(len);
    for _ in 0..len {
        state ^= state >> 30;
        state = state.wrapping_mul(0xBF58_476D_1CE4_E5B9);
        out.push((state >> 16) as u8);
    }
    out
}

/// Deterministic per-thread input buffer (`input_len()` bytes), varying with
/// `seed` so different programs don't all see the same `in[tid] = tid`.
pub fn input_for_seed(seed: u64) -> Vec<u8> {
    let mut out = Vec::with_capacity(input_len());
    for tid in 0..N_THREADS {
        let v = (tid as u64)
            .wrapping_mul(0x9E37_79B9)
            .wrapping_add(seed.wrapping_mul(0xC2B2_AE35)) as u32;
        out.extend_from_slice(&v.to_ne_bytes());
    }
    out
}

// ===== Internals =====

#[derive(Clone, Copy, PartialEq, Eq)]
enum BinOp {
    Add,
    AddS,
    AddSatS,
    Sub,
    SubS,
    SubSatS,
    Mul,
    MulS,
    MulHi,
    And,
    Or,
    Xor,
    Min,
    Max,
    MulHiS,
    MinS,
    MaxS,
}

impl BinOp {
    fn mnemonic(self) -> &'static str {
        match self {
            BinOp::Add => "add.u32",
            BinOp::AddS => "add.s32",
            BinOp::AddSatS => "add.sat.s32",
            BinOp::Sub => "sub.u32",
            BinOp::SubS => "sub.s32",
            BinOp::SubSatS => "sub.sat.s32",
            BinOp::Mul => "mul.lo.u32",
            BinOp::MulS => "mul.lo.s32",
            BinOp::MulHi => "mul.hi.u32",
            BinOp::And => "and.b32",
            BinOp::Or => "or.b32",
            BinOp::Xor => "xor.b32",
            BinOp::Min => "min.u32",
            BinOp::Max => "max.u32",
            BinOp::MulHiS => "mul.hi.s32",
            BinOp::MinS => "min.s32",
            BinOp::MaxS => "max.s32",
        }
    }
}

#[derive(Clone, Copy)]
enum PackedAddOp {
    U16x2,
    S16x2,
}

impl PackedAddOp {
    fn mnemonic(self) -> &'static str {
        match self {
            PackedAddOp::U16x2 => "add.u16x2",
            PackedAddOp::S16x2 => "add.s16x2",
        }
    }
}

#[derive(Clone, Copy)]
enum PackedMinMaxOp {
    MinU16x2,
    MaxU16x2,
    MinS16x2,
    MaxS16x2,
}

impl PackedMinMaxOp {
    fn mnemonic(self) -> &'static str {
        match self {
            PackedMinMaxOp::MinU16x2 => "min.u16x2",
            PackedMinMaxOp::MaxU16x2 => "max.u16x2",
            PackedMinMaxOp::MinS16x2 => "min.s16x2",
            PackedMinMaxOp::MaxS16x2 => "max.s16x2",
        }
    }
}

#[derive(Clone, Copy)]
enum Scalar16Op {
    AddU16,
    SubU16,
    MinU16,
    MaxU16,
    MulLoU16,
    MulHiU16,
    AddS16,
    SubS16,
    MinS16,
    MaxS16,
    MulLoS16,
    MulHiS16,
    AbsS16,
    NegS16,
    AndB16,
    OrB16,
    XorB16,
    NotB16,
    ShlB16,
    ShrU16,
    ShrS16,
}

impl Scalar16Op {
    fn mnemonic(self) -> &'static str {
        match self {
            Scalar16Op::AddU16 => "add.u16",
            Scalar16Op::SubU16 => "sub.u16",
            Scalar16Op::MinU16 => "min.u16",
            Scalar16Op::MaxU16 => "max.u16",
            Scalar16Op::MulLoU16 => "mul.lo.u16",
            Scalar16Op::MulHiU16 => "mul.hi.u16",
            Scalar16Op::AddS16 => "add.s16",
            Scalar16Op::SubS16 => "sub.s16",
            Scalar16Op::MinS16 => "min.s16",
            Scalar16Op::MaxS16 => "max.s16",
            Scalar16Op::MulLoS16 => "mul.lo.s16",
            Scalar16Op::MulHiS16 => "mul.hi.s16",
            Scalar16Op::AbsS16 => "abs.s16",
            Scalar16Op::NegS16 => "neg.s16",
            Scalar16Op::AndB16 => "and.b16",
            Scalar16Op::OrB16 => "or.b16",
            Scalar16Op::XorB16 => "xor.b16",
            Scalar16Op::NotB16 => "not.b16",
            Scalar16Op::ShlB16 => "shl.b16",
            Scalar16Op::ShrU16 => "shr.u16",
            Scalar16Op::ShrS16 => "shr.s16",
        }
    }

    fn input_cvt_mnemonic(self) -> &'static str {
        if self.is_signed_input() {
            "cvt.s16.s32"
        } else {
            "cvt.u16.u32"
        }
    }

    fn output_cvt_mnemonic(self) -> &'static str {
        if self.is_signed() {
            "cvt.s32.s16"
        } else {
            "cvt.u32.u16"
        }
    }

    fn uses_h1(self) -> bool {
        !matches!(
            self,
            Scalar16Op::AbsS16
                | Scalar16Op::NegS16
                | Scalar16Op::NotB16
                | Scalar16Op::ShlB16
                | Scalar16Op::ShrU16
                | Scalar16Op::ShrS16
        )
    }

    fn is_shift(self) -> bool {
        matches!(
            self,
            Scalar16Op::ShlB16 | Scalar16Op::ShrU16 | Scalar16Op::ShrS16
        )
    }

    fn is_unary(self) -> bool {
        matches!(
            self,
            Scalar16Op::AbsS16 | Scalar16Op::NegS16 | Scalar16Op::NotB16
        )
    }

    fn is_signed(self) -> bool {
        matches!(
            self,
            Scalar16Op::AddS16
                | Scalar16Op::SubS16
                | Scalar16Op::MinS16
                | Scalar16Op::MaxS16
                | Scalar16Op::MulLoS16
                | Scalar16Op::MulHiS16
                | Scalar16Op::AbsS16
                | Scalar16Op::NegS16
                | Scalar16Op::ShrS16
        )
    }

    fn is_signed_input(self) -> bool {
        matches!(
            self,
            Scalar16Op::AddS16
                | Scalar16Op::SubS16
                | Scalar16Op::MinS16
                | Scalar16Op::MaxS16
                | Scalar16Op::MulLoS16
                | Scalar16Op::MulHiS16
                | Scalar16Op::AbsS16
                | Scalar16Op::NegS16
                | Scalar16Op::ShrS16
        )
    }
}

#[derive(Clone, Copy)]
enum GlobalLoadCacheOp {
    Default,
    Ca,
    Cg,
    Cs,
    Lu,
    Cv,
    Nc,
}

impl GlobalLoadCacheOp {
    fn prefix(self) -> &'static str {
        match self {
            GlobalLoadCacheOp::Default => "ld.global",
            GlobalLoadCacheOp::Ca => "ld.global.ca",
            GlobalLoadCacheOp::Cg => "ld.global.cg",
            GlobalLoadCacheOp::Cs => "ld.global.cs",
            GlobalLoadCacheOp::Lu => "ld.global.lu",
            GlobalLoadCacheOp::Cv => "ld.global.cv",
            GlobalLoadCacheOp::Nc => "ld.global.nc",
        }
    }
}

#[derive(Clone, Copy)]
enum GlobalLoadOp {
    U8,
    S8,
    U16,
    S16,
    U32,
    U64,
    S64,
    B8,
    B16,
    B32,
    B64,
}

impl GlobalLoadOp {
    fn mnemonic(self) -> &'static str {
        match self {
            GlobalLoadOp::U8 => "ld.global.u8",
            GlobalLoadOp::S8 => "ld.global.s8",
            GlobalLoadOp::U16 => "ld.global.u16",
            GlobalLoadOp::S16 => "ld.global.s16",
            GlobalLoadOp::U32 => "ld.global.u32",
            GlobalLoadOp::U64 => "ld.global.u64",
            GlobalLoadOp::S64 => "ld.global.s64",
            GlobalLoadOp::B8 => "ld.global.b8",
            GlobalLoadOp::B16 => "ld.global.b16",
            GlobalLoadOp::B32 => "ld.global.b32",
            GlobalLoadOp::B64 => "ld.global.b64",
        }
    }

    fn type_suffix(self) -> &'static str {
        match self {
            GlobalLoadOp::U8 => "u8",
            GlobalLoadOp::S8 => "s8",
            GlobalLoadOp::U16 => "u16",
            GlobalLoadOp::S16 => "s16",
            GlobalLoadOp::U32 => "u32",
            GlobalLoadOp::U64 => "u64",
            GlobalLoadOp::S64 => "s64",
            GlobalLoadOp::B8 => "b8",
            GlobalLoadOp::B16 => "b16",
            GlobalLoadOp::B32 => "b32",
            GlobalLoadOp::B64 => "b64",
        }
    }

    fn mnemonic_with_cache(self, cache: GlobalLoadCacheOp) -> String {
        if matches!(cache, GlobalLoadCacheOp::Default) {
            self.mnemonic().to_string()
        } else {
            format!("{}.{}", cache.prefix(), self.type_suffix())
        }
    }

    fn uniform_mnemonic(self) -> String {
        format!("ldu.global.{}", self.type_suffix())
    }

    fn volatile_mnemonic(self) -> String {
        format!("ld.volatile.global.{}", self.type_suffix())
    }

    fn width(self) -> u32 {
        match self {
            GlobalLoadOp::U8 | GlobalLoadOp::S8 | GlobalLoadOp::B8 => 1,
            GlobalLoadOp::U16 | GlobalLoadOp::S16 | GlobalLoadOp::B16 => 2,
            GlobalLoadOp::U32 | GlobalLoadOp::B32 => 4,
            GlobalLoadOp::U64 | GlobalLoadOp::S64 | GlobalLoadOp::B64 => 8,
        }
    }

    fn is_wide(self) -> bool {
        matches!(
            self,
            GlobalLoadOp::U64 | GlobalLoadOp::S64 | GlobalLoadOp::B64
        )
    }

    fn supports_uniform(self) -> bool {
        true
    }
}

#[derive(Clone, Copy)]
enum GlobalStoreCacheOp {
    Default,
    Wb,
    Cg,
    Cs,
    Wt,
}

impl GlobalStoreCacheOp {
    fn prefix(self) -> &'static str {
        match self {
            GlobalStoreCacheOp::Default => "st.global",
            GlobalStoreCacheOp::Wb => "st.global.wb",
            GlobalStoreCacheOp::Cg => "st.global.cg",
            GlobalStoreCacheOp::Cs => "st.global.cs",
            GlobalStoreCacheOp::Wt => "st.global.wt",
        }
    }
}

#[derive(Clone, Copy)]
enum GlobalStoreRoundtripOp {
    U8,
    S8,
    U16,
    S16,
    U32,
    U64,
    S64,
    B8,
    B16,
    B32,
    B64,
}

impl GlobalStoreRoundtripOp {
    fn load_mnemonic(self) -> &'static str {
        match self {
            GlobalStoreRoundtripOp::U8 => "ld.global.u8",
            GlobalStoreRoundtripOp::S8 => "ld.global.s8",
            GlobalStoreRoundtripOp::U16 => "ld.global.u16",
            GlobalStoreRoundtripOp::S16 => "ld.global.s16",
            GlobalStoreRoundtripOp::U32 => "ld.global.u32",
            GlobalStoreRoundtripOp::U64 => "ld.global.u64",
            GlobalStoreRoundtripOp::S64 => "ld.global.s64",
            GlobalStoreRoundtripOp::B8 => "ld.global.b8",
            GlobalStoreRoundtripOp::B16 => "ld.global.b16",
            GlobalStoreRoundtripOp::B32 => "ld.global.b32",
            GlobalStoreRoundtripOp::B64 => "ld.global.b64",
        }
    }

    fn store_mnemonic(self) -> &'static str {
        match self {
            GlobalStoreRoundtripOp::U8 | GlobalStoreRoundtripOp::S8 => "st.global.u8",
            GlobalStoreRoundtripOp::U16 | GlobalStoreRoundtripOp::S16 => "st.global.u16",
            GlobalStoreRoundtripOp::U32 => "st.global.u32",
            GlobalStoreRoundtripOp::U64 | GlobalStoreRoundtripOp::S64 => "st.global.u64",
            GlobalStoreRoundtripOp::B8 => "st.global.b8",
            GlobalStoreRoundtripOp::B16 => "st.global.b16",
            GlobalStoreRoundtripOp::B32 => "st.global.b32",
            GlobalStoreRoundtripOp::B64 => "st.global.b64",
        }
    }

    fn store_type_suffix(self) -> &'static str {
        match self {
            GlobalStoreRoundtripOp::U8 | GlobalStoreRoundtripOp::S8 => "u8",
            GlobalStoreRoundtripOp::U16 | GlobalStoreRoundtripOp::S16 => "u16",
            GlobalStoreRoundtripOp::U32 => "u32",
            GlobalStoreRoundtripOp::U64 | GlobalStoreRoundtripOp::S64 => "u64",
            GlobalStoreRoundtripOp::B8 => "b8",
            GlobalStoreRoundtripOp::B16 => "b16",
            GlobalStoreRoundtripOp::B32 => "b32",
            GlobalStoreRoundtripOp::B64 => "b64",
        }
    }

    fn load_type_suffix(self) -> &'static str {
        match self {
            GlobalStoreRoundtripOp::U8 => "u8",
            GlobalStoreRoundtripOp::S8 => "s8",
            GlobalStoreRoundtripOp::U16 => "u16",
            GlobalStoreRoundtripOp::S16 => "s16",
            GlobalStoreRoundtripOp::U32 => "u32",
            GlobalStoreRoundtripOp::U64 => "u64",
            GlobalStoreRoundtripOp::S64 => "s64",
            GlobalStoreRoundtripOp::B8 => "b8",
            GlobalStoreRoundtripOp::B16 => "b16",
            GlobalStoreRoundtripOp::B32 => "b32",
            GlobalStoreRoundtripOp::B64 => "b64",
        }
    }

    fn store_mnemonic_with_cache(self, cache: GlobalStoreCacheOp) -> String {
        if matches!(cache, GlobalStoreCacheOp::Default) {
            self.store_mnemonic().to_string()
        } else {
            format!("{}.{}", cache.prefix(), self.store_type_suffix())
        }
    }

    fn volatile_load_mnemonic(self) -> String {
        format!("ld.volatile.global.{}", self.load_type_suffix())
    }

    fn volatile_store_mnemonic(self) -> String {
        format!("st.volatile.global.{}", self.store_type_suffix())
    }

    fn width(self) -> u32 {
        match self {
            GlobalStoreRoundtripOp::U8
            | GlobalStoreRoundtripOp::S8
            | GlobalStoreRoundtripOp::B8 => 1,
            GlobalStoreRoundtripOp::U16
            | GlobalStoreRoundtripOp::S16
            | GlobalStoreRoundtripOp::B16 => 2,
            GlobalStoreRoundtripOp::U32 | GlobalStoreRoundtripOp::B32 => 4,
            GlobalStoreRoundtripOp::U64
            | GlobalStoreRoundtripOp::S64
            | GlobalStoreRoundtripOp::B64 => 8,
        }
    }

    fn is_wide(self) -> bool {
        matches!(
            self,
            GlobalStoreRoundtripOp::U64 | GlobalStoreRoundtripOp::S64 | GlobalStoreRoundtripOp::B64
        )
    }
}

#[derive(Clone, Copy)]
enum ConstLoadOp {
    U8,
    S8,
    U16,
    S16,
    U32,
    U64,
    S64,
    B8,
    B16,
    B32,
    B64,
}

impl ConstLoadOp {
    fn mnemonic(self) -> &'static str {
        match self {
            ConstLoadOp::U8 => "ld.const.u8",
            ConstLoadOp::S8 => "ld.const.s8",
            ConstLoadOp::U16 => "ld.const.u16",
            ConstLoadOp::S16 => "ld.const.s16",
            ConstLoadOp::U32 => "ld.const.u32",
            ConstLoadOp::U64 => "ld.const.u64",
            ConstLoadOp::S64 => "ld.const.s64",
            ConstLoadOp::B8 => "ld.const.b8",
            ConstLoadOp::B16 => "ld.const.b16",
            ConstLoadOp::B32 => "ld.const.b32",
            ConstLoadOp::B64 => "ld.const.b64",
        }
    }

    fn width(self) -> u32 {
        match self {
            ConstLoadOp::U8 | ConstLoadOp::S8 | ConstLoadOp::B8 => 1,
            ConstLoadOp::U16 | ConstLoadOp::S16 | ConstLoadOp::B16 => 2,
            ConstLoadOp::U32 | ConstLoadOp::B32 => 4,
            ConstLoadOp::U64 | ConstLoadOp::S64 | ConstLoadOp::B64 => 8,
        }
    }

    fn is_wide(self) -> bool {
        matches!(self, ConstLoadOp::U64 | ConstLoadOp::S64 | ConstLoadOp::B64)
    }
}

#[derive(Clone, Copy)]
enum LocalMemOp {
    U8,
    S8,
    U16,
    S16,
    U32,
    U64,
    S64,
    B8,
    B16,
    B32,
    B64,
}

impl LocalMemOp {
    fn load_mnemonic(self) -> &'static str {
        match self {
            LocalMemOp::U8 => "ld.local.u8",
            LocalMemOp::S8 => "ld.local.s8",
            LocalMemOp::U16 => "ld.local.u16",
            LocalMemOp::S16 => "ld.local.s16",
            LocalMemOp::U32 => "ld.local.u32",
            LocalMemOp::U64 => "ld.local.u64",
            LocalMemOp::S64 => "ld.local.s64",
            LocalMemOp::B8 => "ld.local.b8",
            LocalMemOp::B16 => "ld.local.b16",
            LocalMemOp::B32 => "ld.local.b32",
            LocalMemOp::B64 => "ld.local.b64",
        }
    }

    fn store_mnemonic(self) -> &'static str {
        match self {
            LocalMemOp::U8 | LocalMemOp::S8 => "st.local.u8",
            LocalMemOp::U16 | LocalMemOp::S16 => "st.local.u16",
            LocalMemOp::U32 => "st.local.u32",
            LocalMemOp::U64 | LocalMemOp::S64 => "st.local.u64",
            LocalMemOp::B8 => "st.local.b8",
            LocalMemOp::B16 => "st.local.b16",
            LocalMemOp::B32 => "st.local.b32",
            LocalMemOp::B64 => "st.local.b64",
        }
    }

    fn width(self) -> u32 {
        match self {
            LocalMemOp::U8 | LocalMemOp::S8 | LocalMemOp::B8 => 1,
            LocalMemOp::U16 | LocalMemOp::S16 | LocalMemOp::B16 => 2,
            LocalMemOp::U32 | LocalMemOp::B32 => 4,
            LocalMemOp::U64 | LocalMemOp::S64 | LocalMemOp::B64 => 8,
        }
    }

    fn is_wide(self) -> bool {
        matches!(self, LocalMemOp::U64 | LocalMemOp::S64 | LocalMemOp::B64)
    }
}

#[derive(Clone, Copy)]
enum SharedMemOp {
    U8,
    S8,
    U16,
    S16,
    U32,
    U64,
    S64,
    B8,
    B16,
    B32,
    B64,
}

impl SharedMemOp {
    fn load_mnemonic(self) -> &'static str {
        match self {
            SharedMemOp::U8 => "ld.shared.u8",
            SharedMemOp::S8 => "ld.shared.s8",
            SharedMemOp::U16 => "ld.shared.u16",
            SharedMemOp::S16 => "ld.shared.s16",
            SharedMemOp::U32 => "ld.shared.u32",
            SharedMemOp::U64 => "ld.shared.u64",
            SharedMemOp::S64 => "ld.shared.s64",
            SharedMemOp::B8 => "ld.shared.b8",
            SharedMemOp::B16 => "ld.shared.b16",
            SharedMemOp::B32 => "ld.shared.b32",
            SharedMemOp::B64 => "ld.shared.b64",
        }
    }

    fn load_type_suffix(self) -> &'static str {
        match self {
            SharedMemOp::U8 => "u8",
            SharedMemOp::S8 => "s8",
            SharedMemOp::U16 => "u16",
            SharedMemOp::S16 => "s16",
            SharedMemOp::U32 => "u32",
            SharedMemOp::U64 => "u64",
            SharedMemOp::S64 => "s64",
            SharedMemOp::B8 => "b8",
            SharedMemOp::B16 => "b16",
            SharedMemOp::B32 => "b32",
            SharedMemOp::B64 => "b64",
        }
    }

    fn store_mnemonic(self) -> &'static str {
        match self {
            SharedMemOp::U8 | SharedMemOp::S8 => "st.shared.u8",
            SharedMemOp::U16 | SharedMemOp::S16 => "st.shared.u16",
            SharedMemOp::U32 => "st.shared.u32",
            SharedMemOp::U64 | SharedMemOp::S64 => "st.shared.u64",
            SharedMemOp::B8 => "st.shared.b8",
            SharedMemOp::B16 => "st.shared.b16",
            SharedMemOp::B32 => "st.shared.b32",
            SharedMemOp::B64 => "st.shared.b64",
        }
    }

    fn store_type_suffix(self) -> &'static str {
        match self {
            SharedMemOp::U8 | SharedMemOp::S8 => "u8",
            SharedMemOp::U16 | SharedMemOp::S16 => "u16",
            SharedMemOp::U32 => "u32",
            SharedMemOp::U64 | SharedMemOp::S64 => "u64",
            SharedMemOp::B8 => "b8",
            SharedMemOp::B16 => "b16",
            SharedMemOp::B32 => "b32",
            SharedMemOp::B64 => "b64",
        }
    }

    fn volatile_load_mnemonic(self) -> String {
        format!("ld.volatile.shared.{}", self.load_type_suffix())
    }

    fn volatile_store_mnemonic(self) -> String {
        format!("st.volatile.shared.{}", self.store_type_suffix())
    }

    fn width(self) -> u32 {
        match self {
            SharedMemOp::U8 | SharedMemOp::S8 | SharedMemOp::B8 => 1,
            SharedMemOp::U16 | SharedMemOp::S16 | SharedMemOp::B16 => 2,
            SharedMemOp::U32 | SharedMemOp::B32 => 4,
            SharedMemOp::U64 | SharedMemOp::S64 | SharedMemOp::B64 => 8,
        }
    }

    fn is_wide(self) -> bool {
        matches!(self, SharedMemOp::U64 | SharedMemOp::S64 | SharedMemOp::B64)
    }
}

#[derive(Clone, Copy)]
enum VectorMemOp {
    V2,
    V4,
    V2U64,
    V2B32,
    V4B32,
    V2B64,
}

impl VectorMemOp {
    fn lanes(self) -> usize {
        match self {
            VectorMemOp::V2 | VectorMemOp::V2U64 | VectorMemOp::V2B32 | VectorMemOp::V2B64 => 2,
            VectorMemOp::V4 | VectorMemOp::V4B32 => 4,
        }
    }

    fn bytes(self) -> u32 {
        (self.lanes() as u32) * if self.is_wide() { 8 } else { 4 }
    }

    fn is_wide(self) -> bool {
        matches!(self, VectorMemOp::V2U64 | VectorMemOp::V2B64)
    }

    fn global_load_mnemonic(self) -> &'static str {
        match self {
            VectorMemOp::V2 => "ld.global.v2.u32",
            VectorMemOp::V4 => "ld.global.v4.u32",
            VectorMemOp::V2U64 => "ld.global.v2.u64",
            VectorMemOp::V2B32 => "ld.global.v2.b32",
            VectorMemOp::V4B32 => "ld.global.v4.b32",
            VectorMemOp::V2B64 => "ld.global.v2.b64",
        }
    }

    fn type_suffix(self) -> &'static str {
        match self {
            VectorMemOp::V2 => "v2.u32",
            VectorMemOp::V4 => "v4.u32",
            VectorMemOp::V2U64 => "v2.u64",
            VectorMemOp::V2B32 => "v2.b32",
            VectorMemOp::V4B32 => "v4.b32",
            VectorMemOp::V2B64 => "v2.b64",
        }
    }

    fn global_load_mnemonic_with_cache(self, cache: GlobalLoadCacheOp) -> String {
        if matches!(cache, GlobalLoadCacheOp::Default) {
            self.global_load_mnemonic().to_string()
        } else {
            format!("{}.{}", cache.prefix(), self.type_suffix())
        }
    }

    fn uniform_global_load_mnemonic(self) -> String {
        format!("ldu.global.{}", self.type_suffix())
    }

    fn supports_uniform_global_load(self) -> bool {
        true
    }

    fn volatile_global_load_mnemonic(self) -> String {
        format!("ld.volatile.global.{}", self.type_suffix())
    }

    fn global_store_mnemonic(self) -> &'static str {
        match self {
            VectorMemOp::V2 => "st.global.v2.u32",
            VectorMemOp::V4 => "st.global.v4.u32",
            VectorMemOp::V2U64 => "st.global.v2.u64",
            VectorMemOp::V2B32 => "st.global.v2.b32",
            VectorMemOp::V4B32 => "st.global.v4.b32",
            VectorMemOp::V2B64 => "st.global.v2.b64",
        }
    }

    fn global_store_mnemonic_with_cache(self, cache: GlobalStoreCacheOp) -> String {
        if matches!(cache, GlobalStoreCacheOp::Default) {
            self.global_store_mnemonic().to_string()
        } else {
            format!("{}.{}", cache.prefix(), self.type_suffix())
        }
    }

    fn volatile_global_store_mnemonic(self) -> String {
        format!("st.volatile.global.{}", self.type_suffix())
    }

    fn const_load_mnemonic(self) -> &'static str {
        match self {
            VectorMemOp::V2 => "ld.const.v2.u32",
            VectorMemOp::V4 => "ld.const.v4.u32",
            VectorMemOp::V2U64 => "ld.const.v2.u64",
            VectorMemOp::V2B32 => "ld.const.v2.b32",
            VectorMemOp::V4B32 => "ld.const.v4.b32",
            VectorMemOp::V2B64 => "ld.const.v2.b64",
        }
    }

    fn local_load_mnemonic(self) -> &'static str {
        match self {
            VectorMemOp::V2 => "ld.local.v2.u32",
            VectorMemOp::V4 => "ld.local.v4.u32",
            VectorMemOp::V2U64 => "ld.local.v2.u64",
            VectorMemOp::V2B32 => "ld.local.v2.b32",
            VectorMemOp::V4B32 => "ld.local.v4.b32",
            VectorMemOp::V2B64 => "ld.local.v2.b64",
        }
    }

    fn local_store_mnemonic(self) -> &'static str {
        match self {
            VectorMemOp::V2 => "st.local.v2.u32",
            VectorMemOp::V4 => "st.local.v4.u32",
            VectorMemOp::V2U64 => "st.local.v2.u64",
            VectorMemOp::V2B32 => "st.local.v2.b32",
            VectorMemOp::V4B32 => "st.local.v4.b32",
            VectorMemOp::V2B64 => "st.local.v2.b64",
        }
    }

    fn shared_load_mnemonic(self) -> &'static str {
        match self {
            VectorMemOp::V2 => "ld.shared.v2.u32",
            VectorMemOp::V4 => "ld.shared.v4.u32",
            VectorMemOp::V2U64 => "ld.shared.v2.u64",
            VectorMemOp::V2B32 => "ld.shared.v2.b32",
            VectorMemOp::V4B32 => "ld.shared.v4.b32",
            VectorMemOp::V2B64 => "ld.shared.v2.b64",
        }
    }

    fn volatile_shared_load_mnemonic(self) -> String {
        format!("ld.volatile.shared.{}", self.type_suffix())
    }

    fn shared_store_mnemonic(self) -> &'static str {
        match self {
            VectorMemOp::V2 => "st.shared.v2.u32",
            VectorMemOp::V4 => "st.shared.v4.u32",
            VectorMemOp::V2U64 => "st.shared.v2.u64",
            VectorMemOp::V2B32 => "st.shared.v2.b32",
            VectorMemOp::V4B32 => "st.shared.v4.b32",
            VectorMemOp::V2B64 => "st.shared.v2.b64",
        }
    }

    fn volatile_shared_store_mnemonic(self) -> String {
        format!("st.volatile.shared.{}", self.type_suffix())
    }
}

#[derive(Clone, Copy)]
enum F32ArithOp {
    Add,
    Sub,
    Mul,
    Div,
    DivApprox,
    Fma,
    AddSat,
    SubSat,
    MulSat,
    FmaSat,
    Copysign,
    Min,
    Max,
    MinFtz,
    MaxFtz,
}

impl F32ArithOp {
    fn mnemonic(self) -> &'static str {
        match self {
            F32ArithOp::Add => "add.rn.f32",
            F32ArithOp::Sub => "sub.rn.f32",
            F32ArithOp::Mul => "mul.rn.f32",
            F32ArithOp::Div => "div.rn.f32",
            F32ArithOp::DivApprox => "div.approx.ftz.f32",
            F32ArithOp::Fma => "fma.rn.f32",
            F32ArithOp::AddSat => "add.rn.sat.f32",
            F32ArithOp::SubSat => "sub.rn.sat.f32",
            F32ArithOp::MulSat => "mul.rn.sat.f32",
            F32ArithOp::FmaSat => "fma.rn.sat.f32",
            F32ArithOp::Copysign => "copysign.f32",
            F32ArithOp::Min => "min.f32",
            F32ArithOp::Max => "max.f32",
            F32ArithOp::MinFtz => "min.ftz.f32",
            F32ArithOp::MaxFtz => "max.ftz.f32",
        }
    }

    fn uses_c(self) -> bool {
        matches!(self, F32ArithOp::Fma | F32ArithOp::FmaSat)
    }

    fn needs_positive_b(self) -> bool {
        matches!(self, F32ArithOp::Div | F32ArithOp::DivApprox)
    }

    fn uses_arbitrary_sign_b(self) -> bool {
        matches!(self, F32ArithOp::Copysign)
    }
}

#[derive(Clone, Copy)]
enum F32RoundingArithOp {
    AddRz,
    AddRm,
    AddRp,
    AddRnFtz,
    AddRzFtz,
    AddRmFtz,
    AddRpFtz,
    SubRz,
    SubRm,
    SubRp,
    SubRnFtz,
    SubRzFtz,
    SubRmFtz,
    SubRpFtz,
    MulRz,
    MulRm,
    MulRp,
    MulRnFtz,
    MulRzFtz,
    MulRmFtz,
    MulRpFtz,
    DivRz,
    DivRm,
    DivRp,
    DivRnFtz,
    DivRzFtz,
    DivRmFtz,
    DivRpFtz,
    FmaRz,
    FmaRm,
    FmaRp,
    FmaRnFtz,
    FmaRzFtz,
    FmaRmFtz,
    FmaRpFtz,
}

impl F32RoundingArithOp {
    fn mnemonic(self) -> &'static str {
        match self {
            F32RoundingArithOp::AddRz => "add.rz.f32",
            F32RoundingArithOp::AddRm => "add.rm.f32",
            F32RoundingArithOp::AddRp => "add.rp.f32",
            F32RoundingArithOp::AddRnFtz => "add.rn.ftz.f32",
            F32RoundingArithOp::AddRzFtz => "add.rz.ftz.f32",
            F32RoundingArithOp::AddRmFtz => "add.rm.ftz.f32",
            F32RoundingArithOp::AddRpFtz => "add.rp.ftz.f32",
            F32RoundingArithOp::SubRz => "sub.rz.f32",
            F32RoundingArithOp::SubRm => "sub.rm.f32",
            F32RoundingArithOp::SubRp => "sub.rp.f32",
            F32RoundingArithOp::SubRnFtz => "sub.rn.ftz.f32",
            F32RoundingArithOp::SubRzFtz => "sub.rz.ftz.f32",
            F32RoundingArithOp::SubRmFtz => "sub.rm.ftz.f32",
            F32RoundingArithOp::SubRpFtz => "sub.rp.ftz.f32",
            F32RoundingArithOp::MulRz => "mul.rz.f32",
            F32RoundingArithOp::MulRm => "mul.rm.f32",
            F32RoundingArithOp::MulRp => "mul.rp.f32",
            F32RoundingArithOp::MulRnFtz => "mul.rn.ftz.f32",
            F32RoundingArithOp::MulRzFtz => "mul.rz.ftz.f32",
            F32RoundingArithOp::MulRmFtz => "mul.rm.ftz.f32",
            F32RoundingArithOp::MulRpFtz => "mul.rp.ftz.f32",
            F32RoundingArithOp::DivRz => "div.rz.f32",
            F32RoundingArithOp::DivRm => "div.rm.f32",
            F32RoundingArithOp::DivRp => "div.rp.f32",
            F32RoundingArithOp::DivRnFtz => "div.rn.ftz.f32",
            F32RoundingArithOp::DivRzFtz => "div.rz.ftz.f32",
            F32RoundingArithOp::DivRmFtz => "div.rm.ftz.f32",
            F32RoundingArithOp::DivRpFtz => "div.rp.ftz.f32",
            F32RoundingArithOp::FmaRz => "fma.rz.f32",
            F32RoundingArithOp::FmaRm => "fma.rm.f32",
            F32RoundingArithOp::FmaRp => "fma.rp.f32",
            F32RoundingArithOp::FmaRnFtz => "fma.rn.ftz.f32",
            F32RoundingArithOp::FmaRzFtz => "fma.rz.ftz.f32",
            F32RoundingArithOp::FmaRmFtz => "fma.rm.ftz.f32",
            F32RoundingArithOp::FmaRpFtz => "fma.rp.ftz.f32",
        }
    }

    fn uses_c(self) -> bool {
        matches!(
            self,
            F32RoundingArithOp::FmaRz
                | F32RoundingArithOp::FmaRm
                | F32RoundingArithOp::FmaRp
                | F32RoundingArithOp::FmaRnFtz
                | F32RoundingArithOp::FmaRzFtz
                | F32RoundingArithOp::FmaRmFtz
                | F32RoundingArithOp::FmaRpFtz
        )
    }

    fn needs_positive_b(self) -> bool {
        matches!(
            self,
            F32RoundingArithOp::DivRz
                | F32RoundingArithOp::DivRm
                | F32RoundingArithOp::DivRp
                | F32RoundingArithOp::DivRnFtz
                | F32RoundingArithOp::DivRzFtz
                | F32RoundingArithOp::DivRmFtz
                | F32RoundingArithOp::DivRpFtz
        )
    }
}

#[derive(Clone, Copy)]
enum F32UnaryOp {
    Abs,
    Neg,
    AbsFtz,
    NegFtz,
}

impl F32UnaryOp {
    fn mnemonic(self) -> &'static str {
        match self {
            F32UnaryOp::Abs => "abs.f32",
            F32UnaryOp::Neg => "neg.f32",
            F32UnaryOp::AbsFtz => "abs.ftz.f32",
            F32UnaryOp::NegFtz => "neg.ftz.f32",
        }
    }
}

#[derive(Clone, Copy)]
enum F32FromIntCvtOp {
    U32Rn,
    U32Rz,
    U32Rm,
    U32Rp,
    U32RnFtz,
    U32RzFtz,
    U32RmFtz,
    U32RpFtz,
    S32Rn,
    S32Rz,
    S32Rm,
    S32Rp,
    S32RnFtz,
    S32RzFtz,
    S32RmFtz,
    S32RpFtz,
    U64Rn,
    U64Rz,
    U64Rm,
    U64Rp,
    U64RnFtz,
    U64RzFtz,
    U64RmFtz,
    U64RpFtz,
    S64Rn,
    S64Rz,
    S64Rm,
    S64Rp,
    S64RnFtz,
    S64RzFtz,
    S64RmFtz,
    S64RpFtz,
}

impl F32FromIntCvtOp {
    fn mnemonic(self) -> &'static str {
        match self {
            F32FromIntCvtOp::U32Rn => "cvt.rn.f32.u32",
            F32FromIntCvtOp::U32Rz => "cvt.rz.f32.u32",
            F32FromIntCvtOp::U32Rm => "cvt.rm.f32.u32",
            F32FromIntCvtOp::U32Rp => "cvt.rp.f32.u32",
            F32FromIntCvtOp::U32RnFtz => "cvt.rn.ftz.f32.u32",
            F32FromIntCvtOp::U32RzFtz => "cvt.rz.ftz.f32.u32",
            F32FromIntCvtOp::U32RmFtz => "cvt.rm.ftz.f32.u32",
            F32FromIntCvtOp::U32RpFtz => "cvt.rp.ftz.f32.u32",
            F32FromIntCvtOp::S32Rn => "cvt.rn.f32.s32",
            F32FromIntCvtOp::S32Rz => "cvt.rz.f32.s32",
            F32FromIntCvtOp::S32Rm => "cvt.rm.f32.s32",
            F32FromIntCvtOp::S32Rp => "cvt.rp.f32.s32",
            F32FromIntCvtOp::S32RnFtz => "cvt.rn.ftz.f32.s32",
            F32FromIntCvtOp::S32RzFtz => "cvt.rz.ftz.f32.s32",
            F32FromIntCvtOp::S32RmFtz => "cvt.rm.ftz.f32.s32",
            F32FromIntCvtOp::S32RpFtz => "cvt.rp.ftz.f32.s32",
            F32FromIntCvtOp::U64Rn => "cvt.rn.f32.u64",
            F32FromIntCvtOp::U64Rz => "cvt.rz.f32.u64",
            F32FromIntCvtOp::U64Rm => "cvt.rm.f32.u64",
            F32FromIntCvtOp::U64Rp => "cvt.rp.f32.u64",
            F32FromIntCvtOp::U64RnFtz => "cvt.rn.ftz.f32.u64",
            F32FromIntCvtOp::U64RzFtz => "cvt.rz.ftz.f32.u64",
            F32FromIntCvtOp::U64RmFtz => "cvt.rm.ftz.f32.u64",
            F32FromIntCvtOp::U64RpFtz => "cvt.rp.ftz.f32.u64",
            F32FromIntCvtOp::S64Rn => "cvt.rn.f32.s64",
            F32FromIntCvtOp::S64Rz => "cvt.rz.f32.s64",
            F32FromIntCvtOp::S64Rm => "cvt.rm.f32.s64",
            F32FromIntCvtOp::S64Rp => "cvt.rp.f32.s64",
            F32FromIntCvtOp::S64RnFtz => "cvt.rn.ftz.f32.s64",
            F32FromIntCvtOp::S64RzFtz => "cvt.rz.ftz.f32.s64",
            F32FromIntCvtOp::S64RmFtz => "cvt.rm.ftz.f32.s64",
            F32FromIntCvtOp::S64RpFtz => "cvt.rp.ftz.f32.s64",
        }
    }

    fn source_is_64(self) -> bool {
        matches!(
            self,
            F32FromIntCvtOp::U64Rn
                | F32FromIntCvtOp::U64Rz
                | F32FromIntCvtOp::U64Rm
                | F32FromIntCvtOp::U64Rp
                | F32FromIntCvtOp::U64RnFtz
                | F32FromIntCvtOp::U64RzFtz
                | F32FromIntCvtOp::U64RmFtz
                | F32FromIntCvtOp::U64RpFtz
                | F32FromIntCvtOp::S64Rn
                | F32FromIntCvtOp::S64Rz
                | F32FromIntCvtOp::S64Rm
                | F32FromIntCvtOp::S64Rp
                | F32FromIntCvtOp::S64RnFtz
                | F32FromIntCvtOp::S64RzFtz
                | F32FromIntCvtOp::S64RmFtz
                | F32FromIntCvtOp::S64RpFtz
        )
    }

    fn source_extend_mnemonic(self) -> &'static str {
        match self {
            F32FromIntCvtOp::S64Rn
            | F32FromIntCvtOp::S64Rz
            | F32FromIntCvtOp::S64Rm
            | F32FromIntCvtOp::S64Rp
            | F32FromIntCvtOp::S64RnFtz
            | F32FromIntCvtOp::S64RzFtz
            | F32FromIntCvtOp::S64RmFtz
            | F32FromIntCvtOp::S64RpFtz => "cvt.s64.s32",
            _ => "cvt.u64.u32",
        }
    }
}

#[derive(Clone, Copy)]
enum F32ToIntCvtOp {
    S32Rzi,
    S32Rni,
    S32Rmi,
    S32Rpi,
    S32RziFtz,
    S32RniFtz,
    S32RmiFtz,
    S32RpiFtz,
    U32Rzi,
    U32Rni,
    U32Rmi,
    U32Rpi,
    U32RziFtz,
    U32RniFtz,
    U32RmiFtz,
    U32RpiFtz,
    S32RziSat,
    S32RniSat,
    S32RmiSat,
    S32RpiSat,
    S32RziFtzSat,
    S32RniFtzSat,
    S32RmiFtzSat,
    S32RpiFtzSat,
    U32RziSat,
    U32RniSat,
    U32RmiSat,
    U32RpiSat,
    U32RziFtzSat,
    U32RniFtzSat,
    U32RmiFtzSat,
    U32RpiFtzSat,
    S64Rzi,
    S64Rni,
    S64Rmi,
    S64Rpi,
    S64RziFtz,
    S64RniFtz,
    S64RmiFtz,
    S64RpiFtz,
    U64Rzi,
    U64Rni,
    U64Rmi,
    U64Rpi,
    U64RziFtz,
    U64RniFtz,
    U64RmiFtz,
    U64RpiFtz,
    S64RziSat,
    S64RniSat,
    S64RmiSat,
    S64RpiSat,
    S64RziFtzSat,
    S64RniFtzSat,
    S64RmiFtzSat,
    S64RpiFtzSat,
    U64RziSat,
    U64RniSat,
    U64RmiSat,
    U64RpiSat,
    U64RziFtzSat,
    U64RniFtzSat,
    U64RmiFtzSat,
    U64RpiFtzSat,
}

impl F32ToIntCvtOp {
    fn mnemonic(self) -> &'static str {
        match self {
            F32ToIntCvtOp::S32Rzi => "cvt.rzi.s32.f32",
            F32ToIntCvtOp::S32Rni => "cvt.rni.s32.f32",
            F32ToIntCvtOp::S32Rmi => "cvt.rmi.s32.f32",
            F32ToIntCvtOp::S32Rpi => "cvt.rpi.s32.f32",
            F32ToIntCvtOp::S32RziFtz => "cvt.rzi.ftz.s32.f32",
            F32ToIntCvtOp::S32RniFtz => "cvt.rni.ftz.s32.f32",
            F32ToIntCvtOp::S32RmiFtz => "cvt.rmi.ftz.s32.f32",
            F32ToIntCvtOp::S32RpiFtz => "cvt.rpi.ftz.s32.f32",
            F32ToIntCvtOp::U32Rzi => "cvt.rzi.u32.f32",
            F32ToIntCvtOp::U32Rni => "cvt.rni.u32.f32",
            F32ToIntCvtOp::U32Rmi => "cvt.rmi.u32.f32",
            F32ToIntCvtOp::U32Rpi => "cvt.rpi.u32.f32",
            F32ToIntCvtOp::U32RziFtz => "cvt.rzi.ftz.u32.f32",
            F32ToIntCvtOp::U32RniFtz => "cvt.rni.ftz.u32.f32",
            F32ToIntCvtOp::U32RmiFtz => "cvt.rmi.ftz.u32.f32",
            F32ToIntCvtOp::U32RpiFtz => "cvt.rpi.ftz.u32.f32",
            F32ToIntCvtOp::S32RziSat => "cvt.rzi.sat.s32.f32",
            F32ToIntCvtOp::S32RniSat => "cvt.rni.sat.s32.f32",
            F32ToIntCvtOp::S32RmiSat => "cvt.rmi.sat.s32.f32",
            F32ToIntCvtOp::S32RpiSat => "cvt.rpi.sat.s32.f32",
            F32ToIntCvtOp::S32RziFtzSat => "cvt.rzi.ftz.sat.s32.f32",
            F32ToIntCvtOp::S32RniFtzSat => "cvt.rni.ftz.sat.s32.f32",
            F32ToIntCvtOp::S32RmiFtzSat => "cvt.rmi.ftz.sat.s32.f32",
            F32ToIntCvtOp::S32RpiFtzSat => "cvt.rpi.ftz.sat.s32.f32",
            F32ToIntCvtOp::U32RziSat => "cvt.rzi.sat.u32.f32",
            F32ToIntCvtOp::U32RniSat => "cvt.rni.sat.u32.f32",
            F32ToIntCvtOp::U32RmiSat => "cvt.rmi.sat.u32.f32",
            F32ToIntCvtOp::U32RpiSat => "cvt.rpi.sat.u32.f32",
            F32ToIntCvtOp::U32RziFtzSat => "cvt.rzi.ftz.sat.u32.f32",
            F32ToIntCvtOp::U32RniFtzSat => "cvt.rni.ftz.sat.u32.f32",
            F32ToIntCvtOp::U32RmiFtzSat => "cvt.rmi.ftz.sat.u32.f32",
            F32ToIntCvtOp::U32RpiFtzSat => "cvt.rpi.ftz.sat.u32.f32",
            F32ToIntCvtOp::S64Rzi => "cvt.rzi.s64.f32",
            F32ToIntCvtOp::S64Rni => "cvt.rni.s64.f32",
            F32ToIntCvtOp::S64Rmi => "cvt.rmi.s64.f32",
            F32ToIntCvtOp::S64Rpi => "cvt.rpi.s64.f32",
            F32ToIntCvtOp::S64RziFtz => "cvt.rzi.ftz.s64.f32",
            F32ToIntCvtOp::S64RniFtz => "cvt.rni.ftz.s64.f32",
            F32ToIntCvtOp::S64RmiFtz => "cvt.rmi.ftz.s64.f32",
            F32ToIntCvtOp::S64RpiFtz => "cvt.rpi.ftz.s64.f32",
            F32ToIntCvtOp::U64Rzi => "cvt.rzi.u64.f32",
            F32ToIntCvtOp::U64Rni => "cvt.rni.u64.f32",
            F32ToIntCvtOp::U64Rmi => "cvt.rmi.u64.f32",
            F32ToIntCvtOp::U64Rpi => "cvt.rpi.u64.f32",
            F32ToIntCvtOp::U64RziFtz => "cvt.rzi.ftz.u64.f32",
            F32ToIntCvtOp::U64RniFtz => "cvt.rni.ftz.u64.f32",
            F32ToIntCvtOp::U64RmiFtz => "cvt.rmi.ftz.u64.f32",
            F32ToIntCvtOp::U64RpiFtz => "cvt.rpi.ftz.u64.f32",
            F32ToIntCvtOp::S64RziSat => "cvt.rzi.sat.s64.f32",
            F32ToIntCvtOp::S64RniSat => "cvt.rni.sat.s64.f32",
            F32ToIntCvtOp::S64RmiSat => "cvt.rmi.sat.s64.f32",
            F32ToIntCvtOp::S64RpiSat => "cvt.rpi.sat.s64.f32",
            F32ToIntCvtOp::S64RziFtzSat => "cvt.rzi.ftz.sat.s64.f32",
            F32ToIntCvtOp::S64RniFtzSat => "cvt.rni.ftz.sat.s64.f32",
            F32ToIntCvtOp::S64RmiFtzSat => "cvt.rmi.ftz.sat.s64.f32",
            F32ToIntCvtOp::S64RpiFtzSat => "cvt.rpi.ftz.sat.s64.f32",
            F32ToIntCvtOp::U64RziSat => "cvt.rzi.sat.u64.f32",
            F32ToIntCvtOp::U64RniSat => "cvt.rni.sat.u64.f32",
            F32ToIntCvtOp::U64RmiSat => "cvt.rmi.sat.u64.f32",
            F32ToIntCvtOp::U64RpiSat => "cvt.rpi.sat.u64.f32",
            F32ToIntCvtOp::U64RziFtzSat => "cvt.rzi.ftz.sat.u64.f32",
            F32ToIntCvtOp::U64RniFtzSat => "cvt.rni.ftz.sat.u64.f32",
            F32ToIntCvtOp::U64RmiFtzSat => "cvt.rmi.ftz.sat.u64.f32",
            F32ToIntCvtOp::U64RpiFtzSat => "cvt.rpi.ftz.sat.u64.f32",
        }
    }

    fn dest_is_64(self) -> bool {
        matches!(
            self,
            F32ToIntCvtOp::S64Rzi
                | F32ToIntCvtOp::S64Rni
                | F32ToIntCvtOp::S64Rmi
                | F32ToIntCvtOp::S64Rpi
                | F32ToIntCvtOp::S64RziFtz
                | F32ToIntCvtOp::S64RniFtz
                | F32ToIntCvtOp::S64RmiFtz
                | F32ToIntCvtOp::S64RpiFtz
                | F32ToIntCvtOp::U64Rzi
                | F32ToIntCvtOp::U64Rni
                | F32ToIntCvtOp::U64Rmi
                | F32ToIntCvtOp::U64Rpi
                | F32ToIntCvtOp::U64RziFtz
                | F32ToIntCvtOp::U64RniFtz
                | F32ToIntCvtOp::U64RmiFtz
                | F32ToIntCvtOp::U64RpiFtz
                | F32ToIntCvtOp::S64RziSat
                | F32ToIntCvtOp::S64RniSat
                | F32ToIntCvtOp::S64RmiSat
                | F32ToIntCvtOp::S64RpiSat
                | F32ToIntCvtOp::S64RziFtzSat
                | F32ToIntCvtOp::S64RniFtzSat
                | F32ToIntCvtOp::S64RmiFtzSat
                | F32ToIntCvtOp::S64RpiFtzSat
                | F32ToIntCvtOp::U64RziSat
                | F32ToIntCvtOp::U64RniSat
                | F32ToIntCvtOp::U64RmiSat
                | F32ToIntCvtOp::U64RpiSat
                | F32ToIntCvtOp::U64RziFtzSat
                | F32ToIntCvtOp::U64RniFtzSat
                | F32ToIntCvtOp::U64RmiFtzSat
                | F32ToIntCvtOp::U64RpiFtzSat
        )
    }
}

#[derive(Clone, Copy)]
enum F32FromF64CvtOp {
    Rn,
    Rz,
    Rm,
    Rp,
    RnFtz,
    RzFtz,
    RmFtz,
    RpFtz,
}

impl F32FromF64CvtOp {
    fn mnemonic(self) -> &'static str {
        match self {
            F32FromF64CvtOp::Rn => "cvt.rn.f32.f64",
            F32FromF64CvtOp::Rz => "cvt.rz.f32.f64",
            F32FromF64CvtOp::Rm => "cvt.rm.f32.f64",
            F32FromF64CvtOp::Rp => "cvt.rp.f32.f64",
            F32FromF64CvtOp::RnFtz => "cvt.rn.ftz.f32.f64",
            F32FromF64CvtOp::RzFtz => "cvt.rz.ftz.f32.f64",
            F32FromF64CvtOp::RmFtz => "cvt.rm.ftz.f32.f64",
            F32FromF64CvtOp::RpFtz => "cvt.rp.ftz.f32.f64",
        }
    }
}

#[derive(Clone, Copy)]
enum FloatInputDomain {
    NonNegative,
    Positive,
    SmallNonNegative,
}

#[derive(Clone, Copy)]
enum F32SpecialMathOp {
    SqrtRn,
    SqrtRz,
    SqrtRm,
    SqrtRp,
    SqrtRnFtz,
    SqrtRzFtz,
    SqrtRmFtz,
    SqrtRpFtz,
    RcpRn,
    RcpRz,
    RcpRm,
    RcpRp,
    RcpRnFtz,
    RcpRzFtz,
    RcpRmFtz,
    RcpRpFtz,
    RcpApprox,
    RsqrtApprox,
    Ex2Approx,
    Lg2Approx,
    SinApprox,
    CosApprox,
}

impl F32SpecialMathOp {
    fn mnemonic(self) -> &'static str {
        match self {
            F32SpecialMathOp::SqrtRn => "sqrt.rn.f32",
            F32SpecialMathOp::SqrtRz => "sqrt.rz.f32",
            F32SpecialMathOp::SqrtRm => "sqrt.rm.f32",
            F32SpecialMathOp::SqrtRp => "sqrt.rp.f32",
            F32SpecialMathOp::SqrtRnFtz => "sqrt.rn.ftz.f32",
            F32SpecialMathOp::SqrtRzFtz => "sqrt.rz.ftz.f32",
            F32SpecialMathOp::SqrtRmFtz => "sqrt.rm.ftz.f32",
            F32SpecialMathOp::SqrtRpFtz => "sqrt.rp.ftz.f32",
            F32SpecialMathOp::RcpRn => "rcp.rn.f32",
            F32SpecialMathOp::RcpRz => "rcp.rz.f32",
            F32SpecialMathOp::RcpRm => "rcp.rm.f32",
            F32SpecialMathOp::RcpRp => "rcp.rp.f32",
            F32SpecialMathOp::RcpRnFtz => "rcp.rn.ftz.f32",
            F32SpecialMathOp::RcpRzFtz => "rcp.rz.ftz.f32",
            F32SpecialMathOp::RcpRmFtz => "rcp.rm.ftz.f32",
            F32SpecialMathOp::RcpRpFtz => "rcp.rp.ftz.f32",
            F32SpecialMathOp::RcpApprox => "rcp.approx.ftz.f32",
            F32SpecialMathOp::RsqrtApprox => "rsqrt.approx.ftz.f32",
            F32SpecialMathOp::Ex2Approx => "ex2.approx.ftz.f32",
            F32SpecialMathOp::Lg2Approx => "lg2.approx.ftz.f32",
            F32SpecialMathOp::SinApprox => "sin.approx.ftz.f32",
            F32SpecialMathOp::CosApprox => "cos.approx.ftz.f32",
        }
    }

    fn input_domain(self) -> FloatInputDomain {
        match self {
            F32SpecialMathOp::SqrtRn
            | F32SpecialMathOp::SqrtRz
            | F32SpecialMathOp::SqrtRm
            | F32SpecialMathOp::SqrtRp
            | F32SpecialMathOp::SqrtRnFtz
            | F32SpecialMathOp::SqrtRzFtz
            | F32SpecialMathOp::SqrtRmFtz
            | F32SpecialMathOp::SqrtRpFtz => FloatInputDomain::NonNegative,
            F32SpecialMathOp::RcpRn
            | F32SpecialMathOp::RcpRz
            | F32SpecialMathOp::RcpRm
            | F32SpecialMathOp::RcpRp
            | F32SpecialMathOp::RcpRnFtz
            | F32SpecialMathOp::RcpRzFtz
            | F32SpecialMathOp::RcpRmFtz
            | F32SpecialMathOp::RcpRpFtz
            | F32SpecialMathOp::RcpApprox
            | F32SpecialMathOp::RsqrtApprox
            | F32SpecialMathOp::Lg2Approx => FloatInputDomain::Positive,
            F32SpecialMathOp::Ex2Approx
            | F32SpecialMathOp::SinApprox
            | F32SpecialMathOp::CosApprox => FloatInputDomain::SmallNonNegative,
        }
    }
}

#[derive(Clone, Copy)]
enum F64ArithOp {
    Add,
    Sub,
    Mul,
    Div,
    Fma,
    Copysign,
    Min,
    Max,
}

impl F64ArithOp {
    fn mnemonic(self) -> &'static str {
        match self {
            F64ArithOp::Add => "add.rn.f64",
            F64ArithOp::Sub => "sub.rn.f64",
            F64ArithOp::Mul => "mul.rn.f64",
            F64ArithOp::Div => "div.rn.f64",
            F64ArithOp::Fma => "fma.rn.f64",
            F64ArithOp::Copysign => "copysign.f64",
            F64ArithOp::Min => "min.f64",
            F64ArithOp::Max => "max.f64",
        }
    }

    fn uses_c(self) -> bool {
        matches!(self, F64ArithOp::Fma)
    }

    fn needs_positive_b(self) -> bool {
        matches!(self, F64ArithOp::Div)
    }

    fn uses_arbitrary_sign_b(self) -> bool {
        matches!(self, F64ArithOp::Copysign)
    }
}

#[derive(Clone, Copy)]
enum F64RoundingArithOp {
    AddRz,
    AddRm,
    AddRp,
    SubRz,
    SubRm,
    SubRp,
    MulRz,
    MulRm,
    MulRp,
    DivRz,
    DivRm,
    DivRp,
    FmaRz,
    FmaRm,
    FmaRp,
}

impl F64RoundingArithOp {
    fn mnemonic(self) -> &'static str {
        match self {
            F64RoundingArithOp::AddRz => "add.rz.f64",
            F64RoundingArithOp::AddRm => "add.rm.f64",
            F64RoundingArithOp::AddRp => "add.rp.f64",
            F64RoundingArithOp::SubRz => "sub.rz.f64",
            F64RoundingArithOp::SubRm => "sub.rm.f64",
            F64RoundingArithOp::SubRp => "sub.rp.f64",
            F64RoundingArithOp::MulRz => "mul.rz.f64",
            F64RoundingArithOp::MulRm => "mul.rm.f64",
            F64RoundingArithOp::MulRp => "mul.rp.f64",
            F64RoundingArithOp::DivRz => "div.rz.f64",
            F64RoundingArithOp::DivRm => "div.rm.f64",
            F64RoundingArithOp::DivRp => "div.rp.f64",
            F64RoundingArithOp::FmaRz => "fma.rz.f64",
            F64RoundingArithOp::FmaRm => "fma.rm.f64",
            F64RoundingArithOp::FmaRp => "fma.rp.f64",
        }
    }

    fn uses_c(self) -> bool {
        matches!(
            self,
            F64RoundingArithOp::FmaRz | F64RoundingArithOp::FmaRm | F64RoundingArithOp::FmaRp
        )
    }

    fn needs_positive_b(self) -> bool {
        matches!(
            self,
            F64RoundingArithOp::DivRz | F64RoundingArithOp::DivRm | F64RoundingArithOp::DivRp
        )
    }
}

#[derive(Clone, Copy)]
enum F64UnaryOp {
    Abs,
    Neg,
}

impl F64UnaryOp {
    fn mnemonic(self) -> &'static str {
        match self {
            F64UnaryOp::Abs => "abs.f64",
            F64UnaryOp::Neg => "neg.f64",
        }
    }
}

#[derive(Clone, Copy)]
enum F64FromIntCvtOp {
    U32Rn,
    U32Rz,
    U32Rm,
    U32Rp,
    S32Rn,
    S32Rz,
    S32Rm,
    S32Rp,
    U64Rn,
    U64Rz,
    U64Rm,
    U64Rp,
    S64Rn,
    S64Rz,
    S64Rm,
    S64Rp,
}

impl F64FromIntCvtOp {
    fn mnemonic(self) -> &'static str {
        match self {
            F64FromIntCvtOp::U32Rn => "cvt.rn.f64.u32",
            F64FromIntCvtOp::U32Rz => "cvt.rz.f64.u32",
            F64FromIntCvtOp::U32Rm => "cvt.rm.f64.u32",
            F64FromIntCvtOp::U32Rp => "cvt.rp.f64.u32",
            F64FromIntCvtOp::S32Rn => "cvt.rn.f64.s32",
            F64FromIntCvtOp::S32Rz => "cvt.rz.f64.s32",
            F64FromIntCvtOp::S32Rm => "cvt.rm.f64.s32",
            F64FromIntCvtOp::S32Rp => "cvt.rp.f64.s32",
            F64FromIntCvtOp::U64Rn => "cvt.rn.f64.u64",
            F64FromIntCvtOp::U64Rz => "cvt.rz.f64.u64",
            F64FromIntCvtOp::U64Rm => "cvt.rm.f64.u64",
            F64FromIntCvtOp::U64Rp => "cvt.rp.f64.u64",
            F64FromIntCvtOp::S64Rn => "cvt.rn.f64.s64",
            F64FromIntCvtOp::S64Rz => "cvt.rz.f64.s64",
            F64FromIntCvtOp::S64Rm => "cvt.rm.f64.s64",
            F64FromIntCvtOp::S64Rp => "cvt.rp.f64.s64",
        }
    }

    fn source_is_64(self) -> bool {
        matches!(
            self,
            F64FromIntCvtOp::U64Rn
                | F64FromIntCvtOp::U64Rz
                | F64FromIntCvtOp::U64Rm
                | F64FromIntCvtOp::U64Rp
                | F64FromIntCvtOp::S64Rn
                | F64FromIntCvtOp::S64Rz
                | F64FromIntCvtOp::S64Rm
                | F64FromIntCvtOp::S64Rp
        )
    }

    fn source_extend_mnemonic(self) -> &'static str {
        match self {
            F64FromIntCvtOp::S64Rn
            | F64FromIntCvtOp::S64Rz
            | F64FromIntCvtOp::S64Rm
            | F64FromIntCvtOp::S64Rp => "cvt.s64.s32",
            _ => "cvt.u64.u32",
        }
    }
}

#[derive(Clone, Copy)]
enum F64ToIntCvtOp {
    S32Rzi,
    S32Rni,
    S32Rmi,
    S32Rpi,
    U32Rzi,
    U32Rni,
    U32Rmi,
    U32Rpi,
    S32RziSat,
    S32RniSat,
    S32RmiSat,
    S32RpiSat,
    U32RziSat,
    U32RniSat,
    U32RmiSat,
    U32RpiSat,
    S64Rzi,
    S64Rni,
    S64Rmi,
    S64Rpi,
    U64Rzi,
    U64Rni,
    U64Rmi,
    U64Rpi,
    S64RziSat,
    S64RniSat,
    S64RmiSat,
    S64RpiSat,
    U64RziSat,
    U64RniSat,
    U64RmiSat,
    U64RpiSat,
}

impl F64ToIntCvtOp {
    fn mnemonic(self) -> &'static str {
        match self {
            F64ToIntCvtOp::S32Rzi => "cvt.rzi.s32.f64",
            F64ToIntCvtOp::S32Rni => "cvt.rni.s32.f64",
            F64ToIntCvtOp::S32Rmi => "cvt.rmi.s32.f64",
            F64ToIntCvtOp::S32Rpi => "cvt.rpi.s32.f64",
            F64ToIntCvtOp::U32Rzi => "cvt.rzi.u32.f64",
            F64ToIntCvtOp::U32Rni => "cvt.rni.u32.f64",
            F64ToIntCvtOp::U32Rmi => "cvt.rmi.u32.f64",
            F64ToIntCvtOp::U32Rpi => "cvt.rpi.u32.f64",
            F64ToIntCvtOp::S32RziSat => "cvt.rzi.sat.s32.f64",
            F64ToIntCvtOp::S32RniSat => "cvt.rni.sat.s32.f64",
            F64ToIntCvtOp::S32RmiSat => "cvt.rmi.sat.s32.f64",
            F64ToIntCvtOp::S32RpiSat => "cvt.rpi.sat.s32.f64",
            F64ToIntCvtOp::U32RziSat => "cvt.rzi.sat.u32.f64",
            F64ToIntCvtOp::U32RniSat => "cvt.rni.sat.u32.f64",
            F64ToIntCvtOp::U32RmiSat => "cvt.rmi.sat.u32.f64",
            F64ToIntCvtOp::U32RpiSat => "cvt.rpi.sat.u32.f64",
            F64ToIntCvtOp::S64Rzi => "cvt.rzi.s64.f64",
            F64ToIntCvtOp::S64Rni => "cvt.rni.s64.f64",
            F64ToIntCvtOp::S64Rmi => "cvt.rmi.s64.f64",
            F64ToIntCvtOp::S64Rpi => "cvt.rpi.s64.f64",
            F64ToIntCvtOp::U64Rzi => "cvt.rzi.u64.f64",
            F64ToIntCvtOp::U64Rni => "cvt.rni.u64.f64",
            F64ToIntCvtOp::U64Rmi => "cvt.rmi.u64.f64",
            F64ToIntCvtOp::U64Rpi => "cvt.rpi.u64.f64",
            F64ToIntCvtOp::S64RziSat => "cvt.rzi.sat.s64.f64",
            F64ToIntCvtOp::S64RniSat => "cvt.rni.sat.s64.f64",
            F64ToIntCvtOp::S64RmiSat => "cvt.rmi.sat.s64.f64",
            F64ToIntCvtOp::S64RpiSat => "cvt.rpi.sat.s64.f64",
            F64ToIntCvtOp::U64RziSat => "cvt.rzi.sat.u64.f64",
            F64ToIntCvtOp::U64RniSat => "cvt.rni.sat.u64.f64",
            F64ToIntCvtOp::U64RmiSat => "cvt.rmi.sat.u64.f64",
            F64ToIntCvtOp::U64RpiSat => "cvt.rpi.sat.u64.f64",
        }
    }

    fn dest_is_64(self) -> bool {
        matches!(
            self,
            F64ToIntCvtOp::S64Rzi
                | F64ToIntCvtOp::S64Rni
                | F64ToIntCvtOp::S64Rmi
                | F64ToIntCvtOp::S64Rpi
                | F64ToIntCvtOp::U64Rzi
                | F64ToIntCvtOp::U64Rni
                | F64ToIntCvtOp::U64Rmi
                | F64ToIntCvtOp::U64Rpi
                | F64ToIntCvtOp::S64RziSat
                | F64ToIntCvtOp::S64RniSat
                | F64ToIntCvtOp::S64RmiSat
                | F64ToIntCvtOp::S64RpiSat
                | F64ToIntCvtOp::U64RziSat
                | F64ToIntCvtOp::U64RniSat
                | F64ToIntCvtOp::U64RmiSat
                | F64ToIntCvtOp::U64RpiSat
        )
    }
}

#[derive(Clone, Copy)]
enum F64FromF32CvtOp {
    Default,
}

impl F64FromF32CvtOp {
    fn mnemonic(self) -> &'static str {
        match self {
            F64FromF32CvtOp::Default => "cvt.f64.f32",
        }
    }
}

#[derive(Clone, Copy)]
enum F64SpecialMathOp {
    SqrtRn,
    SqrtRz,
    SqrtRm,
    SqrtRp,
    RcpRn,
    RcpRz,
    RcpRm,
    RcpRp,
}

impl F64SpecialMathOp {
    fn mnemonic(self) -> &'static str {
        match self {
            F64SpecialMathOp::SqrtRn => "sqrt.rn.f64",
            F64SpecialMathOp::SqrtRz => "sqrt.rz.f64",
            F64SpecialMathOp::SqrtRm => "sqrt.rm.f64",
            F64SpecialMathOp::SqrtRp => "sqrt.rp.f64",
            F64SpecialMathOp::RcpRn => "rcp.rn.f64",
            F64SpecialMathOp::RcpRz => "rcp.rz.f64",
            F64SpecialMathOp::RcpRm => "rcp.rm.f64",
            F64SpecialMathOp::RcpRp => "rcp.rp.f64",
        }
    }

    fn input_domain(self) -> FloatInputDomain {
        match self {
            F64SpecialMathOp::SqrtRn
            | F64SpecialMathOp::SqrtRz
            | F64SpecialMathOp::SqrtRm
            | F64SpecialMathOp::SqrtRp => FloatInputDomain::NonNegative,
            F64SpecialMathOp::RcpRn
            | F64SpecialMathOp::RcpRz
            | F64SpecialMathOp::RcpRm
            | F64SpecialMathOp::RcpRp => FloatInputDomain::Positive,
        }
    }
}

#[derive(Clone, Copy)]
enum ShiftOp {
    Shl,
    Shr,
    ShrS,
}

impl ShiftOp {
    fn mnemonic(self) -> &'static str {
        match self {
            ShiftOp::Shl => "shl.b32",
            ShiftOp::Shr => "shr.u32",
            ShiftOp::ShrS => "shr.s32",
        }
    }
}

#[derive(Clone, Copy)]
enum UnaryOp {
    Not,
    Cnot,
    Popc,
    Clz,
    Brev,
    AbsS,
    NegS,
}

impl UnaryOp {
    fn mnemonic(self) -> &'static str {
        match self {
            UnaryOp::Not => "not.b32",
            UnaryOp::Cnot => "cnot.b32",
            UnaryOp::Popc => "popc.b32",
            UnaryOp::Clz => "clz.b32",
            UnaryOp::Brev => "brev.b32",
            // abs.s32(INT_MIN) and neg.s32(INT_MIN) are defined to return
            // INT_MIN per PTX spec — no UB on arbitrary inputs.
            UnaryOp::AbsS => "abs.s32",
            UnaryOp::NegS => "neg.s32",
        }
    }
}

#[derive(Clone, Copy)]
enum SelpOp {
    B32,
    U32,
    S32,
}

impl SelpOp {
    fn mnemonic(self) -> &'static str {
        match self {
            SelpOp::B32 => "selp.b32",
            SelpOp::U32 => "selp.u32",
            SelpOp::S32 => "selp.s32",
        }
    }
}

#[derive(Clone, Copy)]
enum Selp64Op {
    B64,
    U64,
    S64,
}

impl Selp64Op {
    fn mnemonic(self) -> &'static str {
        match self {
            Selp64Op::B64 => "selp.b64",
            Selp64Op::U64 => "selp.u64",
            Selp64Op::S64 => "selp.s64",
        }
    }
}

#[derive(Clone, Copy)]
enum SpecialRegOp {
    TidX,
    TidY,
    TidZ,
    NtidX,
    NtidY,
    NtidZ,
    CtaidX,
    CtaidY,
    CtaidZ,
    NctaidX,
    NctaidY,
    NctaidZ,
    LaneId,
    NWarpId,
    LaneMaskEq,
    LaneMaskLt,
    LaneMaskLe,
    LaneMaskGt,
    LaneMaskGe,
}

impl SpecialRegOp {
    fn reg_name(self) -> &'static str {
        match self {
            SpecialRegOp::TidX => "%tid.x",
            SpecialRegOp::TidY => "%tid.y",
            SpecialRegOp::TidZ => "%tid.z",
            SpecialRegOp::NtidX => "%ntid.x",
            SpecialRegOp::NtidY => "%ntid.y",
            SpecialRegOp::NtidZ => "%ntid.z",
            SpecialRegOp::CtaidX => "%ctaid.x",
            SpecialRegOp::CtaidY => "%ctaid.y",
            SpecialRegOp::CtaidZ => "%ctaid.z",
            SpecialRegOp::NctaidX => "%nctaid.x",
            SpecialRegOp::NctaidY => "%nctaid.y",
            SpecialRegOp::NctaidZ => "%nctaid.z",
            SpecialRegOp::LaneId => "%laneid",
            SpecialRegOp::NWarpId => "%nwarpid",
            SpecialRegOp::LaneMaskEq => "%lanemask_eq",
            SpecialRegOp::LaneMaskLt => "%lanemask_lt",
            SpecialRegOp::LaneMaskLe => "%lanemask_le",
            SpecialRegOp::LaneMaskGt => "%lanemask_gt",
            SpecialRegOp::LaneMaskGe => "%lanemask_ge",
        }
    }
}

#[derive(Clone, Copy)]
enum CvtOp {
    U8ToU32,
    U16ToU32,
    U8ToS32,
    U16ToS32,
    S8ToU32,
    S16ToU32,
    S8ToS32,
    S16ToS32,
}

impl CvtOp {
    fn mnemonic(self) -> &'static str {
        match self {
            CvtOp::U8ToU32 => "cvt.u32.u8",
            CvtOp::U16ToU32 => "cvt.u32.u16",
            CvtOp::U8ToS32 => "cvt.s32.u8",
            CvtOp::U16ToS32 => "cvt.s32.u16",
            CvtOp::S8ToU32 => "cvt.u32.s8",
            CvtOp::S16ToU32 => "cvt.u32.s16",
            CvtOp::S8ToS32 => "cvt.s32.s8",
            CvtOp::S16ToS32 => "cvt.s32.s16",
        }
    }
}

#[derive(Clone, Copy)]
enum NarrowCvtOp {
    U32ToU8,
    U32ToU16,
    S32ToS8,
    S32ToS16,
}

impl NarrowCvtOp {
    fn narrow_mnemonic(self) -> &'static str {
        match self {
            NarrowCvtOp::U32ToU8 => "cvt.u8.u32",
            NarrowCvtOp::U32ToU16 => "cvt.u16.u32",
            NarrowCvtOp::S32ToS8 => "cvt.s8.s32",
            NarrowCvtOp::S32ToS16 => "cvt.s16.s32",
        }
    }

    fn extend_mnemonic(self) -> &'static str {
        match self {
            NarrowCvtOp::U32ToU8 => "cvt.u32.u8",
            NarrowCvtOp::U32ToU16 => "cvt.u32.u16",
            NarrowCvtOp::S32ToS8 => "cvt.s32.s8",
            NarrowCvtOp::S32ToS16 => "cvt.s32.s16",
        }
    }
}

#[derive(Clone, Copy)]
enum SzextOp {
    WrapU32,
    ClampU32,
    WrapS32,
    ClampS32,
}

impl SzextOp {
    fn mnemonic(self) -> &'static str {
        match self {
            SzextOp::WrapU32 => "szext.wrap.u32",
            SzextOp::ClampU32 => "szext.clamp.u32",
            SzextOp::WrapS32 => "szext.wrap.s32",
            SzextOp::ClampS32 => "szext.clamp.s32",
        }
    }
}

#[derive(Clone, Copy)]
enum BfindOp {
    PositionU32,
    ShiftAmountU32,
    PositionS32,
    ShiftAmountS32,
    PositionU64,
    ShiftAmountU64,
    PositionS64,
    ShiftAmountS64,
}

impl BfindOp {
    fn mnemonic(self) -> &'static str {
        match self {
            BfindOp::PositionU32 => "bfind.u32",
            BfindOp::ShiftAmountU32 => "bfind.shiftamt.u32",
            BfindOp::PositionS32 => "bfind.s32",
            BfindOp::ShiftAmountS32 => "bfind.shiftamt.s32",
            BfindOp::PositionU64 => "bfind.u64",
            BfindOp::ShiftAmountU64 => "bfind.shiftamt.u64",
            BfindOp::PositionS64 => "bfind.s64",
            BfindOp::ShiftAmountS64 => "bfind.shiftamt.s64",
        }
    }

    fn is_wide(self) -> bool {
        matches!(
            self,
            BfindOp::PositionU64
                | BfindOp::ShiftAmountU64
                | BfindOp::PositionS64
                | BfindOp::ShiftAmountS64
        )
    }

    fn cvt_mnemonic(self) -> &'static str {
        match self {
            BfindOp::PositionS64 | BfindOp::ShiftAmountS64 => "cvt.s64.s32",
            _ => "cvt.u64.u32",
        }
    }
}

#[derive(Clone, Copy)]
enum BfeOp {
    U32,
    S32,
}

impl BfeOp {
    fn mnemonic(self) -> &'static str {
        match self {
            BfeOp::U32 => "bfe.u32",
            BfeOp::S32 => "bfe.s32",
        }
    }
}

#[derive(Clone, Copy)]
enum BmskMode {
    Clamp,
    Wrap,
}

#[derive(Clone, Copy)]
enum BitfieldParamSlot {
    Pos,
    Len,
}

#[derive(Clone, Copy)]
enum FnsParamSlot {
    Base,
    Offset,
}

impl BmskMode {
    fn mnemonic(self) -> &'static str {
        match self {
            BmskMode::Clamp => "bmsk.clamp.b32",
            BmskMode::Wrap => "bmsk.wrap.b32",
        }
    }
}

#[derive(Clone, Copy)]
enum PrmtMode {
    F4e,
    B4e,
    Rc8,
    Ecl,
    Ecr,
    Rc16,
}

impl PrmtMode {
    fn suffix(self) -> &'static str {
        match self {
            PrmtMode::F4e => ".f4e",
            PrmtMode::B4e => ".b4e",
            PrmtMode::Rc8 => ".rc8",
            PrmtMode::Ecl => ".ecl",
            PrmtMode::Ecr => ".ecr",
            PrmtMode::Rc16 => ".rc16",
        }
    }

    fn ctrl_mask(self) -> u32 {
        match self {
            PrmtMode::F4e | PrmtMode::B4e => 3,
            PrmtMode::Rc8 | PrmtMode::Ecl | PrmtMode::Ecr => 7,
            PrmtMode::Rc16 => 1,
        }
    }
}

#[derive(Clone, Copy)]
enum WideBfeOp {
    U64,
    S64,
}

impl WideBfeOp {
    fn mnemonic(self) -> &'static str {
        match self {
            WideBfeOp::U64 => "bfe.u64",
            WideBfeOp::S64 => "bfe.s64",
        }
    }

    fn cvt_mnemonic(self) -> &'static str {
        match self {
            WideBfeOp::U64 => "cvt.u64.u32",
            WideBfeOp::S64 => "cvt.s64.s32",
        }
    }
}

#[derive(Clone, Copy)]
enum DivRemOp {
    DivU,
    RemU,
    DivS,
    RemS,
}

impl DivRemOp {
    fn mnemonic(self) -> &'static str {
        match self {
            DivRemOp::DivU => "div.u32",
            DivRemOp::RemU => "rem.u32",
            DivRemOp::DivS => "div.s32",
            DivRemOp::RemS => "rem.s32",
        }
    }

    fn is_signed(self) -> bool {
        matches!(self, DivRemOp::DivS | DivRemOp::RemS)
    }
}

#[derive(Clone, Copy)]
enum MadHiOp {
    U32,
    S32,
}

impl MadHiOp {
    fn mnemonic(self) -> &'static str {
        match self {
            MadHiOp::U32 => "mad.hi.u32",
            MadHiOp::S32 => "mad.hi.s32",
        }
    }
}

#[derive(Clone, Copy)]
enum MadCarryOp {
    LoU32,
    HiU32,
    LoS32,
    HiS32,
}

impl MadCarryOp {
    fn mnemonic_triple(self) -> (&'static str, &'static str, &'static str) {
        match self {
            MadCarryOp::LoU32 => ("mad.lo.cc.u32", "madc.lo.cc.u32", "madc.lo.u32"),
            MadCarryOp::HiU32 => ("mad.hi.cc.u32", "madc.hi.cc.u32", "madc.hi.u32"),
            MadCarryOp::LoS32 => ("mad.lo.cc.s32", "madc.lo.cc.s32", "madc.lo.s32"),
            MadCarryOp::HiS32 => ("mad.hi.cc.s32", "madc.hi.cc.s32", "madc.hi.s32"),
        }
    }
}

#[derive(Clone, Copy)]
enum Mad24Op {
    LoU32,
    HiU32,
    LoS32,
    HiS32,
}

impl Mad24Op {
    fn mnemonic(self) -> &'static str {
        match self {
            Mad24Op::LoU32 => "mad24.lo.u32",
            Mad24Op::HiU32 => "mad24.hi.u32",
            Mad24Op::LoS32 => "mad24.lo.s32",
            Mad24Op::HiS32 => "mad24.hi.s32",
        }
    }
}

#[derive(Clone, Copy)]
enum Mul24Op {
    LoU32,
    HiU32,
    LoS32,
    HiS32,
}

impl Mul24Op {
    fn mnemonic(self) -> &'static str {
        match self {
            Mul24Op::LoU32 => "mul24.lo.u32",
            Mul24Op::HiU32 => "mul24.hi.u32",
            Mul24Op::LoS32 => "mul24.lo.s32",
            Mul24Op::HiS32 => "mul24.hi.s32",
        }
    }
}

#[derive(Clone, Copy)]
enum SubwordWideOp {
    MulU16,
    MulS16,
    MadU16,
    MadS16,
}

impl SubwordWideOp {
    fn mnemonic(self) -> &'static str {
        match self {
            SubwordWideOp::MulU16 => "mul.wide.u16",
            SubwordWideOp::MulS16 => "mul.wide.s16",
            SubwordWideOp::MadU16 => "mad.wide.u16",
            SubwordWideOp::MadS16 => "mad.wide.s16",
        }
    }

    fn cvt_mnemonic(self) -> &'static str {
        match self {
            SubwordWideOp::MulU16 | SubwordWideOp::MadU16 => "cvt.u16.u32",
            SubwordWideOp::MulS16 | SubwordWideOp::MadS16 => "cvt.s16.s32",
        }
    }

    fn is_mad(self) -> bool {
        matches!(self, SubwordWideOp::MadU16 | SubwordWideOp::MadS16)
    }
}

#[derive(Clone, Copy)]
enum MulWideOp {
    U32,
    S32,
}

impl MulWideOp {
    fn mnemonic(self) -> &'static str {
        match self {
            MulWideOp::U32 => "mul.wide.u32",
            MulWideOp::S32 => "mul.wide.s32",
        }
    }
}

#[derive(Clone, Copy)]
enum MadWideOp {
    U32,
    S32,
}

impl MadWideOp {
    fn mnemonic(self) -> &'static str {
        match self {
            MadWideOp::U32 => "mad.wide.u32",
            MadWideOp::S32 => "mad.wide.s32",
        }
    }

    fn cvt_mnemonic(self) -> &'static str {
        match self {
            MadWideOp::U32 => "cvt.u64.u32",
            MadWideOp::S32 => "cvt.s64.s32",
        }
    }
}

#[derive(Clone, Copy)]
enum WideIntOp {
    AddU64,
    SubU64,
    MulLoU64,
    MulHiU64,
    MinU64,
    MaxU64,
    AddS64,
    SubS64,
    MulLoS64,
    MulHiS64,
    MinS64,
    MaxS64,
    AndB64,
    OrB64,
    XorB64,
}

impl WideIntOp {
    fn mnemonic(self) -> &'static str {
        match self {
            WideIntOp::AddU64 => "add.u64",
            WideIntOp::SubU64 => "sub.u64",
            WideIntOp::MulLoU64 => "mul.lo.u64",
            WideIntOp::MulHiU64 => "mul.hi.u64",
            WideIntOp::MinU64 => "min.u64",
            WideIntOp::MaxU64 => "max.u64",
            WideIntOp::AddS64 => "add.s64",
            WideIntOp::SubS64 => "sub.s64",
            WideIntOp::MulLoS64 => "mul.lo.s64",
            WideIntOp::MulHiS64 => "mul.hi.s64",
            WideIntOp::MinS64 => "min.s64",
            WideIntOp::MaxS64 => "max.s64",
            WideIntOp::AndB64 => "and.b64",
            WideIntOp::OrB64 => "or.b64",
            WideIntOp::XorB64 => "xor.b64",
        }
    }

    fn cvt_mnemonic(self) -> &'static str {
        match self {
            WideIntOp::AddS64
            | WideIntOp::SubS64
            | WideIntOp::MulLoS64
            | WideIntOp::MulHiS64
            | WideIntOp::MinS64
            | WideIntOp::MaxS64 => "cvt.s64.s32",
            WideIntOp::AddU64
            | WideIntOp::SubU64
            | WideIntOp::MulLoU64
            | WideIntOp::MulHiU64
            | WideIntOp::MinU64
            | WideIntOp::MaxU64
            | WideIntOp::AndB64
            | WideIntOp::OrB64
            | WideIntOp::XorB64 => "cvt.u64.u32",
        }
    }
}

#[derive(Clone, Copy)]
enum WideCvtOp {
    U64ToU32,
    S64ToS32,
    S64ToU32,
    U64ToS32,
}

impl WideCvtOp {
    fn source_cvt_mnemonic(self) -> &'static str {
        match self {
            WideCvtOp::U64ToU32 | WideCvtOp::U64ToS32 => "cvt.u64.u32",
            WideCvtOp::S64ToS32 | WideCvtOp::S64ToU32 => "cvt.s64.s32",
        }
    }

    fn mnemonic(self) -> &'static str {
        match self {
            WideCvtOp::U64ToU32 => "cvt.u32.u64",
            WideCvtOp::S64ToS32 => "cvt.s32.s64",
            WideCvtOp::S64ToU32 => "cvt.u32.s64",
            WideCvtOp::U64ToS32 => "cvt.s32.u64",
        }
    }
}

#[derive(Clone, Copy)]
enum WideMad64Op {
    LoU64,
    HiU64,
    LoS64,
    HiS64,
}

impl WideMad64Op {
    fn mnemonic(self) -> &'static str {
        match self {
            WideMad64Op::LoU64 => "mad.lo.u64",
            WideMad64Op::HiU64 => "mad.hi.u64",
            WideMad64Op::LoS64 => "mad.lo.s64",
            WideMad64Op::HiS64 => "mad.hi.s64",
        }
    }

    fn cvt_mnemonic(self) -> &'static str {
        match self {
            WideMad64Op::LoU64 | WideMad64Op::HiU64 => "cvt.u64.u32",
            WideMad64Op::LoS64 | WideMad64Op::HiS64 => "cvt.s64.s32",
        }
    }
}

#[derive(Clone, Copy)]
enum WideShiftOp {
    ShlB64,
    ShrU64,
    ShrS64,
}

impl WideShiftOp {
    fn mnemonic(self) -> &'static str {
        match self {
            WideShiftOp::ShlB64 => "shl.b64",
            WideShiftOp::ShrU64 => "shr.u64",
            WideShiftOp::ShrS64 => "shr.s64",
        }
    }

    fn cvt_mnemonic(self) -> &'static str {
        match self {
            WideShiftOp::ShrS64 => "cvt.s64.s32",
            WideShiftOp::ShlB64 | WideShiftOp::ShrU64 => "cvt.u64.u32",
        }
    }
}

#[derive(Clone, Copy)]
enum WideUnaryOp {
    NotB64,
    CnotB64,
    PopcB64,
    ClzB64,
    BrevB64,
    NegS64,
    AbsS64,
}

impl WideUnaryOp {
    fn mnemonic(self) -> &'static str {
        match self {
            WideUnaryOp::NotB64 => "not.b64",
            WideUnaryOp::CnotB64 => "cnot.b64",
            WideUnaryOp::PopcB64 => "popc.b64",
            WideUnaryOp::ClzB64 => "clz.b64",
            WideUnaryOp::BrevB64 => "brev.b64",
            WideUnaryOp::NegS64 => "neg.s64",
            WideUnaryOp::AbsS64 => "abs.s64",
        }
    }

    fn cvt_mnemonic(self) -> &'static str {
        match self {
            WideUnaryOp::NegS64 | WideUnaryOp::AbsS64 => "cvt.s64.s32",
            WideUnaryOp::NotB64
            | WideUnaryOp::CnotB64
            | WideUnaryOp::PopcB64
            | WideUnaryOp::ClzB64
            | WideUnaryOp::BrevB64 => "cvt.u64.u32",
        }
    }

    fn writes_b64(self) -> bool {
        matches!(
            self,
            WideUnaryOp::NotB64
                | WideUnaryOp::CnotB64
                | WideUnaryOp::BrevB64
                | WideUnaryOp::NegS64
                | WideUnaryOp::AbsS64
        )
    }
}

#[derive(Clone, Copy)]
enum WideDivRemOp {
    DivU64,
    RemU64,
    DivS64,
    RemS64,
}

impl WideDivRemOp {
    fn mnemonic(self) -> &'static str {
        match self {
            WideDivRemOp::DivU64 => "div.u64",
            WideDivRemOp::RemU64 => "rem.u64",
            WideDivRemOp::DivS64 => "div.s64",
            WideDivRemOp::RemS64 => "rem.s64",
        }
    }

    fn cvt_mnemonic(self) -> &'static str {
        match self {
            WideDivRemOp::DivS64 | WideDivRemOp::RemS64 => "cvt.s64.s32",
            WideDivRemOp::DivU64 | WideDivRemOp::RemU64 => "cvt.u64.u32",
        }
    }

    fn is_signed(self) -> bool {
        matches!(self, WideDivRemOp::DivS64 | WideDivRemOp::RemS64)
    }
}

#[derive(Clone, Copy)]
enum WideDivisor {
    Imm(i64),
    Reg(Operand),
}

#[derive(Clone, Copy)]
enum AddCarryOp {
    Add,
    Sub,
}

impl AddCarryOp {
    fn mnemonic_pair(self) -> (&'static str, &'static str) {
        match self {
            AddCarryOp::Add => ("add.cc.u32", "addc.u32"),
            AddCarryOp::Sub => ("sub.cc.u32", "subc.u32"),
        }
    }

    fn wide_mnemonic_pair(self) -> (&'static str, &'static str) {
        match self {
            AddCarryOp::Add => ("add.cc.u64", "addc.u64"),
            AddCarryOp::Sub => ("sub.cc.u64", "subc.u64"),
        }
    }

    fn mnemonic_triple(self) -> (&'static str, &'static str, &'static str) {
        match self {
            AddCarryOp::Add => ("add.cc.u32", "addc.cc.u32", "addc.u32"),
            AddCarryOp::Sub => ("sub.cc.u32", "subc.cc.u32", "subc.u32"),
        }
    }

    fn wide_mnemonic_triple(self) -> (&'static str, &'static str, &'static str) {
        match self {
            AddCarryOp::Add => ("add.cc.u64", "addc.cc.u64", "addc.u64"),
            AddCarryOp::Sub => ("sub.cc.u64", "subc.cc.u64", "subc.u64"),
        }
    }
}

#[derive(Clone, Copy)]
enum Dp4aOp {
    U32U32,
    U32S32,
    S32U32,
    S32S32,
}

impl Dp4aOp {
    fn mnemonic(self) -> &'static str {
        match self {
            Dp4aOp::U32U32 => "dp4a.u32.u32",
            Dp4aOp::U32S32 => "dp4a.u32.s32",
            Dp4aOp::S32U32 => "dp4a.s32.u32",
            Dp4aOp::S32S32 => "dp4a.s32.s32",
        }
    }
}

#[derive(Clone, Copy)]
enum Dp2aOp {
    LoU32U32,
    HiU32U32,
    LoU32S32,
    HiU32S32,
    LoS32U32,
    HiS32U32,
    LoS32S32,
    HiS32S32,
}

impl Dp2aOp {
    fn mnemonic(self) -> &'static str {
        match self {
            Dp2aOp::LoU32U32 => "dp2a.lo.u32.u32",
            Dp2aOp::HiU32U32 => "dp2a.hi.u32.u32",
            Dp2aOp::LoU32S32 => "dp2a.lo.u32.s32",
            Dp2aOp::HiU32S32 => "dp2a.hi.u32.s32",
            Dp2aOp::LoS32U32 => "dp2a.lo.s32.u32",
            Dp2aOp::HiS32U32 => "dp2a.hi.s32.u32",
            Dp2aOp::LoS32S32 => "dp2a.lo.s32.s32",
            Dp2aOp::HiS32S32 => "dp2a.hi.s32.s32",
        }
    }
}

#[derive(Clone, Copy)]
enum SadOp {
    U32,
    S32,
}

impl SadOp {
    fn mnemonic(self) -> &'static str {
        match self {
            SadOp::U32 => "sad.u32",
            SadOp::S32 => "sad.s32",
        }
    }
}

#[derive(Clone, Copy)]
enum SlctOp {
    U32S32,
    S32S32,
    B32S32,
    U32F32,
    S32F32,
    B32F32,
    F32S32,
    F32F32,
    U64S32,
    S64S32,
    B64S32,
    U64F32,
    S64F32,
    B64F32,
    F64S32,
    F64F32,
}

impl SlctOp {
    fn mnemonic(self) -> &'static str {
        match self {
            SlctOp::U32S32 => "slct.u32.s32",
            SlctOp::S32S32 => "slct.s32.s32",
            SlctOp::B32S32 => "slct.b32.s32",
            SlctOp::U32F32 => "slct.u32.f32",
            SlctOp::S32F32 => "slct.s32.f32",
            SlctOp::B32F32 => "slct.b32.f32",
            SlctOp::F32S32 => "slct.f32.s32",
            SlctOp::F32F32 => "slct.f32.f32",
            SlctOp::U64S32 => "slct.u64.s32",
            SlctOp::S64S32 => "slct.s64.s32",
            SlctOp::B64S32 => "slct.b64.s32",
            SlctOp::U64F32 => "slct.u64.f32",
            SlctOp::S64F32 => "slct.s64.f32",
            SlctOp::B64F32 => "slct.b64.f32",
            SlctOp::F64S32 => "slct.f64.s32",
            SlctOp::F64F32 => "slct.f64.f32",
        }
    }

    fn selector_is_f32(self) -> bool {
        matches!(
            self,
            SlctOp::U32F32
                | SlctOp::S32F32
                | SlctOp::B32F32
                | SlctOp::F32F32
                | SlctOp::U64F32
                | SlctOp::S64F32
                | SlctOp::B64F32
                | SlctOp::F64F32
        )
    }

    fn dst_is_f32(self) -> bool {
        matches!(self, SlctOp::F32S32 | SlctOp::F32F32)
    }

    fn dst_is_wide(self) -> bool {
        matches!(
            self,
            SlctOp::U64S32
                | SlctOp::S64S32
                | SlctOp::B64S32
                | SlctOp::U64F32
                | SlctOp::S64F32
                | SlctOp::B64F32
        )
    }

    fn dst_is_f64(self) -> bool {
        matches!(self, SlctOp::F64S32 | SlctOp::F64F32)
    }

    fn wide_input_is_signed(self) -> bool {
        matches!(self, SlctOp::S64S32 | SlctOp::S64F32)
    }
}

#[derive(Clone, Copy)]
enum VideoKind {
    Add2,
    Sub2,
    Avrg2,
    AbsDiff2,
    Min2,
    Max2,
    Add4,
    Sub4,
    Avrg4,
    AbsDiff4,
    Min4,
    Max4,
}

impl VideoKind {
    fn base(self) -> &'static str {
        match self {
            VideoKind::Add2 => "vadd2",
            VideoKind::Sub2 => "vsub2",
            VideoKind::Avrg2 => "vavrg2",
            VideoKind::AbsDiff2 => "vabsdiff2",
            VideoKind::Min2 => "vmin2",
            VideoKind::Max2 => "vmax2",
            VideoKind::Add4 => "vadd4",
            VideoKind::Sub4 => "vsub4",
            VideoKind::Avrg4 => "vavrg4",
            VideoKind::AbsDiff4 => "vabsdiff4",
            VideoKind::Min4 => "vmin4",
            VideoKind::Max4 => "vmax4",
        }
    }
}

#[derive(Clone, Copy)]
enum VideoType {
    U32,
    S32,
}

impl VideoType {
    fn suffix(self) -> &'static str {
        match self {
            VideoType::U32 => "u32",
            VideoType::S32 => "s32",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum VideoMode {
    Plain,
    Add,
    Sat,
}

impl VideoMode {
    fn suffix(self) -> &'static str {
        match self {
            VideoMode::Plain => "",
            VideoMode::Add => ".add",
            VideoMode::Sat => ".sat",
        }
    }
}

#[derive(Clone, Copy)]
struct VideoOp {
    kind: VideoKind,
    dst_type: VideoType,
    a_type: VideoType,
    b_type: VideoType,
    mode: VideoMode,
}

impl VideoOp {
    fn mnemonic(self) -> String {
        format!(
            "{}.{}.{}.{}{}",
            self.kind.base(),
            self.dst_type.suffix(),
            self.a_type.suffix(),
            self.b_type.suffix(),
            self.mode.suffix()
        )
    }
}

#[derive(Clone, Copy)]
enum CmpOp {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    // Signed compares; setp.{eq,ne} are bit-identical signed/unsigned.
    LtS,
    LeS,
    GtS,
    GeS,
}

#[derive(Clone, Copy)]
enum FloatCmpOp {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    Equ,
    Neu,
    Ltu,
    Leu,
    Gtu,
    Geu,
    Num,
    Nan,
}

impl FloatCmpOp {
    fn suffix(self) -> &'static str {
        match self {
            FloatCmpOp::Eq => "eq",
            FloatCmpOp::Ne => "ne",
            FloatCmpOp::Lt => "lt",
            FloatCmpOp::Le => "le",
            FloatCmpOp::Gt => "gt",
            FloatCmpOp::Ge => "ge",
            FloatCmpOp::Equ => "equ",
            FloatCmpOp::Neu => "neu",
            FloatCmpOp::Ltu => "ltu",
            FloatCmpOp::Leu => "leu",
            FloatCmpOp::Gtu => "gtu",
            FloatCmpOp::Geu => "geu",
            FloatCmpOp::Num => "num",
            FloatCmpOp::Nan => "nan",
        }
    }

    fn f64_set_mnemonic(self) -> String {
        format!("set.{}.u32.f64", self.suffix())
    }

    fn f64_setp_mnemonic(self) -> String {
        format!("setp.{}.f64", self.suffix())
    }

    fn f64_setp_bool_mnemonic(self, op: PredicateBoolOp) -> String {
        format!("setp.{}.{}.f64", self.suffix(), op.suffix())
    }
}

#[derive(Clone, Copy)]
struct F32CmpOp {
    cmp: FloatCmpOp,
    ftz: bool,
}

impl F32CmpOp {
    fn suffix(self) -> String {
        if self.ftz {
            format!("{}.ftz", self.cmp.suffix())
        } else {
            self.cmp.suffix().to_string()
        }
    }

    fn set_mnemonic(self) -> String {
        format!("set.{}.u32.f32", self.suffix())
    }

    fn setp_mnemonic(self) -> String {
        format!("setp.{}.f32", self.suffix())
    }

    fn setp_bool_mnemonic(self, op: PredicateBoolOp) -> String {
        format!("setp.{}.{}.f32", self.suffix(), op.suffix())
    }
}

#[derive(Clone, Copy)]
enum FloatTestpOp {
    Finite,
    Infinite,
    Number,
    NotANumber,
    Normal,
    Subnormal,
}

impl FloatTestpOp {
    fn suffix(self) -> &'static str {
        match self {
            FloatTestpOp::Finite => "finite",
            FloatTestpOp::Infinite => "infinite",
            FloatTestpOp::Number => "number",
            FloatTestpOp::NotANumber => "notanumber",
            FloatTestpOp::Normal => "normal",
            FloatTestpOp::Subnormal => "subnormal",
        }
    }

    fn f32_mnemonic(self) -> String {
        format!("testp.{}.f32", self.suffix())
    }

    fn f64_mnemonic(self) -> String {
        format!("testp.{}.f64", self.suffix())
    }
}

impl CmpOp {
    fn mnemonic(self) -> &'static str {
        match self {
            CmpOp::Eq => "setp.eq.u32",
            CmpOp::Ne => "setp.ne.u32",
            CmpOp::Lt => "setp.lt.u32",
            CmpOp::Le => "setp.le.u32",
            CmpOp::Gt => "setp.gt.u32",
            CmpOp::Ge => "setp.ge.u32",
            CmpOp::LtS => "setp.lt.s32",
            CmpOp::LeS => "setp.le.s32",
            CmpOp::GtS => "setp.gt.s32",
            CmpOp::GeS => "setp.ge.s32",
        }
    }

    fn set_mnemonic(self) -> &'static str {
        match self {
            CmpOp::Eq => "set.eq.u32.u32",
            CmpOp::Ne => "set.ne.u32.u32",
            CmpOp::Lt => "set.lt.u32.u32",
            CmpOp::Le => "set.le.u32.u32",
            CmpOp::Gt => "set.gt.u32.u32",
            CmpOp::Ge => "set.ge.u32.u32",
            CmpOp::LtS => "set.lt.u32.s32",
            CmpOp::LeS => "set.le.u32.s32",
            CmpOp::GtS => "set.gt.u32.s32",
            CmpOp::GeS => "set.ge.u32.s32",
        }
    }

    fn scalar16_input_cvt_mnemonic(self) -> &'static str {
        match self {
            CmpOp::LtS | CmpOp::LeS | CmpOp::GtS | CmpOp::GeS => "cvt.s16.s32",
            CmpOp::Eq | CmpOp::Ne | CmpOp::Lt | CmpOp::Le | CmpOp::Gt | CmpOp::Ge => "cvt.u16.u32",
        }
    }

    fn scalar16_output_cvt_mnemonic(self) -> &'static str {
        match self {
            CmpOp::LtS | CmpOp::LeS | CmpOp::GtS | CmpOp::GeS => "cvt.s32.s16",
            CmpOp::Eq | CmpOp::Ne | CmpOp::Lt | CmpOp::Le | CmpOp::Gt | CmpOp::Ge => "cvt.u32.u16",
        }
    }

    fn scalar16_setp_mnemonic(self) -> &'static str {
        match self {
            CmpOp::Eq => "setp.eq.u16",
            CmpOp::Ne => "setp.ne.u16",
            CmpOp::Lt => "setp.lt.u16",
            CmpOp::Le => "setp.le.u16",
            CmpOp::Gt => "setp.gt.u16",
            CmpOp::Ge => "setp.ge.u16",
            CmpOp::LtS => "setp.lt.s16",
            CmpOp::LeS => "setp.le.s16",
            CmpOp::GtS => "setp.gt.s16",
            CmpOp::GeS => "setp.ge.s16",
        }
    }

    fn scalar16_set_mnemonic(self) -> &'static str {
        match self {
            CmpOp::Eq => "set.eq.u32.u16",
            CmpOp::Ne => "set.ne.u32.u16",
            CmpOp::Lt => "set.lt.u32.u16",
            CmpOp::Le => "set.le.u32.u16",
            CmpOp::Gt => "set.gt.u32.u16",
            CmpOp::Ge => "set.ge.u32.u16",
            CmpOp::LtS => "set.lt.u32.s16",
            CmpOp::LeS => "set.le.u32.s16",
            CmpOp::GtS => "set.gt.u32.s16",
            CmpOp::GeS => "set.ge.u32.s16",
        }
    }

    fn scalar16_selp_mnemonic(self) -> &'static str {
        match self {
            CmpOp::LtS | CmpOp::LeS | CmpOp::GtS | CmpOp::GeS => "selp.s16",
            CmpOp::Eq | CmpOp::Ne | CmpOp::Lt | CmpOp::Le | CmpOp::Gt | CmpOp::Ge => "selp.u16",
        }
    }

    fn wide_setp_mnemonic(self) -> &'static str {
        match self {
            CmpOp::Eq => "setp.eq.u64",
            CmpOp::Ne => "setp.ne.u64",
            CmpOp::Lt => "setp.lt.u64",
            CmpOp::Le => "setp.le.u64",
            CmpOp::Gt => "setp.gt.u64",
            CmpOp::Ge => "setp.ge.u64",
            CmpOp::LtS => "setp.lt.s64",
            CmpOp::LeS => "setp.le.s64",
            CmpOp::GtS => "setp.gt.s64",
            CmpOp::GeS => "setp.ge.s64",
        }
    }

    fn wide_set_mnemonic(self) -> &'static str {
        match self {
            CmpOp::Eq => "set.eq.u32.u64",
            CmpOp::Ne => "set.ne.u32.u64",
            CmpOp::Lt => "set.lt.u32.u64",
            CmpOp::Le => "set.le.u32.u64",
            CmpOp::Gt => "set.gt.u32.u64",
            CmpOp::Ge => "set.ge.u32.u64",
            CmpOp::LtS => "set.lt.u32.s64",
            CmpOp::LeS => "set.le.u32.s64",
            CmpOp::GtS => "set.gt.u32.s64",
            CmpOp::GeS => "set.ge.u32.s64",
        }
    }

    fn wide_setp_bool_mnemonic(self, op: PredicateBoolOp) -> String {
        let suffix = op.suffix();
        match self {
            CmpOp::Eq => format!("setp.eq.{suffix}.u64"),
            CmpOp::Ne => format!("setp.ne.{suffix}.u64"),
            CmpOp::Lt => format!("setp.lt.{suffix}.u64"),
            CmpOp::Le => format!("setp.le.{suffix}.u64"),
            CmpOp::Gt => format!("setp.gt.{suffix}.u64"),
            CmpOp::Ge => format!("setp.ge.{suffix}.u64"),
            CmpOp::LtS => format!("setp.lt.{suffix}.s64"),
            CmpOp::LeS => format!("setp.le.{suffix}.s64"),
            CmpOp::GtS => format!("setp.gt.{suffix}.s64"),
            CmpOp::GeS => format!("setp.ge.{suffix}.s64"),
        }
    }

    fn wide_cvt_mnemonic(self) -> &'static str {
        match self {
            CmpOp::LtS | CmpOp::LeS | CmpOp::GtS | CmpOp::GeS => "cvt.s64.s32",
            CmpOp::Eq | CmpOp::Ne | CmpOp::Lt | CmpOp::Le | CmpOp::Gt | CmpOp::Ge => "cvt.u64.u32",
        }
    }

    fn setp_bool_mnemonic(self, op: PredicateBoolOp) -> String {
        let suffix = op.suffix();
        match self {
            CmpOp::Eq => format!("setp.eq.{suffix}.u32"),
            CmpOp::Ne => format!("setp.ne.{suffix}.u32"),
            CmpOp::Lt => format!("setp.lt.{suffix}.u32"),
            CmpOp::Le => format!("setp.le.{suffix}.u32"),
            CmpOp::Gt => format!("setp.gt.{suffix}.u32"),
            CmpOp::Ge => format!("setp.ge.{suffix}.u32"),
            CmpOp::LtS => format!("setp.lt.{suffix}.s32"),
            CmpOp::LeS => format!("setp.le.{suffix}.s32"),
            CmpOp::GtS => format!("setp.gt.{suffix}.s32"),
            CmpOp::GeS => format!("setp.ge.{suffix}.s32"),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PredicateBoolOp {
    And,
    Or,
    Xor,
}

impl PredicateBoolOp {
    fn suffix(self) -> &'static str {
        match self {
            PredicateBoolOp::And => "and",
            PredicateBoolOp::Or => "or",
            PredicateBoolOp::Xor => "xor",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PredicateLogicOp {
    And,
    Or,
    Xor,
    Not,
}

impl PredicateLogicOp {
    fn mnemonic(self) -> &'static str {
        match self {
            PredicateLogicOp::And => "and.pred",
            PredicateLogicOp::Or => "or.pred",
            PredicateLogicOp::Xor => "xor.pred",
            PredicateLogicOp::Not => "not.pred",
        }
    }
}

#[derive(Clone, Copy)]
enum FunnelDir {
    Left,
    Right,
}

impl FunnelDir {
    fn mnemonic(self, mode: FunnelMode) -> &'static str {
        match (self, mode) {
            // .wrap masks the shift amount to 5 bits, while .clamp has defined
            // behavior for out-of-range counts.
            (FunnelDir::Left, FunnelMode::Wrap) => "shf.l.wrap.b32",
            (FunnelDir::Right, FunnelMode::Wrap) => "shf.r.wrap.b32",
            (FunnelDir::Left, FunnelMode::Clamp) => "shf.l.clamp.b32",
            (FunnelDir::Right, FunnelMode::Clamp) => "shf.r.clamp.b32",
        }
    }
}

#[derive(Clone, Copy)]
enum FunnelMode {
    Wrap,
    Clamp,
}

impl FunnelMode {
    fn max_immediate_amount(self) -> u32 {
        match self {
            FunnelMode::Wrap => 31,
            FunnelMode::Clamp => 63,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Operand {
    Reg(u32),
    Imm(u32),
}

impl Operand {
    fn emit(self, out: &mut String) {
        match self {
            Operand::Reg(i) => write!(out, "%r{i}").unwrap(),
            Operand::Imm(v) => write!(out, "{v}").unwrap(),
        }
    }
}

fn sanitize_xor_not_operand(operand: Operand) -> Operand {
    match operand {
        Operand::Imm(0xFFFF_FFFF) => Operand::Imm(0xFFFF_FFFE),
        _ => operand,
    }
}

const NEGATED_PRED_BIT: u32 = 1 << 31;

fn pred_id(pred: u32) -> u32 {
    pred & !NEGATED_PRED_BIT
}

fn pred_is_negated(pred: u32) -> bool {
    pred & NEGATED_PRED_BIT != 0
}

fn pred_guard(pred: u32) -> String {
    if pred_is_negated(pred) {
        format!("@!%p{}", pred_id(pred))
    } else {
        format!("@%p{}", pred_id(pred))
    }
}

enum Inst {
    Bin {
        op: BinOp,
        dst: u32,
        a: Operand,
        b: Operand,
    },
    /// Packed halfword add through a 32-bit register.
    PackedAdd {
        op: PackedAddOp,
        dst: u32,
        a: Operand,
        b: Operand,
    },
    /// Packed halfword min/max through a 32-bit register.
    PackedMinMax {
        op: PackedMinMaxOp,
        dst: u32,
        a: Operand,
        b: Operand,
    },
    /// Scalar 16-bit ALU through `.b16` scratch registers.
    Scalar16 {
        op: Scalar16Op,
        dst: u32,
        a: Operand,
        b: Operand,
    },
    /// Scalar 16-bit `setp` through `.b16` scratch registers, consumed by `selp.b32`.
    Scalar16Setp {
        cmp: CmpOp,
        dst: u32,
        a: Operand,
        b: Operand,
        pred: u32,
    },
    /// Scalar 16-bit `set` through `.b16` scratch registers.
    Scalar16Set {
        cmp: CmpOp,
        dst: u32,
        a: Operand,
        b: Operand,
    },
    /// Scalar 16-bit `setp` feeding `selp.{u16,s16}` through `.b16` scratch registers.
    Scalar16Selp {
        cmp: CmpOp,
        dst: u32,
        a: Operand,
        b: Operand,
        pred: u32,
    },
    /// Bounded read-only load from the existing input buffer.
    GlobalLoad {
        op: GlobalLoadOp,
        cache: GlobalLoadCacheOp,
        volatile: bool,
        uniform: bool,
        dst: u32,
        offset: u32,
    },
    /// Predicated bounded read-only global load from the input buffer.
    PredicatedGlobalLoad {
        op: GlobalLoadOp,
        cache: GlobalLoadCacheOp,
        volatile: bool,
        uniform: bool,
        dst: u32,
        offset: u32,
        cmp: CmpOp,
        ca: Operand,
        cb: Operand,
        pred: u32,
    },
    /// Store to and reload from this thread's output slice in global memory.
    GlobalStoreRoundtrip {
        op: GlobalStoreRoundtripOp,
        store_cache: GlobalStoreCacheOp,
        volatile: bool,
        dst: u32,
        src: u32,
        offset: u32,
    },
    /// Predicated store/reload through this thread's output slice.
    PredicatedGlobalStoreRoundtrip {
        op: GlobalStoreRoundtripOp,
        store_cache: GlobalStoreCacheOp,
        volatile: bool,
        dst: u32,
        src: u32,
        offset: u32,
        cmp: CmpOp,
        ca: Operand,
        cb: Operand,
        pred: u32,
    },
    /// Bounded read-only load from module-scope constant memory.
    ConstLoad {
        op: ConstLoadOp,
        dst: u32,
        offset: u32,
    },
    /// Predicated bounded read-only load from module-scope constant memory.
    PredicatedConstLoad {
        op: ConstLoadOp,
        dst: u32,
        offset: u32,
        cmp: CmpOp,
        ca: Operand,
        cb: Operand,
        pred: u32,
    },
    /// Store to and reload from private per-thread local memory.
    LocalMem {
        op: LocalMemOp,
        dst: u32,
        src: u32,
        offset: u32,
    },
    /// Predicated store/reload through private per-thread local memory.
    PredicatedLocalMem {
        op: LocalMemOp,
        dst: u32,
        src: u32,
        offset: u32,
        cmp: CmpOp,
        ca: Operand,
        cb: Operand,
        pred: u32,
    },
    /// Store to and reload from this thread's private shared-memory slot.
    SharedMem {
        op: SharedMemOp,
        volatile: bool,
        dst: u32,
        src: u32,
        offset: u32,
    },
    /// Predicated store/reload through this thread's private shared-memory slot.
    PredicatedSharedMem {
        op: SharedMemOp,
        volatile: bool,
        dst: u32,
        src: u32,
        offset: u32,
        cmp: CmpOp,
        ca: Operand,
        cb: Operand,
        pred: u32,
    },
    /// Aligned vector load from the input buffer.
    GlobalVectorLoad {
        op: VectorMemOp,
        cache: GlobalLoadCacheOp,
        volatile: bool,
        uniform: bool,
        dsts: [u32; 4],
        offset: u32,
    },
    /// Predicated aligned vector load from the input buffer.
    PredicatedGlobalVectorLoad {
        op: VectorMemOp,
        cache: GlobalLoadCacheOp,
        volatile: bool,
        uniform: bool,
        dsts: [u32; 4],
        offset: u32,
        cmp: CmpOp,
        ca: Operand,
        cb: Operand,
        pred: u32,
    },
    /// Aligned vector store/reload through this thread's output slice.
    GlobalVectorStoreRoundtrip {
        op: VectorMemOp,
        store_cache: GlobalStoreCacheOp,
        volatile: bool,
        dsts: [u32; 4],
        srcs: [u32; 4],
        offset: u32,
    },
    /// Predicated aligned vector store/reload through this thread's output slice.
    PredicatedGlobalVectorStoreRoundtrip {
        op: VectorMemOp,
        store_cache: GlobalStoreCacheOp,
        volatile: bool,
        dsts: [u32; 4],
        srcs: [u32; 4],
        offset: u32,
        cmp: CmpOp,
        ca: Operand,
        cb: Operand,
        pred: u32,
    },
    /// Aligned vector load from module-scope constant memory.
    ConstVectorLoad {
        op: VectorMemOp,
        dsts: [u32; 4],
        offset: u32,
    },
    /// Predicated aligned vector load from module-scope constant memory.
    PredicatedConstVectorLoad {
        op: VectorMemOp,
        dsts: [u32; 4],
        offset: u32,
        cmp: CmpOp,
        ca: Operand,
        cb: Operand,
        pred: u32,
    },
    /// Aligned vector store/reload through private per-thread local memory.
    LocalVectorMem {
        op: VectorMemOp,
        dsts: [u32; 4],
        srcs: [u32; 4],
        offset: u32,
    },
    /// Predicated aligned vector store/reload through private per-thread local memory.
    PredicatedLocalVectorMem {
        op: VectorMemOp,
        dsts: [u32; 4],
        srcs: [u32; 4],
        offset: u32,
        cmp: CmpOp,
        ca: Operand,
        cb: Operand,
        pred: u32,
    },
    /// Aligned vector store/reload through this thread's private shared slot.
    SharedVectorMem {
        op: VectorMemOp,
        volatile: bool,
        dsts: [u32; 4],
        srcs: [u32; 4],
        offset: u32,
    },
    /// Predicated aligned vector store/reload through this thread's private shared slot.
    PredicatedSharedVectorMem {
        op: VectorMemOp,
        volatile: bool,
        dsts: [u32; 4],
        srcs: [u32; 4],
        offset: u32,
        cmp: CmpOp,
        ca: Operand,
        cb: Operand,
        pred: u32,
    },
    /// Sanitized single-precision floating-point arithmetic.
    F32Arith {
        op: F32ArithOp,
        dst: u32,
        a: Operand,
        b: Operand,
        c: Operand,
    },
    /// Predicated sanitized single-precision floating-point arithmetic.
    PredicatedF32Arith {
        op: F32ArithOp,
        dst: u32,
        a: Operand,
        b: Operand,
        c: Operand,
        cmp: CmpOp,
        ca: Operand,
        cb: Operand,
        pred: u32,
    },
    /// Sanitized f32 arithmetic with explicit non-default rounding modes.
    F32RoundingArith {
        op: F32RoundingArithOp,
        dst: u32,
        a: Operand,
        b: Operand,
        c: Operand,
    },
    /// Predicated sanitized f32 arithmetic with explicit non-default rounding modes.
    PredicatedF32RoundingArith {
        op: F32RoundingArithOp,
        dst: u32,
        a: Operand,
        b: Operand,
        c: Operand,
        cmp: CmpOp,
        ca: Operand,
        cb: Operand,
        pred: u32,
    },
    /// Sanitized single-precision unary arithmetic.
    F32Unary {
        op: F32UnaryOp,
        dst: u32,
        src: Operand,
    },
    /// Predicated sanitized single-precision unary arithmetic.
    PredicatedF32Unary {
        op: F32UnaryOp,
        dst: u32,
        src: Operand,
        cmp: CmpOp,
        ca: Operand,
        cb: Operand,
        pred: u32,
    },
    /// Sanitized f32/int conversion chain.
    F32Cvt {
        from_int: F32FromIntCvtOp,
        to_int: F32ToIntCvtOp,
        dst: u32,
        src: Operand,
    },
    /// Predicated sanitized f32/int conversion chain.
    PredicatedF32Cvt {
        from_int: F32FromIntCvtOp,
        to_int: F32ToIntCvtOp,
        dst: u32,
        src: Operand,
        cmp: CmpOp,
        ca: Operand,
        cb: Operand,
        pred: u32,
    },
    /// Sanitized f64-to-f32 conversion.
    F32FloatCvt {
        op: F32FromF64CvtOp,
        dst: u32,
        src: Operand,
    },
    /// Predicated sanitized f64-to-f32 conversion.
    PredicatedF32FloatCvt {
        op: F32FromF64CvtOp,
        dst: u32,
        src: Operand,
        cmp: CmpOp,
        ca: Operand,
        cb: Operand,
        pred: u32,
    },
    /// Sanitized single-precision special math.
    F32SpecialMath {
        op: F32SpecialMathOp,
        dst: u32,
        src: Operand,
    },
    /// Predicated sanitized single-precision special math.
    PredicatedF32SpecialMath {
        op: F32SpecialMathOp,
        dst: u32,
        src: Operand,
        cmp: CmpOp,
        ca: Operand,
        cb: Operand,
        pred: u32,
    },
    /// Sanitized single-precision floating-point compare materialized as u32.
    F32Set {
        cmp: F32CmpOp,
        dst: u32,
        a: Operand,
        b: Operand,
    },
    /// Predicated single-precision floating-point compare materialized as u32.
    PredicatedF32Set {
        cmp: F32CmpOp,
        dst: u32,
        a: Operand,
        b: Operand,
        guard_cmp: CmpOp,
        guard_ca: Operand,
        guard_cb: Operand,
        guard_pred: u32,
    },
    /// Sanitized single-precision `setp.<cmp>.<and|or|xor>` materialized as u32.
    F32SetpBool {
        bool_op: PredicateBoolOp,
        base_cmp: CmpOp,
        base_a: Operand,
        base_b: Operand,
        cmp: F32CmpOp,
        dst: u32,
        a: Operand,
        b: Operand,
        base_pred: u32,
        pred: u32,
    },
    /// Instruction-predicated single-precision `setp.<cmp>.<and|or|xor>`.
    PredicatedF32SetpBool {
        bool_op: PredicateBoolOp,
        base_cmp: CmpOp,
        base_a: Operand,
        base_b: Operand,
        cmp: F32CmpOp,
        dst: u32,
        a: Operand,
        b: Operand,
        base_pred: u32,
        pred: u32,
        guard_cmp: CmpOp,
        guard_ca: Operand,
        guard_cb: Operand,
        guard_pred: u32,
    },
    /// Single-precision floating-point classification with `testp`.
    F32Testp {
        op: FloatTestpOp,
        dst: u32,
        src: Operand,
        pred: u32,
    },
    /// Instruction-predicated single-precision floating-point classification.
    PredicatedF32Testp {
        op: FloatTestpOp,
        dst: u32,
        src: Operand,
        pred: u32,
        guard_cmp: CmpOp,
        guard_ca: Operand,
        guard_cb: Operand,
        guard_pred: u32,
    },
    /// Sanitized single-precision compare feeding `selp.f32`.
    F32Selp {
        cmp: F32CmpOp,
        dst: u32,
        a: Operand,
        b: Operand,
        pred: u32,
    },
    /// Instruction-predicated single-precision compare feeding `selp.f32`.
    PredicatedF32Selp {
        cmp: F32CmpOp,
        dst: u32,
        a: Operand,
        b: Operand,
        pred: u32,
        guard_cmp: CmpOp,
        guard_ca: Operand,
        guard_cb: Operand,
        guard_pred: u32,
    },
    /// Sanitized double-precision floating-point arithmetic.
    F64Arith {
        op: F64ArithOp,
        dst: u32,
        a: Operand,
        b: Operand,
        c: Operand,
    },
    /// Predicated sanitized double-precision floating-point arithmetic.
    PredicatedF64Arith {
        op: F64ArithOp,
        dst: u32,
        a: Operand,
        b: Operand,
        c: Operand,
        cmp: CmpOp,
        ca: Operand,
        cb: Operand,
        pred: u32,
    },
    /// Sanitized f64 arithmetic with explicit non-default rounding modes.
    F64RoundingArith {
        op: F64RoundingArithOp,
        dst: u32,
        a: Operand,
        b: Operand,
        c: Operand,
    },
    /// Predicated sanitized f64 arithmetic with explicit non-default rounding modes.
    PredicatedF64RoundingArith {
        op: F64RoundingArithOp,
        dst: u32,
        a: Operand,
        b: Operand,
        c: Operand,
        cmp: CmpOp,
        ca: Operand,
        cb: Operand,
        pred: u32,
    },
    /// Sanitized double-precision unary arithmetic.
    F64Unary {
        op: F64UnaryOp,
        dst: u32,
        src: Operand,
    },
    /// Predicated sanitized double-precision unary arithmetic.
    PredicatedF64Unary {
        op: F64UnaryOp,
        dst: u32,
        src: Operand,
        cmp: CmpOp,
        ca: Operand,
        cb: Operand,
        pred: u32,
    },
    /// Sanitized f64/int conversion chain.
    F64Cvt {
        from_int: F64FromIntCvtOp,
        to_int: F64ToIntCvtOp,
        dst: u32,
        src: Operand,
    },
    /// Predicated sanitized f64/int conversion chain.
    PredicatedF64Cvt {
        from_int: F64FromIntCvtOp,
        to_int: F64ToIntCvtOp,
        dst: u32,
        src: Operand,
        cmp: CmpOp,
        ca: Operand,
        cb: Operand,
        pred: u32,
    },
    /// Sanitized f32-to-f64 conversion.
    F64FloatCvt {
        op: F64FromF32CvtOp,
        dst: u32,
        src: Operand,
    },
    /// Predicated sanitized f32-to-f64 conversion.
    PredicatedF64FloatCvt {
        op: F64FromF32CvtOp,
        dst: u32,
        src: Operand,
        cmp: CmpOp,
        ca: Operand,
        cb: Operand,
        pred: u32,
    },
    /// Sanitized double-precision special math.
    F64SpecialMath {
        op: F64SpecialMathOp,
        dst: u32,
        src: Operand,
    },
    /// Predicated sanitized double-precision special math.
    PredicatedF64SpecialMath {
        op: F64SpecialMathOp,
        dst: u32,
        src: Operand,
        cmp: CmpOp,
        ca: Operand,
        cb: Operand,
        pred: u32,
    },
    /// Sanitized double-precision compare materialized as u32.
    F64Set {
        cmp: FloatCmpOp,
        dst: u32,
        a: Operand,
        b: Operand,
    },
    /// Predicated double-precision floating-point compare materialized as u32.
    PredicatedF64Set {
        cmp: FloatCmpOp,
        dst: u32,
        a: Operand,
        b: Operand,
        guard_cmp: CmpOp,
        guard_ca: Operand,
        guard_cb: Operand,
        guard_pred: u32,
    },
    /// Sanitized double-precision `setp.<cmp>.<and|or|xor>` materialized as u32.
    F64SetpBool {
        bool_op: PredicateBoolOp,
        base_cmp: CmpOp,
        base_a: Operand,
        base_b: Operand,
        cmp: FloatCmpOp,
        dst: u32,
        a: Operand,
        b: Operand,
        base_pred: u32,
        pred: u32,
    },
    /// Instruction-predicated double-precision `setp.<cmp>.<and|or|xor>`.
    PredicatedF64SetpBool {
        bool_op: PredicateBoolOp,
        base_cmp: CmpOp,
        base_a: Operand,
        base_b: Operand,
        cmp: FloatCmpOp,
        dst: u32,
        a: Operand,
        b: Operand,
        base_pred: u32,
        pred: u32,
        guard_cmp: CmpOp,
        guard_ca: Operand,
        guard_cb: Operand,
        guard_pred: u32,
    },
    /// Double-precision floating-point classification with `testp`.
    F64Testp {
        op: FloatTestpOp,
        dst: u32,
        src_lo: Operand,
        src_hi: Operand,
        pred: u32,
    },
    /// Instruction-predicated double-precision floating-point classification.
    PredicatedF64Testp {
        op: FloatTestpOp,
        dst: u32,
        src_lo: Operand,
        src_hi: Operand,
        pred: u32,
        guard_cmp: CmpOp,
        guard_ca: Operand,
        guard_cb: Operand,
        guard_pred: u32,
    },
    /// Sanitized double-precision compare feeding `selp.f64`.
    F64Selp {
        cmp: FloatCmpOp,
        dst: u32,
        a: Operand,
        b: Operand,
        pred: u32,
    },
    /// Instruction-predicated double-precision compare feeding `selp.f64`.
    PredicatedF64Selp {
        cmp: FloatCmpOp,
        dst: u32,
        a: Operand,
        b: Operand,
        pred: u32,
        guard_cmp: CmpOp,
        guard_ca: Operand,
        guard_cb: Operand,
        guard_pred: u32,
    },
    Sel {
        op: SelpOp,
        dst: u32,
        a: Operand,
        b: Operand,
        cmp: CmpOp,
        ca: Operand,
        cb: Operand,
        pred: u32,
    },
    /// `setp` for selp input and instruction guard, then guarded `selp.b32`.
    PredicatedSel {
        op: SelpOp,
        dst: u32,
        a: Operand,
        b: Operand,
        cmp: CmpOp,
        ca: Operand,
        cb: Operand,
        pred: u32,
        guard_cmp: CmpOp,
        guard_ca: Operand,
        guard_cb: Operand,
        guard_pred: u32,
    },
    /// `setp.<cmp> pred, ca, cb; @pred <binop> dst, a, b;`.
    PredicatedBin {
        op: BinOp,
        dst: u32,
        a: Operand,
        b: Operand,
        cmp: CmpOp,
        ca: Operand,
        cb: Operand,
        pred: u32,
    },
    /// `setp.<cmp> pred, ca, cb; @pred add.{u16x2,s16x2} dst, a, b;`.
    PredicatedPackedAdd {
        op: PackedAddOp,
        dst: u32,
        a: Operand,
        b: Operand,
        cmp: CmpOp,
        ca: Operand,
        cb: Operand,
        pred: u32,
    },
    /// `setp.<cmp> pred, ca, cb; @pred min/max.{u16x2,s16x2} dst, a, b;`.
    PredicatedPackedMinMax {
        op: PackedMinMaxOp,
        dst: u32,
        a: Operand,
        b: Operand,
        cmp: CmpOp,
        ca: Operand,
        cb: Operand,
        pred: u32,
    },
    /// Predicated scalar 16-bit ALU through `.b16` scratch registers.
    PredicatedScalar16 {
        op: Scalar16Op,
        dst: u32,
        a: Operand,
        b: Operand,
        cmp: CmpOp,
        ca: Operand,
        cb: Operand,
        pred: u32,
    },
    /// `setp` + `setp.<cmp>.<bool>` feeding a guarded ALU instruction.
    SetpBoolBin {
        bool_op: PredicateBoolOp,
        base_cmp: CmpOp,
        base_a: Operand,
        base_b: Operand,
        cmp: CmpOp,
        cmp_a: Operand,
        cmp_b: Operand,
        base_pred: u32,
        guard_pred: u32,
        op: BinOp,
        dst: u32,
        a: Operand,
        b: Operand,
    },
    /// `setp.<cmp> %p|%q` feeding complementary guarded ALU instructions.
    SetpDualBin {
        cmp: CmpOp,
        cmp_a: Operand,
        cmp_b: Operand,
        true_pred: u32,
        false_pred: u32,
        dst: u32,
        true_op: BinOp,
        true_a: Operand,
        true_b: Operand,
        false_op: BinOp,
        false_a: Operand,
        false_b: Operand,
    },
    /// Predicate logic op fed by one or two `setp` producers, then guarded ALU.
    PredLogicBin {
        logic_op: PredicateLogicOp,
        lhs_cmp: CmpOp,
        lhs_a: Operand,
        lhs_b: Operand,
        rhs_cmp: CmpOp,
        rhs_a: Operand,
        rhs_b: Operand,
        lhs_pred: u32,
        rhs_pred: u32,
        guard_pred: u32,
        op: BinOp,
        dst: u32,
        a: Operand,
        b: Operand,
    },
    /// `setp.<cmp> pred, ca, cb; @pred <shift> dst, src, imm;`.
    PredicatedShift {
        op: ShiftOp,
        dst: u32,
        src: Operand,
        amount: u32,
        cmp: CmpOp,
        ca: Operand,
        cb: Operand,
        pred: u32,
    },
    /// `set.<cmp>.u32.{u32,s32} dst, a, b;` — materialize comparison result.
    Set {
        dst: u32,
        cmp: CmpOp,
        a: Operand,
        b: Operand,
    },
    /// `setp.<cmp> guard, ca, cb; @guard set.<cmp> dst, a, b;`.
    PredicatedSet {
        dst: u32,
        cmp: CmpOp,
        a: Operand,
        b: Operand,
        guard_cmp: CmpOp,
        guard_ca: Operand,
        guard_cb: Operand,
        guard_pred: u32,
    },
    /// `<op>.b32 dst, src, amount;` where amount is an immediate in 0..=31
    /// (avoids shift-amount-≥-32 UB).
    Shift {
        op: ShiftOp,
        dst: u32,
        src: Operand,
        amount: u32,
    },
    /// `<op>.b32 dst, src, (amount & 31);` using a scratch register.
    RegShift {
        op: ShiftOp,
        dst: u32,
        src: Operand,
        amount: Operand,
    },
    /// `setp.<cmp> pred, ca, cb; @pred <shift> dst, src, (amount & 31);`.
    PredicatedRegShift {
        op: ShiftOp,
        dst: u32,
        src: Operand,
        amount: Operand,
        cmp: CmpOp,
        ca: Operand,
        cb: Operand,
        pred: u32,
    },
    /// `<op>.b32 dst, src;`
    Unary { op: UnaryOp, dst: u32, src: Operand },
    /// `setp.<cmp> pred, ca, cb; @pred <unary> dst, src;`.
    PredicatedUnary {
        op: UnaryOp,
        dst: u32,
        src: Operand,
        cmp: CmpOp,
        ca: Operand,
        cb: Operand,
        pred: u32,
    },
    /// `mov.u32 dst, %special;` for deterministic per-lane special registers.
    SpecialReg { op: SpecialRegOp, dst: u32 },
    /// `setp.<cmp> pred, ca, cb; @pred mov.u32 dst, %special;`.
    PredicatedSpecialReg {
        op: SpecialRegOp,
        dst: u32,
        cmp: CmpOp,
        ca: Operand,
        cb: Operand,
        pred: u32,
    },
    /// `cvt.{u32,s32}.{u8,u16,s8,s16} dst, src;` — subword integer extension.
    Cvt { op: CvtOp, dst: u32, src: Operand },
    /// `setp.<cmp> pred, ca, cb; @pred cvt.{...} dst, src;`.
    PredicatedCvt {
        op: CvtOp,
        dst: u32,
        src: Operand,
        cmp: CmpOp,
        ca: Operand,
        cb: Operand,
        pred: u32,
    },
    /// Narrow to 8/16 bits and re-extend, so the output register is fully defined.
    NarrowCvt {
        op: NarrowCvtOp,
        dst: u32,
        src: Operand,
    },
    /// Predicated narrow cvt round-trip.
    PredicatedNarrowCvt {
        op: NarrowCvtOp,
        dst: u32,
        src: Operand,
        cmp: CmpOp,
        ca: Operand,
        cb: Operand,
        pred: u32,
    },
    /// Convert through a scratch 64-bit register and truncate back to 32 bits.
    WideCvt {
        op: WideCvtOp,
        dst: u32,
        src: Operand,
    },
    /// Predicated 64-bit-source cvt round-trip.
    PredicatedWideCvt {
        op: WideCvtOp,
        dst: u32,
        src: Operand,
        cmp: CmpOp,
        ca: Operand,
        cb: Operand,
        pred: u32,
    },
    /// `szext.{wrap,clamp}.{u32,s32} dst, src, width;`.
    Szext {
        op: SzextOp,
        dst: u32,
        src: Operand,
        width: Operand,
    },
    /// `setp.<cmp> pred, ca, cb; @pred szext.{...} dst, src, width;`.
    PredicatedSzext {
        op: SzextOp,
        dst: u32,
        src: Operand,
        width: Operand,
        cmp: CmpOp,
        ca: Operand,
        cb: Operand,
        pred: u32,
    },
    /// `bfind[.shiftamt].u32 dst, src;` — bit position / shift amount.
    Bfind { op: BfindOp, dst: u32, src: Operand },
    /// `setp.<cmp> pred, ca, cb; @pred bfind[.shiftamt].u32 dst, src;`.
    PredicatedBfind {
        op: BfindOp,
        dst: u32,
        src: Operand,
        cmp: CmpOp,
        ca: Operand,
        cb: Operand,
        pred: u32,
    },
    /// `fns.b32 dst, mask, base, offset;` with base in the defined 0..31 range.
    Fns {
        dst: u32,
        mask: Operand,
        base: u32,
        offset: i32,
    },
    /// `setp.<cmp> pred, ca, cb; @pred fns.b32 dst, mask, base, offset;`.
    PredicatedFns {
        dst: u32,
        mask: Operand,
        base: u32,
        offset: i32,
        cmp: CmpOp,
        ca: Operand,
        cb: Operand,
        pred: u32,
    },
    /// `fns.b32` with one base/offset parameter sanitized through a register.
    RegFns {
        dst: u32,
        mask: Operand,
        param: Operand,
        slot: FnsParamSlot,
        imm: i32,
    },
    /// Predicated `fns.b32` with one sanitized register base/offset parameter.
    PredicatedRegFns {
        dst: u32,
        mask: Operand,
        param: Operand,
        slot: FnsParamSlot,
        imm: i32,
        cmp: CmpOp,
        ca: Operand,
        cb: Operand,
        pred: u32,
    },
    /// `div/rem.u32 dst, src, divisor;` with a nonzero immediate divisor.
    DivRem {
        op: DivRemOp,
        dst: u32,
        src: Operand,
        divisor: u32,
    },
    /// `or.b32 scratch, divisor, 1; div/rem.u32 dst, src, scratch;`.
    RegDivRem {
        op: DivRemOp,
        dst: u32,
        src: Operand,
        divisor: Operand,
    },
    /// `setp.<cmp> pred, ca, cb; @pred div/rem dst, src, divisor;`.
    PredicatedDivRem {
        op: DivRemOp,
        dst: u32,
        src: Operand,
        divisor: u32,
        cmp: CmpOp,
        ca: Operand,
        cb: Operand,
        pred: u32,
    },
    /// Register-divisor unsigned div/rem behind an instruction predicate.
    PredicatedRegDivRem {
        op: DivRemOp,
        dst: u32,
        src: Operand,
        divisor: Operand,
        cmp: CmpOp,
        ca: Operand,
        cb: Operand,
        pred: u32,
    },
    /// `mad24.{lo,hi}.{u32,s32} dst, a, b, c;` — 24-bit multiply plus addend.
    Mad24 {
        op: Mad24Op,
        dst: u32,
        a: Operand,
        b: Operand,
        c: Operand,
    },
    /// `setp.<cmp> pred, ca, cb; @pred mad24.* dst, a, b, c;`.
    PredicatedMad24 {
        op: Mad24Op,
        dst: u32,
        a: Operand,
        b: Operand,
        c: Operand,
        cmp: CmpOp,
        ca: Operand,
        cb: Operand,
        pred: u32,
    },
    /// `mul24.{lo,hi}.{u32,s32} dst, a, b;` — 24-bit multiply.
    Mul24 {
        op: Mul24Op,
        dst: u32,
        a: Operand,
        b: Operand,
    },
    /// `setp.<cmp> pred, ca, cb; @pred mul24.* dst, a, b;`.
    PredicatedMul24 {
        op: Mul24Op,
        dst: u32,
        a: Operand,
        b: Operand,
        cmp: CmpOp,
        ca: Operand,
        cb: Operand,
        pred: u32,
    },
    /// 16-bit source `mul/mad.wide` through `.b16` scratch registers.
    SubwordWide {
        op: SubwordWideOp,
        dst: u32,
        a: Operand,
        b: Operand,
        c: Operand,
    },
    /// Predicated 16-bit source `mul/mad.wide`.
    PredicatedSubwordWide {
        op: SubwordWideOp,
        dst: u32,
        a: Operand,
        b: Operand,
        c: Operand,
        cmp: CmpOp,
        ca: Operand,
        cb: Operand,
        pred: u32,
    },
    /// `mul.wide.{u32,s32}` through a scratch b64 register.
    MulWide {
        op: MulWideOp,
        dst: u32,
        a: Operand,
        b: Operand,
        keep_high: bool,
    },
    /// `setp.<cmp> pred, ca, cb; @pred mul.wide.* ...; @pred mov.b64 ...;`.
    PredicatedMulWide {
        op: MulWideOp,
        dst: u32,
        a: Operand,
        b: Operand,
        keep_high: bool,
        cmp: CmpOp,
        ca: Operand,
        cb: Operand,
        pred: u32,
    },
    /// `mad.wide.{u32,s32}` through scratch b64 registers.
    MadWide {
        op: MadWideOp,
        dst: u32,
        a: Operand,
        b: Operand,
        c: Operand,
        keep_high: bool,
    },
    /// `setp.<cmp> pred, ca, cb; @pred mad.wide.* ...; @pred mov.b64 ...;`.
    PredicatedMadWide {
        op: MadWideOp,
        dst: u32,
        a: Operand,
        b: Operand,
        c: Operand,
        keep_high: bool,
        cmp: CmpOp,
        ca: Operand,
        cb: Operand,
        pred: u32,
    },
    /// 64-bit integer ALU through scratch b64 registers, low 32 bits kept.
    WideInt {
        op: WideIntOp,
        dst: u32,
        a: Operand,
        b: Operand,
    },
    /// 64-bit integer ALU through scratch b64 registers behind a predicate.
    PredicatedWideInt {
        op: WideIntOp,
        dst: u32,
        a: Operand,
        b: Operand,
        cmp: CmpOp,
        ca: Operand,
        cb: Operand,
        pred: u32,
    },
    /// 64-bit operand `mad.{lo,hi}.{u64,s64}` through scratch b64 registers.
    WideMad64 {
        op: WideMad64Op,
        dst: u32,
        a: Operand,
        b: Operand,
        c: Operand,
    },
    /// Predicated 64-bit operand MAD.
    PredicatedWideMad64 {
        op: WideMad64Op,
        dst: u32,
        a: Operand,
        b: Operand,
        c: Operand,
        cmp: CmpOp,
        ca: Operand,
        cb: Operand,
        pred: u32,
    },
    /// 64-bit `setp` through scratch b64 registers, feeding a guarded ALU op.
    WideSetpBin {
        cmp: CmpOp,
        ca: Operand,
        cb: Operand,
        pred: u32,
        op: BinOp,
        dst: u32,
        a: Operand,
        b: Operand,
    },
    /// 64-bit `setp.<cmp>.<bool>` feeding a guarded ALU op.
    WideSetpBoolBin {
        bool_op: PredicateBoolOp,
        base_cmp: CmpOp,
        base_a: Operand,
        base_b: Operand,
        cmp: CmpOp,
        cmp_a: Operand,
        cmp_b: Operand,
        base_pred: u32,
        guard_pred: u32,
        op: BinOp,
        dst: u32,
        a: Operand,
        b: Operand,
    },
    /// 64-bit `set` materialization through scratch b64 registers.
    WideSet {
        dst: u32,
        cmp: CmpOp,
        a: Operand,
        b: Operand,
    },
    /// Predicated 64-bit `set` materialization.
    PredicatedWideSet {
        dst: u32,
        cmp: CmpOp,
        a: Operand,
        b: Operand,
        guard_cmp: CmpOp,
        guard_ca: Operand,
        guard_cb: Operand,
        guard_pred: u32,
    },
    /// 64-bit compare plus 64-bit `selp`, keeping the selected low 32 bits.
    WideSelp {
        op: Selp64Op,
        cmp: CmpOp,
        ca: Operand,
        cb: Operand,
        pred: u32,
        dst: u32,
        true_value: Operand,
        false_value: Operand,
    },
    /// 64-bit unary op through scratch b64 registers or a b32 result.
    WideUnary {
        op: WideUnaryOp,
        dst: u32,
        src: Operand,
    },
    /// Predicated 64-bit unary op.
    PredicatedWideUnary {
        op: WideUnaryOp,
        dst: u32,
        src: Operand,
        cmp: CmpOp,
        ca: Operand,
        cb: Operand,
        pred: u32,
    },
    /// 64-bit shift through scratch b64 registers, low 32 bits kept.
    WideShift {
        op: WideShiftOp,
        dst: u32,
        src: Operand,
        amount: u32,
    },
    /// 64-bit shift through scratch b64 registers with an explicitly masked register count.
    RegWideShift {
        op: WideShiftOp,
        dst: u32,
        src: Operand,
        amount: Operand,
    },
    /// Predicated 64-bit shift through scratch b64 registers.
    PredicatedWideShift {
        op: WideShiftOp,
        dst: u32,
        src: Operand,
        amount: u32,
        cmp: CmpOp,
        ca: Operand,
        cb: Operand,
        pred: u32,
    },
    /// Predicated 64-bit shift with an explicitly masked register count.
    PredicatedRegWideShift {
        op: WideShiftOp,
        dst: u32,
        src: Operand,
        amount: Operand,
        cmp: CmpOp,
        ca: Operand,
        cb: Operand,
        pred: u32,
    },
    /// 64-bit div/rem through scratch b64 registers with a nonzero divisor.
    WideDivRem {
        op: WideDivRemOp,
        dst: u32,
        src: Operand,
        divisor: WideDivisor,
    },
    /// Predicated 64-bit div/rem with a nonzero divisor.
    PredicatedWideDivRem {
        op: WideDivRemOp,
        dst: u32,
        src: Operand,
        divisor: WideDivisor,
        cmp: CmpOp,
        ca: Operand,
        cb: Operand,
        pred: u32,
    },
    /// 64-bit add/sub carry chain through scratch b64 registers.
    WideCarry {
        op: AddCarryOp,
        dst_lo: u32,
        dst_hi: u32,
        a: Operand,
        b: Operand,
        c: Operand,
        d: Operand,
    },
    /// Predicated 64-bit add/sub carry chain.
    PredicatedWideCarry {
        op: AddCarryOp,
        dst_lo: u32,
        dst_hi: u32,
        a: Operand,
        b: Operand,
        c: Operand,
        d: Operand,
        cmp: CmpOp,
        ca: Operand,
        cb: Operand,
        pred: u32,
    },
    /// Three-instruction 64-bit add/sub carry chain through scratch b64 registers.
    WideCarryChain {
        op: AddCarryOp,
        dst0: u32,
        dst1: u32,
        dst2: u32,
        a: Operand,
        b: Operand,
        c: Operand,
        d: Operand,
        e: Operand,
        f: Operand,
    },
    /// Predicated three-instruction 64-bit add/sub carry chain.
    PredicatedWideCarryChain {
        op: AddCarryOp,
        dst0: u32,
        dst1: u32,
        dst2: u32,
        a: Operand,
        b: Operand,
        c: Operand,
        d: Operand,
        e: Operand,
        f: Operand,
        cmp: CmpOp,
        ca: Operand,
        cb: Operand,
        pred: u32,
    },
    /// `add/sub.cc.u32` followed by `addc/subc.u32`, keeping carry dataflow explicit.
    AddCarry {
        op: AddCarryOp,
        dst_lo: u32,
        dst_hi: u32,
        a: Operand,
        b: Operand,
        c: Operand,
        d: Operand,
    },
    /// `setp.<cmp> pred, ca, cb; @pred add/sub.cc; @pred addc/subc`.
    PredicatedAddCarry {
        op: AddCarryOp,
        dst_lo: u32,
        dst_hi: u32,
        a: Operand,
        b: Operand,
        c: Operand,
        d: Operand,
        cmp: CmpOp,
        ca: Operand,
        cb: Operand,
        pred: u32,
    },
    /// Three-instruction `add/sub.cc` followed by `addc/subc.cc` and `addc/subc`.
    CarryChain {
        op: AddCarryOp,
        dst0: u32,
        dst1: u32,
        dst2: u32,
        a: Operand,
        b: Operand,
        c: Operand,
        d: Operand,
        e: Operand,
        f: Operand,
    },
    /// Predicated three-instruction add/sub carry chain.
    PredicatedCarryChain {
        op: AddCarryOp,
        dst0: u32,
        dst1: u32,
        dst2: u32,
        a: Operand,
        b: Operand,
        c: Operand,
        d: Operand,
        e: Operand,
        f: Operand,
        cmp: CmpOp,
        ca: Operand,
        cb: Operand,
        pred: u32,
    },
    /// `sad.u32 dst, a, b, c;` — unsigned sum of absolute difference.
    Sad {
        op: SadOp,
        dst: u32,
        a: Operand,
        b: Operand,
        c: Operand,
    },
    /// `setp.<cmp> pred, ca, cb; @pred sad.{u32,s32} dst, a, b, c;`.
    PredicatedSad {
        op: SadOp,
        dst: u32,
        a: Operand,
        b: Operand,
        c: Operand,
        cmp: CmpOp,
        ca: Operand,
        cb: Operand,
        pred: u32,
    },
    /// `slct.u32.s32 dst, a, b, c;` — select `a` or `b` from signed `c`.
    Slct {
        op: SlctOp,
        dst: u32,
        a: Operand,
        b: Operand,
        c: Operand,
    },
    /// `setp.<cmp> pred, ca, cb; @pred slct.* dst, a, b, c;`.
    PredicatedSlct {
        op: SlctOp,
        dst: u32,
        a: Operand,
        b: Operand,
        c: Operand,
        cmp: CmpOp,
        ca: Operand,
        cb: Operand,
        pred: u32,
    },
    /// `dp4a.{u32,s32}.{u32,s32} dst, a, b, c;` — 4-byte dot product.
    Dp4a {
        op: Dp4aOp,
        dst: u32,
        a: Operand,
        b: Operand,
        c: Operand,
    },
    /// `setp.<cmp> pred, ca, cb; @pred dp4a.* dst, a, b, c;`.
    PredicatedDp4a {
        op: Dp4aOp,
        dst: u32,
        a: Operand,
        b: Operand,
        c: Operand,
        cmp: CmpOp,
        ca: Operand,
        cb: Operand,
        pred: u32,
    },
    /// `dp2a.{lo,hi}.{u32,s32}.{u32,s32} dst, a, b, c;` — 2-lane dot product.
    Dp2a {
        op: Dp2aOp,
        dst: u32,
        a: Operand,
        b: Operand,
        c: Operand,
    },
    /// `setp.<cmp> pred, ca, cb; @pred dp2a.* dst, a, b, c;`.
    PredicatedDp2a {
        op: Dp2aOp,
        dst: u32,
        a: Operand,
        b: Operand,
        c: Operand,
        cmp: CmpOp,
        ca: Operand,
        cb: Operand,
        pred: u32,
    },
    /// Two-/four-lane unsigned video arithmetic with an accumulator.
    Video {
        op: VideoOp,
        dst: u32,
        a: Operand,
        b: Operand,
        c: Operand,
    },
    /// `setp.<cmp> pred, ca, cb; @pred v* dst, a, b, c;`.
    PredicatedVideo {
        op: VideoOp,
        dst: u32,
        a: Operand,
        b: Operand,
        c: Operand,
        cmp: CmpOp,
        ca: Operand,
        cb: Operand,
        pred: u32,
    },
    /// `mad.lo.u32 dst, a, b, c;` — (a*b + c) low 32 bits. Heavy optimizer
    /// fold target (mul+add → mad).
    Mad {
        signed: bool,
        dst: u32,
        a: Operand,
        b: Operand,
        c: Operand,
    },
    /// `setp.<cmp> pred, ca, cb; @pred mad.lo.{u32,s32} dst, a, b, c;`.
    PredicatedMad {
        signed: bool,
        dst: u32,
        a: Operand,
        b: Operand,
        c: Operand,
        cmp: CmpOp,
        ca: Operand,
        cb: Operand,
        pred: u32,
    },
    /// Three-instruction `mad.cc` / `madc.cc` / `madc` carry chain.
    MadCarry {
        op: MadCarryOp,
        dst0: u32,
        dst1: u32,
        dst2: u32,
        a: Operand,
        b: Operand,
        c: Operand,
        d: Operand,
        e: Operand,
        f: Operand,
        g: Operand,
        h: Operand,
        i: Operand,
    },
    /// Predicated `mad.cc` / `madc.cc` / `madc` carry chain.
    PredicatedMadCarry {
        op: MadCarryOp,
        dst0: u32,
        dst1: u32,
        dst2: u32,
        a: Operand,
        b: Operand,
        c: Operand,
        d: Operand,
        e: Operand,
        f: Operand,
        g: Operand,
        h: Operand,
        i: Operand,
        cmp: CmpOp,
        ca: Operand,
        cb: Operand,
        pred: u32,
    },
    /// `mad.hi.{u32,s32} dst, a, b, c;` — high product word plus addend.
    MadHi {
        op: MadHiOp,
        dst: u32,
        a: Operand,
        b: Operand,
        c: Operand,
    },
    /// `setp.<cmp> pred, ca, cb; @pred mad.hi.{u32,s32} dst, a, b, c;`.
    PredicatedMadHi {
        op: MadHiOp,
        dst: u32,
        a: Operand,
        b: Operand,
        c: Operand,
        cmp: CmpOp,
        ca: Operand,
        cb: Operand,
        pred: u32,
    },
    /// `lop3.b32 dst, a, b, c, imm;` — 3-input logical op via 8-bit truth
    /// table. The optimizer canonicalizes many boolean lattices through lop3.
    Lop3 {
        dst: u32,
        a: Operand,
        b: Operand,
        c: Operand,
        imm: u8,
    },
    /// `setp.<cmp> pred, ca, cb; @pred lop3.b32 dst, a, b, c, imm;`.
    PredicatedLop3 {
        dst: u32,
        a: Operand,
        b: Operand,
        c: Operand,
        imm: u8,
        cmp: CmpOp,
        ca: Operand,
        cb: Operand,
        pred: u32,
    },
    /// `prmt.b32 dst, a, b, ctrl;` — byte permute. Each nibble of `ctrl`
    /// (low 16 bits) selects one byte from the 8 source bytes (a|b<<32).
    /// All u32 ctrl values are well-defined.
    Prmt {
        mode: Option<PrmtMode>,
        dst: u32,
        a: Operand,
        b: Operand,
        ctrl: Operand,
    },
    /// `setp.<cmp> pred, ca, cb; @pred prmt.b32 dst, a, b, ctrl;`.
    PredicatedPrmt {
        mode: Option<PrmtMode>,
        dst: u32,
        a: Operand,
        b: Operand,
        ctrl: Operand,
        cmp: CmpOp,
        ca: Operand,
        cb: Operand,
        pred: u32,
    },
    /// `shf.{l,r}.{wrap,clamp}.b32 dst, a, b, amount;`.
    Funnel {
        dir: FunnelDir,
        mode: FunnelMode,
        dst: u32,
        a: Operand,
        b: Operand,
        amount: u32,
    },
    /// `shf.{l,r}.{wrap,clamp}.b32 dst, a, b, amount_reg;`.
    RegFunnel {
        dir: FunnelDir,
        mode: FunnelMode,
        dst: u32,
        a: Operand,
        b: Operand,
        amount: Operand,
    },
    /// `setp.<cmp> pred, ca, cb; @pred shf.* dst, a, b, amount;`.
    PredicatedFunnel {
        dir: FunnelDir,
        mode: FunnelMode,
        dst: u32,
        a: Operand,
        b: Operand,
        amount: Operand,
        cmp: CmpOp,
        ca: Operand,
        cb: Operand,
        pred: u32,
    },
    /// `bfe.u32 dst, src, pos, len;` — extract `len` bits from `src` at
    /// position `pos`. PTX uses the low 8 bits of pos/len → safe.
    Bfe {
        op: BfeOp,
        dst: u32,
        src: Operand,
        pos: Operand,
        len: Operand,
    },
    /// `setp.<cmp> pred, ca, cb; @pred bfe.{u32,s32} dst, src, pos, len;`.
    PredicatedBfe {
        op: BfeOp,
        dst: u32,
        src: Operand,
        pos: Operand,
        len: Operand,
        cmp: CmpOp,
        ca: Operand,
        cb: Operand,
        pred: u32,
    },
    /// 64-bit `bfe` through scratch b64 registers, returning the low half.
    WideBfe {
        op: WideBfeOp,
        dst: u32,
        src: Operand,
        pos: u32,
        len: u32,
    },
    /// 64-bit `bfe` with one sanitized register pos/len parameter.
    RegWideBfe {
        op: WideBfeOp,
        dst: u32,
        src: Operand,
        param: Operand,
        slot: BitfieldParamSlot,
        imm: u32,
    },
    /// Predicated 64-bit `bfe` through scratch b64 registers.
    PredicatedWideBfe {
        op: WideBfeOp,
        dst: u32,
        src: Operand,
        pos: u32,
        len: u32,
        cmp: CmpOp,
        ca: Operand,
        cb: Operand,
        pred: u32,
    },
    /// Predicated 64-bit `bfe` with one sanitized register pos/len parameter.
    PredicatedRegWideBfe {
        op: WideBfeOp,
        dst: u32,
        src: Operand,
        param: Operand,
        slot: BitfieldParamSlot,
        imm: u32,
        cmp: CmpOp,
        ca: Operand,
        cb: Operand,
        pred: u32,
    },
    /// `bfi.b32 dst, src, base, pos, len;` — insert low `len` bits of `src`
    /// into `base` starting at `pos`. PTX uses the low 8 bits of pos/len → safe.
    Bfi {
        dst: u32,
        src: Operand,
        base: Operand,
        pos: Operand,
        len: Operand,
    },
    /// `setp.<cmp> pred, ca, cb; @pred bfi.b32 dst, src, base, pos, len;`.
    PredicatedBfi {
        dst: u32,
        src: Operand,
        base: Operand,
        pos: Operand,
        len: Operand,
        cmp: CmpOp,
        ca: Operand,
        cb: Operand,
        pred: u32,
    },
    /// 64-bit `bfi` through scratch b64 registers, returning the low half.
    WideBfi {
        dst: u32,
        src: Operand,
        base: Operand,
        pos: u32,
        len: u32,
    },
    /// 64-bit `bfi` with one sanitized register pos/len parameter.
    RegWideBfi {
        dst: u32,
        src: Operand,
        base: Operand,
        param: Operand,
        slot: BitfieldParamSlot,
        imm: u32,
    },
    /// Predicated 64-bit `bfi` through scratch b64 registers.
    PredicatedWideBfi {
        dst: u32,
        src: Operand,
        base: Operand,
        pos: u32,
        len: u32,
        cmp: CmpOp,
        ca: Operand,
        cb: Operand,
        pred: u32,
    },
    /// Predicated 64-bit `bfi` with one sanitized register pos/len parameter.
    PredicatedRegWideBfi {
        dst: u32,
        src: Operand,
        base: Operand,
        param: Operand,
        slot: BitfieldParamSlot,
        imm: u32,
        cmp: CmpOp,
        ca: Operand,
        cb: Operand,
        pred: u32,
    },
    /// `bmsk.{clamp,wrap}.b32 dst, pos, len;` — create a bit mask.
    Bmsk {
        mode: BmskMode,
        dst: u32,
        pos: Operand,
        len: Operand,
    },
    /// `setp.<cmp> pred, ca, cb; @pred bmsk.{clamp,wrap}.b32 dst, pos, len;`.
    PredicatedBmsk {
        mode: BmskMode,
        dst: u32,
        pos: Operand,
        len: Operand,
        cmp: CmpOp,
        ca: Operand,
        cb: Operand,
        pred: u32,
    },
}

enum Term {
    Branch(usize),
    CondBranch {
        cmp: CmpOp,
        a: Operand,
        b: Operand,
        pred: u32,
        t: usize,
        f: usize,
    },
    /// `if ctr == 0: bra fwd; else: ctr -= 1; bra back;`
    Loop {
        ctr: u32,
        pred: u32,
        back: usize,
        fwd: usize,
    },
    Return,
}

struct Block {
    insts: Vec<Inst>,
    term: Term,
}

enum StructuredStmt {
    Basic(Vec<Inst>),
    IfElse {
        cmp: CmpOp,
        a: Operand,
        b: Operand,
        pred: u32,
        then_body: Vec<StructuredStmt>,
        else_body: Vec<StructuredStmt>,
    },
    Loop {
        ctr: u32,
        pred: u32,
        body: Vec<StructuredStmt>,
    },
}

struct Generator<'a> {
    cfg: &'a GenConfig,
    n_working: u32,
    n_pred: u32,
    blocks: Vec<Block>,
    prmt_result_regs: Vec<u32>,
    set_result_regs: Vec<u32>,
    /// (reg id, initial value). reg id is ≥ `n_working` (counters live above
    /// the working-reg range).
    counters: Vec<(u32, u32)>,
}

impl<'a> Generator<'a> {
    fn new(cfg: &'a GenConfig) -> Self {
        assert!(
            cfg.n_working_regs >= N_OUTPUTS + 1,
            "n_working_regs must hold at least N_OUTPUTS output regs plus tid",
        );
        Self {
            cfg,
            n_working: cfg.n_working_regs,
            n_pred: 0,
            blocks: Vec::new(),
            prmt_result_regs: Vec::new(),
            set_result_regs: Vec::new(),
            counters: Vec::new(),
        }
    }

    fn alloc_pred(&mut self) -> u32 {
        let p = self.n_pred;
        self.n_pred += 1;
        p
    }

    fn alloc_inst_pred(&mut self, u: &mut Unstructured) -> Result<u32> {
        let pred = self.alloc_pred();
        Ok(
            if self.cfg.emit_negated_predicates && u.arbitrary::<bool>()? {
                pred | NEGATED_PRED_BIT
            } else {
                pred
            },
        )
    }

    /// Register reserved for `%tid.x`. Outside the working-reg pool so the
    /// body can never overwrite it — critical, since the epilogue uses it as
    /// the per-thread address offset. A corrupted tid would write OOB and
    /// poison the CUDA context.
    fn tid_reg(&self) -> u32 {
        self.n_working
    }

    fn alloc_counter(&mut self, init: u32) -> u32 {
        // Counters live above the tid reg.
        let id = self.n_working + 1 + self.counters.len() as u32;
        self.counters.push((id, init));
        id
    }

    fn wide_scratch_hi_reg(&self) -> u32 {
        self.n_working + 1 + self.counters.len() as u32
    }

    fn build(mut self, u: &mut Unstructured) -> Result<String> {
        if self.cfg.control_flow == ControlFlowMode::Structured {
            return self.build_structured(u);
        }

        let min_blocks = self.cfg.min_blocks.max(1);
        let n_blocks = u.int_in_range(min_blocks..=self.cfg.max_blocks.max(min_blocks))?;
        for i in 0..n_blocks {
            let n_insts = u.int_in_range(
                self.cfg.min_insts_per_block
                    ..=self
                        .cfg
                        .max_insts_per_block
                        .max(self.cfg.min_insts_per_block),
            )?;
            let mut insts = Vec::with_capacity(n_insts);
            for _ in 0..n_insts {
                let inst = self.gen_inst(u)?;
                self.note_inst(&inst);
                insts.push(inst);
            }
            let term = if i + 1 == n_blocks {
                Term::Return
            } else {
                self.gen_terminator(u, i, n_blocks)?
            };
            self.blocks.push(Block { insts, term });
        }
        Ok(self.emit())
    }

    fn build_structured(mut self, u: &mut Unstructured) -> Result<String> {
        let min_blocks = self.cfg.min_blocks.max(1);
        let n_blocks = u.int_in_range(min_blocks..=self.cfg.max_blocks.max(min_blocks))?;
        let body = self.gen_structured_seq(u, n_blocks, 0)?;
        Ok(self.emit_structured(&body))
    }

    fn pick_dst(&mut self, u: &mut Unstructured) -> Result<u32> {
        u.int_in_range(0..=self.n_working - 1)
    }

    fn pick_non_output_dst(&mut self, u: &mut Unstructured) -> Result<u32> {
        u.int_in_range(N_OUTPUTS..=self.n_working - 1)
    }

    fn pick_raw_operand(&mut self, u: &mut Unstructured) -> Result<Operand> {
        let pick: u8 = u.arbitrary()?;
        if pick < 192 {
            Ok(Operand::Reg(u.int_in_range(0..=self.n_working - 1)?))
        } else {
            Ok(Operand::Imm(pick_imm32(
                u,
                self.cfg.max_immediate,
                self.cfg.emit_i32_boundary_immediates,
            )?))
        }
    }

    fn pick_operand(&mut self, u: &mut Unstructured) -> Result<Operand> {
        let operand = self.pick_raw_operand(u)?;
        Ok(match operand {
            // m013 is a materialized-boolean fold where ptxas treats a true
            // set.{cmp} result as 1 instead of 0xffffffff. Avoid feeding live
            // set result registers into later value flow.
            Operand::Reg(reg) if self.set_result_regs.contains(&reg) => Operand::Imm(0),
            operand => operand,
        })
    }

    fn pick_reg_operand(&mut self, u: &mut Unstructured) -> Result<Operand> {
        let reg = self.pick_dst(u)?;
        Ok(if self.set_result_regs.contains(&reg) {
            Operand::Reg(0)
        } else {
            Operand::Reg(reg)
        })
    }

    fn pick_safe_reg(&mut self, u: &mut Unstructured) -> Result<u32> {
        let reg = self.pick_dst(u)?;
        Ok(if self.set_result_regs.contains(&reg) {
            0
        } else {
            reg
        })
    }

    fn can_emit_unary(&self) -> bool {
        self.cfg.emit_not
            || self.cfg.emit_clz
            || self.cfg.emit_brev
            || self.cfg.emit_neg
            || self.cfg.emit_cnot
            || self.cfg.emit_popc
            || self.cfg.emit_abs
    }

    fn pick_bin_operand(&mut self, u: &mut Unstructured, op: BinOp) -> Result<Operand> {
        let operand = self.pick_operand(u)?;
        Ok(if op == BinOp::Xor && !self.cfg.emit_not {
            sanitize_xor_not_operand(operand)
        } else {
            operand
        })
    }

    fn pick_cvt_operand(&mut self, u: &mut Unstructured) -> Result<Operand> {
        let operand = self.pick_operand(u)?;
        Ok(match operand {
            // m024 is a `prmt.b32` + narrowing `cvt` fold bug. Avoid feeding
            // prmt result registers into cvt so prmt sweeps do not keep
            // rediscovering that family.
            Operand::Reg(reg) if self.prmt_result_regs.contains(&reg) => Operand::Imm(0),
            operand => operand,
        })
    }

    fn pick_narrow_cvt(&mut self, u: &mut Unstructured) -> Result<Inst> {
        let op = pick_narrow_cvt(u, self.cfg.emit_signed_narrow_cvt)?;
        if self.cfg.emit_predicated_cvt
            && self.cfg.emit_predicated_narrow_cvt
            && u.arbitrary::<bool>()?
        {
            Ok(Inst::PredicatedNarrowCvt {
                op,
                dst: self.pick_dst(u)?,
                src: self.pick_cvt_operand(u)?,
                cmp: pick_cmp(u, self.cfg.emit_signed_cmp)?,
                ca: self.pick_guard_operand(u)?,
                cb: self.pick_guard_operand(u)?,
                pred: self.alloc_inst_pred(u)?,
            })
        } else {
            Ok(Inst::NarrowCvt {
                op,
                dst: self.pick_dst(u)?,
                src: self.pick_cvt_operand(u)?,
            })
        }
    }

    fn pick_wide_cvt(&mut self, u: &mut Unstructured) -> Result<Inst> {
        let op = pick_wide_cvt(u, self.cfg.emit_signed_wide_cvt)?;
        if self.cfg.emit_predicated_cvt
            && self.cfg.emit_predicated_wide_cvt
            && u.arbitrary::<bool>()?
        {
            Ok(Inst::PredicatedWideCvt {
                op,
                dst: self.pick_dst(u)?,
                src: self.pick_cvt_operand(u)?,
                cmp: pick_cmp(u, self.cfg.emit_signed_cmp)?,
                ca: self.pick_guard_operand(u)?,
                cb: self.pick_guard_operand(u)?,
                pred: self.alloc_inst_pred(u)?,
            })
        } else {
            Ok(Inst::WideCvt {
                op,
                dst: self.pick_dst(u)?,
                src: self.pick_cvt_operand(u)?,
            })
        }
    }

    fn pick_bitfield_param(&mut self, u: &mut Unstructured, allow_reg: bool) -> Result<Operand> {
        if allow_reg && u.arbitrary::<bool>()? {
            self.pick_reg_operand(u)
        } else {
            Ok(Operand::Imm(u.int_in_range(0..=31)?))
        }
    }

    fn pick_reg_wide_bitfield_param(
        &mut self,
        u: &mut Unstructured,
    ) -> Result<(Operand, BitfieldParamSlot, u32)> {
        let slot = if u.arbitrary::<bool>()? {
            BitfieldParamSlot::Pos
        } else {
            BitfieldParamSlot::Len
        };
        Ok((self.pick_reg_operand(u)?, slot, u.int_in_range(0..=63)?))
    }

    fn pick_reg_fns_param(&mut self, u: &mut Unstructured) -> Result<(Operand, FnsParamSlot, i32)> {
        let slot = if u.arbitrary::<bool>()? {
            FnsParamSlot::Base
        } else {
            FnsParamSlot::Offset
        };
        let imm = match slot {
            FnsParamSlot::Base => pick_fns_offset(u)?,
            FnsParamSlot::Offset => i32::from(u.int_in_range(0..=31)?),
        };
        Ok((self.pick_reg_operand(u)?, slot, imm))
    }

    fn pick_reg_fns(&mut self, u: &mut Unstructured) -> Result<Inst> {
        let (param, slot, imm) = self.pick_reg_fns_param(u)?;
        if self.cfg.emit_predicated_reg_fns && u.arbitrary::<bool>()? {
            Ok(Inst::PredicatedRegFns {
                dst: self.pick_dst(u)?,
                mask: self.pick_operand(u)?,
                param,
                slot,
                imm,
                cmp: pick_cmp(u, self.cfg.emit_signed_cmp)?,
                ca: self.pick_guard_operand(u)?,
                cb: self.pick_guard_operand(u)?,
                pred: self.alloc_inst_pred(u)?,
            })
        } else {
            Ok(Inst::RegFns {
                dst: self.pick_dst(u)?,
                mask: self.pick_operand(u)?,
                param,
                slot,
                imm,
            })
        }
    }

    fn pick_prmt_ctrl(
        &mut self,
        u: &mut Unstructured,
        mode: Option<PrmtMode>,
        allow_reg: bool,
    ) -> Result<Operand> {
        if allow_reg && self.cfg.emit_bitwise_binops && u.arbitrary::<bool>()? {
            return self.pick_reg_operand(u);
        }

        let max = mode.map_or(0xFFFF, PrmtMode::ctrl_mask);
        Ok(Operand::Imm(u.int_in_range(0..=max)?))
    }

    fn pick_divisor(&mut self, u: &mut Unstructured, op: DivRemOp) -> Result<u32> {
        if op.is_signed() {
            pick_signed_divisor_imm32(u, self.cfg.max_immediate)
        } else {
            pick_nonzero_imm32(
                u,
                self.cfg.max_immediate,
                self.cfg.emit_i32_boundary_immediates,
            )
        }
    }

    fn pick_guard_operand(&mut self, u: &mut Unstructured) -> Result<Operand> {
        self.pick_operand(u)
    }

    fn pick_mad_or_add(&mut self, u: &mut Unstructured) -> Result<Inst> {
        if self.cfg.emit_mul_lo || self.cfg.emit_mad_hi {
            let use_mad_hi = if self.cfg.emit_mul_lo && self.cfg.emit_mad_hi {
                u.arbitrary::<bool>()?
            } else {
                self.cfg.emit_mad_hi
            };
            if use_mad_hi {
                let op = pick_mad_hi(u, self.cfg.emit_signed_mad_hi)?;
                if self.cfg.emit_predicated_mad_hi && u.arbitrary::<bool>()? {
                    return Ok(Inst::PredicatedMadHi {
                        op,
                        dst: self.pick_dst(u)?,
                        a: self.pick_operand(u)?,
                        b: self.pick_operand(u)?,
                        c: self.pick_operand(u)?,
                        cmp: pick_cmp(u, self.cfg.emit_signed_cmp)?,
                        ca: self.pick_guard_operand(u)?,
                        cb: self.pick_guard_operand(u)?,
                        pred: self.alloc_inst_pred(u)?,
                    });
                }
                return Ok(Inst::MadHi {
                    op,
                    dst: self.pick_dst(u)?,
                    a: self.pick_operand(u)?,
                    b: self.pick_operand(u)?,
                    c: self.pick_operand(u)?,
                });
            }
            let signed = self.cfg.emit_signed_lo_alu && u.arbitrary::<bool>()?;
            if self.cfg.emit_predicated_mad && u.arbitrary::<bool>()? {
                Ok(Inst::PredicatedMad {
                    signed,
                    dst: self.pick_dst(u)?,
                    a: self.pick_operand(u)?,
                    b: self.pick_operand(u)?,
                    c: self.pick_operand(u)?,
                    cmp: pick_cmp(u, self.cfg.emit_signed_cmp)?,
                    ca: self.pick_guard_operand(u)?,
                    cb: self.pick_guard_operand(u)?,
                    pred: self.alloc_inst_pred(u)?,
                })
            } else {
                Ok(Inst::Mad {
                    signed,
                    dst: self.pick_dst(u)?,
                    a: self.pick_operand(u)?,
                    b: self.pick_operand(u)?,
                    c: self.pick_operand(u)?,
                })
            }
        } else {
            Ok(Inst::Bin {
                op: BinOp::Add,
                dst: self.pick_dst(u)?,
                a: self.pick_operand(u)?,
                b: self.pick_operand(u)?,
            })
        }
    }

    fn pick_setp_bool_bin(&mut self, u: &mut Unstructured) -> Result<Inst> {
        let op = pick_binop(
            u,
            self.cfg.emit_minmax,
            self.cfg.emit_sub,
            self.cfg.emit_mul_lo,
            self.cfg.emit_signed_lo_alu,
            self.cfg.emit_sat_arith,
            self.cfg.emit_mulhi,
            self.cfg.emit_signed_mulhi,
            self.cfg.emit_bitwise_binops,
            self.cfg.emit_or,
            self.cfg.emit_xor,
        )?;
        Ok(Inst::SetpBoolBin {
            bool_op: pick_predicate_bool_op(u)?,
            base_cmp: pick_cmp(u, self.cfg.emit_signed_cmp)?,
            base_a: self.pick_guard_operand(u)?,
            base_b: self.pick_guard_operand(u)?,
            cmp: pick_cmp(u, self.cfg.emit_signed_cmp)?,
            cmp_a: self.pick_guard_operand(u)?,
            cmp_b: self.pick_guard_operand(u)?,
            base_pred: self.alloc_pred(),
            guard_pred: self.alloc_inst_pred(u)?,
            op,
            dst: self.pick_dst(u)?,
            a: self.pick_bin_operand(u, op)?,
            b: self.pick_bin_operand(u, op)?,
        })
    }

    fn pick_setp_dual_bin(&mut self, u: &mut Unstructured) -> Result<Inst> {
        let true_op = pick_binop(
            u,
            self.cfg.emit_minmax,
            self.cfg.emit_sub,
            self.cfg.emit_mul_lo,
            self.cfg.emit_signed_lo_alu,
            self.cfg.emit_sat_arith,
            self.cfg.emit_mulhi,
            self.cfg.emit_signed_mulhi,
            self.cfg.emit_bitwise_binops,
            self.cfg.emit_or,
            self.cfg.emit_xor,
        )?;
        let false_op = pick_binop(
            u,
            self.cfg.emit_minmax,
            self.cfg.emit_sub,
            self.cfg.emit_mul_lo,
            self.cfg.emit_signed_lo_alu,
            self.cfg.emit_sat_arith,
            self.cfg.emit_mulhi,
            self.cfg.emit_signed_mulhi,
            self.cfg.emit_bitwise_binops,
            self.cfg.emit_or,
            self.cfg.emit_xor,
        )?;
        Ok(Inst::SetpDualBin {
            cmp: pick_cmp(u, self.cfg.emit_signed_cmp)?,
            cmp_a: self.pick_guard_operand(u)?,
            cmp_b: self.pick_guard_operand(u)?,
            true_pred: self.alloc_pred(),
            false_pred: self.alloc_pred(),
            dst: self.pick_dst(u)?,
            true_op,
            true_a: self.pick_bin_operand(u, true_op)?,
            true_b: self.pick_bin_operand(u, true_op)?,
            false_op,
            false_a: self.pick_bin_operand(u, false_op)?,
            false_b: self.pick_bin_operand(u, false_op)?,
        })
    }

    fn pick_pred_logic_bin(&mut self, u: &mut Unstructured) -> Result<Inst> {
        let op = pick_binop(
            u,
            self.cfg.emit_minmax,
            self.cfg.emit_sub,
            self.cfg.emit_mul_lo,
            self.cfg.emit_signed_lo_alu,
            self.cfg.emit_sat_arith,
            self.cfg.emit_mulhi,
            self.cfg.emit_signed_mulhi,
            self.cfg.emit_bitwise_binops,
            self.cfg.emit_or,
            self.cfg.emit_xor,
        )?;
        Ok(Inst::PredLogicBin {
            logic_op: pick_predicate_logic_op(u)?,
            lhs_cmp: pick_cmp(u, self.cfg.emit_signed_cmp)?,
            lhs_a: self.pick_guard_operand(u)?,
            lhs_b: self.pick_guard_operand(u)?,
            rhs_cmp: pick_cmp(u, self.cfg.emit_signed_cmp)?,
            rhs_a: self.pick_guard_operand(u)?,
            rhs_b: self.pick_guard_operand(u)?,
            lhs_pred: self.alloc_pred(),
            rhs_pred: self.alloc_pred(),
            guard_pred: self.alloc_inst_pred(u)?,
            op,
            dst: self.pick_dst(u)?,
            a: self.pick_bin_operand(u, op)?,
            b: self.pick_bin_operand(u, op)?,
        })
    }

    fn pick_wide_setp_bin(&mut self, u: &mut Unstructured) -> Result<Inst> {
        let op = pick_binop(
            u,
            self.cfg.emit_minmax,
            self.cfg.emit_sub,
            self.cfg.emit_mul_lo,
            self.cfg.emit_signed_lo_alu,
            self.cfg.emit_sat_arith,
            self.cfg.emit_mulhi,
            self.cfg.emit_signed_mulhi,
            self.cfg.emit_bitwise_binops,
            self.cfg.emit_or,
            self.cfg.emit_xor,
        )?;
        Ok(Inst::WideSetpBin {
            cmp: pick_cmp(u, self.cfg.emit_signed_cmp)?,
            ca: self.pick_guard_operand(u)?,
            cb: self.pick_guard_operand(u)?,
            pred: self.alloc_inst_pred(u)?,
            op,
            dst: self.pick_dst(u)?,
            a: self.pick_bin_operand(u, op)?,
            b: self.pick_bin_operand(u, op)?,
        })
    }

    fn pick_wide_setp_bool_bin(&mut self, u: &mut Unstructured) -> Result<Inst> {
        let op = pick_binop(
            u,
            self.cfg.emit_minmax,
            self.cfg.emit_sub,
            self.cfg.emit_mul_lo,
            self.cfg.emit_signed_lo_alu,
            self.cfg.emit_sat_arith,
            self.cfg.emit_mulhi,
            self.cfg.emit_signed_mulhi,
            self.cfg.emit_bitwise_binops,
            self.cfg.emit_or,
            self.cfg.emit_xor,
        )?;
        Ok(Inst::WideSetpBoolBin {
            bool_op: pick_predicate_bool_op(u)?,
            base_cmp: pick_cmp(u, self.cfg.emit_signed_cmp)?,
            base_a: self.pick_guard_operand(u)?,
            base_b: self.pick_guard_operand(u)?,
            cmp: pick_cmp(u, self.cfg.emit_signed_cmp)?,
            cmp_a: self.pick_guard_operand(u)?,
            cmp_b: self.pick_guard_operand(u)?,
            base_pred: self.alloc_pred(),
            guard_pred: self.alloc_inst_pred(u)?,
            op,
            dst: self.pick_dst(u)?,
            a: self.pick_bin_operand(u, op)?,
            b: self.pick_bin_operand(u, op)?,
        })
    }

    fn pick_wide_mad64(&mut self, u: &mut Unstructured) -> Result<Inst> {
        let op = pick_wide_mad64(u, self.cfg.emit_signed_wide_mad64)?;
        if self.cfg.emit_predicated_wide_mad64 && u.arbitrary::<bool>()? {
            Ok(Inst::PredicatedWideMad64 {
                op,
                dst: self.pick_dst(u)?,
                a: self.pick_operand(u)?,
                b: self.pick_operand(u)?,
                c: self.pick_operand(u)?,
                cmp: pick_cmp(u, self.cfg.emit_signed_cmp)?,
                ca: self.pick_guard_operand(u)?,
                cb: self.pick_guard_operand(u)?,
                pred: self.alloc_inst_pred(u)?,
            })
        } else {
            Ok(Inst::WideMad64 {
                op,
                dst: self.pick_dst(u)?,
                a: self.pick_operand(u)?,
                b: self.pick_operand(u)?,
                c: self.pick_operand(u)?,
            })
        }
    }

    fn pick_wide_set(&mut self, u: &mut Unstructured) -> Result<Inst> {
        let predicated = self.cfg.emit_predicated_wide_set
            && self.cfg.emit_predicated_set
            && u.arbitrary::<bool>()?;
        if predicated {
            Ok(Inst::PredicatedWideSet {
                dst: self.pick_non_output_dst(u)?,
                cmp: pick_cmp(u, self.cfg.emit_signed_cmp)?,
                a: self.pick_operand(u)?,
                b: self.pick_operand(u)?,
                guard_cmp: pick_cmp(u, self.cfg.emit_signed_cmp)?,
                guard_ca: self.pick_guard_operand(u)?,
                guard_cb: self.pick_guard_operand(u)?,
                guard_pred: self.alloc_inst_pred(u)?,
            })
        } else {
            Ok(Inst::WideSet {
                dst: self.pick_non_output_dst(u)?,
                cmp: pick_cmp(u, self.cfg.emit_signed_cmp)?,
                a: self.pick_operand(u)?,
                b: self.pick_operand(u)?,
            })
        }
    }

    fn pick_wide_carry(&mut self, u: &mut Unstructured) -> Result<Inst> {
        let op = pick_add_carry(u, self.cfg.emit_wide_addc, self.cfg.emit_wide_subc)?;
        if self.cfg.emit_predicated_wide_carry && u.arbitrary::<bool>()? {
            Ok(Inst::PredicatedWideCarry {
                op,
                dst_lo: self.pick_dst(u)?,
                dst_hi: self.pick_dst(u)?,
                a: self.pick_operand(u)?,
                b: self.pick_operand(u)?,
                c: self.pick_operand(u)?,
                d: self.pick_operand(u)?,
                cmp: pick_cmp(u, self.cfg.emit_signed_cmp)?,
                ca: self.pick_guard_operand(u)?,
                cb: self.pick_guard_operand(u)?,
                pred: self.alloc_inst_pred(u)?,
            })
        } else {
            Ok(Inst::WideCarry {
                op,
                dst_lo: self.pick_dst(u)?,
                dst_hi: self.pick_dst(u)?,
                a: self.pick_operand(u)?,
                b: self.pick_operand(u)?,
                c: self.pick_operand(u)?,
                d: self.pick_operand(u)?,
            })
        }
    }

    fn pick_wide_carry_chain(&mut self, u: &mut Unstructured) -> Result<Inst> {
        let op = pick_add_carry(u, self.cfg.emit_wide_addc, self.cfg.emit_wide_subc)?;
        if self.cfg.emit_predicated_wide_carry_chain && u.arbitrary::<bool>()? {
            Ok(Inst::PredicatedWideCarryChain {
                op,
                dst0: self.pick_dst(u)?,
                dst1: self.pick_dst(u)?,
                dst2: self.pick_dst(u)?,
                a: self.pick_operand(u)?,
                b: self.pick_operand(u)?,
                c: self.pick_operand(u)?,
                d: self.pick_operand(u)?,
                e: self.pick_operand(u)?,
                f: self.pick_operand(u)?,
                cmp: pick_cmp(u, self.cfg.emit_signed_cmp)?,
                ca: self.pick_guard_operand(u)?,
                cb: self.pick_guard_operand(u)?,
                pred: self.alloc_inst_pred(u)?,
            })
        } else {
            Ok(Inst::WideCarryChain {
                op,
                dst0: self.pick_dst(u)?,
                dst1: self.pick_dst(u)?,
                dst2: self.pick_dst(u)?,
                a: self.pick_operand(u)?,
                b: self.pick_operand(u)?,
                c: self.pick_operand(u)?,
                d: self.pick_operand(u)?,
                e: self.pick_operand(u)?,
                f: self.pick_operand(u)?,
            })
        }
    }

    fn pick_carry_chain(&mut self, u: &mut Unstructured) -> Result<Inst> {
        let op = pick_add_carry(u, self.cfg.emit_addc, self.cfg.emit_subc)?;
        if self.cfg.emit_predicated_carry_chain && u.arbitrary::<bool>()? {
            Ok(Inst::PredicatedCarryChain {
                op,
                dst0: self.pick_dst(u)?,
                dst1: self.pick_dst(u)?,
                dst2: self.pick_dst(u)?,
                a: self.pick_operand(u)?,
                b: self.pick_operand(u)?,
                c: self.pick_operand(u)?,
                d: self.pick_operand(u)?,
                e: self.pick_operand(u)?,
                f: self.pick_operand(u)?,
                cmp: pick_cmp(u, self.cfg.emit_signed_cmp)?,
                ca: self.pick_guard_operand(u)?,
                cb: self.pick_guard_operand(u)?,
                pred: self.alloc_inst_pred(u)?,
            })
        } else {
            Ok(Inst::CarryChain {
                op,
                dst0: self.pick_dst(u)?,
                dst1: self.pick_dst(u)?,
                dst2: self.pick_dst(u)?,
                a: self.pick_operand(u)?,
                b: self.pick_operand(u)?,
                c: self.pick_operand(u)?,
                d: self.pick_operand(u)?,
                e: self.pick_operand(u)?,
                f: self.pick_operand(u)?,
            })
        }
    }

    fn pick_mad_carry(&mut self, u: &mut Unstructured) -> Result<Inst> {
        let op = pick_mad_carry(u, self.cfg.emit_signed_mad_carry)?;
        if self.cfg.emit_predicated_mad_carry && u.arbitrary::<bool>()? {
            Ok(Inst::PredicatedMadCarry {
                op,
                dst0: self.pick_dst(u)?,
                dst1: self.pick_dst(u)?,
                dst2: self.pick_dst(u)?,
                a: self.pick_operand(u)?,
                b: self.pick_operand(u)?,
                c: self.pick_operand(u)?,
                d: self.pick_operand(u)?,
                e: self.pick_operand(u)?,
                f: self.pick_operand(u)?,
                g: self.pick_operand(u)?,
                h: self.pick_operand(u)?,
                i: self.pick_operand(u)?,
                cmp: pick_cmp(u, self.cfg.emit_signed_cmp)?,
                ca: self.pick_guard_operand(u)?,
                cb: self.pick_guard_operand(u)?,
                pred: self.alloc_inst_pred(u)?,
            })
        } else {
            Ok(Inst::MadCarry {
                op,
                dst0: self.pick_dst(u)?,
                dst1: self.pick_dst(u)?,
                dst2: self.pick_dst(u)?,
                a: self.pick_operand(u)?,
                b: self.pick_operand(u)?,
                c: self.pick_operand(u)?,
                d: self.pick_operand(u)?,
                e: self.pick_operand(u)?,
                f: self.pick_operand(u)?,
                g: self.pick_operand(u)?,
                h: self.pick_operand(u)?,
                i: self.pick_operand(u)?,
            })
        }
    }

    fn pick_subword_wide(&mut self, u: &mut Unstructured) -> Result<Inst> {
        let op = pick_subword_wide(u, self.cfg.emit_signed_subword_wide)?;
        if self.cfg.emit_predicated_subword_wide && u.arbitrary::<bool>()? {
            Ok(Inst::PredicatedSubwordWide {
                op,
                dst: self.pick_dst(u)?,
                a: self.pick_cvt_operand(u)?,
                b: self.pick_cvt_operand(u)?,
                c: self.pick_operand(u)?,
                cmp: pick_cmp(u, self.cfg.emit_signed_cmp)?,
                ca: self.pick_guard_operand(u)?,
                cb: self.pick_guard_operand(u)?,
                pred: self.alloc_inst_pred(u)?,
            })
        } else {
            Ok(Inst::SubwordWide {
                op,
                dst: self.pick_dst(u)?,
                a: self.pick_cvt_operand(u)?,
                b: self.pick_cvt_operand(u)?,
                c: self.pick_operand(u)?,
            })
        }
    }

    fn pick_wide_selp(&mut self, u: &mut Unstructured) -> Result<Inst> {
        Ok(Inst::WideSelp {
            op: pick_selp64(u)?,
            cmp: pick_cmp(u, self.cfg.emit_signed_cmp)?,
            ca: self.pick_guard_operand(u)?,
            cb: self.pick_guard_operand(u)?,
            pred: self.alloc_pred(),
            dst: self.pick_dst(u)?,
            true_value: self.pick_operand(u)?,
            false_value: self.pick_operand(u)?,
        })
    }

    fn pick_wide_unary(&mut self, u: &mut Unstructured) -> Result<Inst> {
        let op = pick_wide_unary(u, self.cfg.emit_signed_wide_unary)?;
        if self.cfg.emit_predicated_wide_unary && u.arbitrary::<bool>()? {
            Ok(Inst::PredicatedWideUnary {
                op,
                dst: self.pick_dst(u)?,
                src: self.pick_operand(u)?,
                cmp: pick_cmp(u, self.cfg.emit_signed_cmp)?,
                ca: self.pick_guard_operand(u)?,
                cb: self.pick_guard_operand(u)?,
                pred: self.alloc_inst_pred(u)?,
            })
        } else {
            Ok(Inst::WideUnary {
                op,
                dst: self.pick_dst(u)?,
                src: self.pick_operand(u)?,
            })
        }
    }

    fn pick_wide_divrem(&mut self, u: &mut Unstructured) -> Result<Inst> {
        let op = pick_wide_divrem(u, self.cfg.emit_signed_wide_divrem)?;
        let predicated = self.cfg.emit_predicated_wide_divrem && u.arbitrary::<bool>()?;
        let allow_reg_divisor = self.cfg.emit_reg_wide_divrem
            && self.cfg.emit_bitwise_binops
            && self.cfg.emit_or
            && (!predicated || self.cfg.emit_predicated_reg_wide_divrem);
        let divisor = self.pick_wide_divisor(u, op, allow_reg_divisor)?;
        if predicated {
            Ok(Inst::PredicatedWideDivRem {
                op,
                dst: self.pick_dst(u)?,
                src: self.pick_operand(u)?,
                divisor,
                cmp: pick_cmp(u, self.cfg.emit_signed_cmp)?,
                ca: self.pick_guard_operand(u)?,
                cb: self.pick_guard_operand(u)?,
                pred: self.alloc_inst_pred(u)?,
            })
        } else {
            Ok(Inst::WideDivRem {
                op,
                dst: self.pick_dst(u)?,
                src: self.pick_operand(u)?,
                divisor,
            })
        }
    }

    fn pick_wide_divisor(
        &mut self,
        u: &mut Unstructured,
        op: WideDivRemOp,
        allow_reg: bool,
    ) -> Result<WideDivisor> {
        if allow_reg && u.arbitrary::<bool>()? {
            return Ok(WideDivisor::Reg(self.pick_reg_operand(u)?));
        }

        let max = self.cfg.max_immediate.max(1).min(i32::MAX as u32);
        let magnitude = i64::from(u.int_in_range(1..=max)?);
        if op.is_signed() && u.arbitrary::<bool>()? {
            Ok(WideDivisor::Imm(-magnitude))
        } else {
            Ok(WideDivisor::Imm(magnitude))
        }
    }

    fn pick_predicate_guard(
        &mut self,
        u: &mut Unstructured,
    ) -> Result<(CmpOp, Operand, Operand, u32)> {
        Ok((
            pick_cmp(u, self.cfg.emit_signed_cmp)?,
            self.pick_guard_operand(u)?,
            self.pick_guard_operand(u)?,
            self.alloc_inst_pred(u)?,
        ))
    }

    fn pick_global_load_cache(&mut self, u: &mut Unstructured) -> Result<GlobalLoadCacheOp> {
        let base_ops = [GlobalLoadCacheOp::Default];
        let cache_ops = [
            GlobalLoadCacheOp::Default,
            GlobalLoadCacheOp::Ca,
            GlobalLoadCacheOp::Cg,
            GlobalLoadCacheOp::Cs,
            GlobalLoadCacheOp::Lu,
            GlobalLoadCacheOp::Cv,
            GlobalLoadCacheOp::Nc,
        ];
        let ops = if self.cfg.emit_memory_cache_ops {
            &cache_ops[..]
        } else {
            &base_ops[..]
        };
        Ok(*u.choose(ops)?)
    }

    fn pick_global_store_cache(&mut self, u: &mut Unstructured) -> Result<GlobalStoreCacheOp> {
        let base_ops = [GlobalStoreCacheOp::Default];
        let cache_ops = [
            GlobalStoreCacheOp::Default,
            GlobalStoreCacheOp::Wb,
            GlobalStoreCacheOp::Cg,
            GlobalStoreCacheOp::Cs,
            GlobalStoreCacheOp::Wt,
        ];
        let ops = if self.cfg.emit_memory_cache_ops {
            &cache_ops[..]
        } else {
            &base_ops[..]
        };
        Ok(*u.choose(ops)?)
    }

    fn pick_volatile_memory(&mut self, u: &mut Unstructured) -> Result<bool> {
        Ok(self.cfg.emit_volatile_memory && u.arbitrary::<bool>()?)
    }

    fn pick_global_load(&mut self, u: &mut Unstructured) -> Result<Inst> {
        let predicated = self.cfg.emit_predicated_memory && u.arbitrary::<bool>()?;
        let allow_narrow_bit =
            self.cfg.emit_bitwise_binops && (!predicated || self.cfg.emit_predicated_alu);
        let base_ops = [
            GlobalLoadOp::U8,
            GlobalLoadOp::S8,
            GlobalLoadOp::U16,
            GlobalLoadOp::S16,
            GlobalLoadOp::U32,
        ];
        let bit_ops = [
            GlobalLoadOp::U8,
            GlobalLoadOp::S8,
            GlobalLoadOp::U16,
            GlobalLoadOp::S16,
            GlobalLoadOp::U32,
            GlobalLoadOp::B8,
            GlobalLoadOp::B16,
            GlobalLoadOp::B32,
        ];
        let bit_no_narrow_ops = [
            GlobalLoadOp::U8,
            GlobalLoadOp::S8,
            GlobalLoadOp::U16,
            GlobalLoadOp::S16,
            GlobalLoadOp::U32,
            GlobalLoadOp::B32,
        ];
        let wide_ops = [
            GlobalLoadOp::U8,
            GlobalLoadOp::S8,
            GlobalLoadOp::U16,
            GlobalLoadOp::S16,
            GlobalLoadOp::U32,
            GlobalLoadOp::U64,
            GlobalLoadOp::S64,
        ];
        let wide_bit_ops = [
            GlobalLoadOp::U8,
            GlobalLoadOp::S8,
            GlobalLoadOp::U16,
            GlobalLoadOp::S16,
            GlobalLoadOp::U32,
            GlobalLoadOp::U64,
            GlobalLoadOp::S64,
            GlobalLoadOp::B8,
            GlobalLoadOp::B16,
            GlobalLoadOp::B32,
            GlobalLoadOp::B64,
        ];
        let wide_bit_no_narrow_ops = [
            GlobalLoadOp::U8,
            GlobalLoadOp::S8,
            GlobalLoadOp::U16,
            GlobalLoadOp::S16,
            GlobalLoadOp::U32,
            GlobalLoadOp::U64,
            GlobalLoadOp::S64,
            GlobalLoadOp::B32,
            GlobalLoadOp::B64,
        ];
        let ops = match (
            self.cfg.emit_wide_memory,
            self.cfg.emit_bit_memory,
            allow_narrow_bit,
        ) {
            (true, true, true) => &wide_bit_ops[..],
            (true, true, false) => &wide_bit_no_narrow_ops[..],
            (true, false, _) => &wide_ops[..],
            (false, true, true) => &bit_ops[..],
            (false, true, false) => &bit_no_narrow_ops[..],
            (false, false, _) => &base_ops[..],
        };
        let op = *u.choose(ops)?;
        let volatile = self.pick_volatile_memory(u)?;
        let uniform = !volatile
            && op.supports_uniform()
            && self.cfg.emit_uniform_global_loads
            && u.int_in_range(0..=3)? == 0;
        let cache = if volatile {
            GlobalLoadCacheOp::Default
        } else if uniform {
            GlobalLoadCacheOp::Default
        } else {
            self.pick_global_load_cache(u)?
        };
        let width = op.width();
        let max_offset = input_len() as u32 - width;
        let offset = u.int_in_range(0..=max_offset / width)? * width;
        let dst = self.pick_dst(u)?;
        if predicated {
            let (cmp, ca, cb, pred) = self.pick_predicate_guard(u)?;
            return Ok(Inst::PredicatedGlobalLoad {
                op,
                cache,
                volatile,
                uniform,
                dst,
                offset,
                cmp,
                ca,
                cb,
                pred,
            });
        }
        Ok(Inst::GlobalLoad {
            op,
            cache,
            volatile,
            uniform,
            dst,
            offset,
        })
    }

    fn pick_global_store_roundtrip(&mut self, u: &mut Unstructured) -> Result<Inst> {
        let predicated = self.cfg.emit_predicated_memory && u.arbitrary::<bool>()?;
        let allow_narrow_bit =
            self.cfg.emit_bitwise_binops && (!predicated || self.cfg.emit_predicated_alu);
        let base_ops = [
            GlobalStoreRoundtripOp::U8,
            GlobalStoreRoundtripOp::S8,
            GlobalStoreRoundtripOp::U16,
            GlobalStoreRoundtripOp::S16,
            GlobalStoreRoundtripOp::U32,
        ];
        let bit_ops = [
            GlobalStoreRoundtripOp::U8,
            GlobalStoreRoundtripOp::S8,
            GlobalStoreRoundtripOp::U16,
            GlobalStoreRoundtripOp::S16,
            GlobalStoreRoundtripOp::U32,
            GlobalStoreRoundtripOp::B8,
            GlobalStoreRoundtripOp::B16,
            GlobalStoreRoundtripOp::B32,
        ];
        let bit_no_narrow_ops = [
            GlobalStoreRoundtripOp::U8,
            GlobalStoreRoundtripOp::S8,
            GlobalStoreRoundtripOp::U16,
            GlobalStoreRoundtripOp::S16,
            GlobalStoreRoundtripOp::U32,
            GlobalStoreRoundtripOp::B32,
        ];
        let wide_ops = [
            GlobalStoreRoundtripOp::U8,
            GlobalStoreRoundtripOp::S8,
            GlobalStoreRoundtripOp::U16,
            GlobalStoreRoundtripOp::S16,
            GlobalStoreRoundtripOp::U32,
            GlobalStoreRoundtripOp::U64,
            GlobalStoreRoundtripOp::S64,
        ];
        let wide_bit_ops = [
            GlobalStoreRoundtripOp::U8,
            GlobalStoreRoundtripOp::S8,
            GlobalStoreRoundtripOp::U16,
            GlobalStoreRoundtripOp::S16,
            GlobalStoreRoundtripOp::U32,
            GlobalStoreRoundtripOp::U64,
            GlobalStoreRoundtripOp::S64,
            GlobalStoreRoundtripOp::B8,
            GlobalStoreRoundtripOp::B16,
            GlobalStoreRoundtripOp::B32,
            GlobalStoreRoundtripOp::B64,
        ];
        let wide_bit_no_narrow_ops = [
            GlobalStoreRoundtripOp::U8,
            GlobalStoreRoundtripOp::S8,
            GlobalStoreRoundtripOp::U16,
            GlobalStoreRoundtripOp::S16,
            GlobalStoreRoundtripOp::U32,
            GlobalStoreRoundtripOp::U64,
            GlobalStoreRoundtripOp::S64,
            GlobalStoreRoundtripOp::B32,
            GlobalStoreRoundtripOp::B64,
        ];
        let ops = match (
            self.cfg.emit_wide_memory,
            self.cfg.emit_bit_memory,
            allow_narrow_bit,
        ) {
            (true, true, true) => &wide_bit_ops[..],
            (true, true, false) => &wide_bit_no_narrow_ops[..],
            (true, false, _) => &wide_ops[..],
            (false, true, true) => &bit_ops[..],
            (false, true, false) => &bit_no_narrow_ops[..],
            (false, false, _) => &base_ops[..],
        };
        let op = *u.choose(ops)?;
        let volatile = self.pick_volatile_memory(u)?;
        let store_cache = if volatile {
            GlobalStoreCacheOp::Default
        } else {
            self.pick_global_store_cache(u)?
        };
        let width = op.width();
        let max_offset = N_OUTPUTS * 4 - width;
        let offset = u.int_in_range(0..=max_offset / width)? * width;
        let dst = self.pick_dst(u)?;
        let src = self.pick_safe_reg(u)?;
        if predicated {
            let (cmp, ca, cb, pred) = self.pick_predicate_guard(u)?;
            return Ok(Inst::PredicatedGlobalStoreRoundtrip {
                op,
                store_cache,
                volatile,
                dst,
                src,
                offset,
                cmp,
                ca,
                cb,
                pred,
            });
        }
        Ok(Inst::GlobalStoreRoundtrip {
            op,
            store_cache,
            volatile,
            dst,
            src,
            offset,
        })
    }

    fn pick_const_load(&mut self, u: &mut Unstructured) -> Result<Inst> {
        let predicated = self.cfg.emit_predicated_memory && u.arbitrary::<bool>()?;
        let allow_narrow_bit =
            self.cfg.emit_bitwise_binops && (!predicated || self.cfg.emit_predicated_alu);
        let base_ops = [
            ConstLoadOp::U8,
            ConstLoadOp::S8,
            ConstLoadOp::U16,
            ConstLoadOp::S16,
            ConstLoadOp::U32,
        ];
        let bit_ops = [
            ConstLoadOp::U8,
            ConstLoadOp::S8,
            ConstLoadOp::U16,
            ConstLoadOp::S16,
            ConstLoadOp::U32,
            ConstLoadOp::B8,
            ConstLoadOp::B16,
            ConstLoadOp::B32,
        ];
        let bit_no_narrow_ops = [
            ConstLoadOp::U8,
            ConstLoadOp::S8,
            ConstLoadOp::U16,
            ConstLoadOp::S16,
            ConstLoadOp::U32,
            ConstLoadOp::B32,
        ];
        let wide_ops = [
            ConstLoadOp::U8,
            ConstLoadOp::S8,
            ConstLoadOp::U16,
            ConstLoadOp::S16,
            ConstLoadOp::U32,
            ConstLoadOp::U64,
            ConstLoadOp::S64,
        ];
        let wide_bit_ops = [
            ConstLoadOp::U8,
            ConstLoadOp::S8,
            ConstLoadOp::U16,
            ConstLoadOp::S16,
            ConstLoadOp::U32,
            ConstLoadOp::U64,
            ConstLoadOp::S64,
            ConstLoadOp::B8,
            ConstLoadOp::B16,
            ConstLoadOp::B32,
            ConstLoadOp::B64,
        ];
        let wide_bit_no_narrow_ops = [
            ConstLoadOp::U8,
            ConstLoadOp::S8,
            ConstLoadOp::U16,
            ConstLoadOp::S16,
            ConstLoadOp::U32,
            ConstLoadOp::U64,
            ConstLoadOp::S64,
            ConstLoadOp::B32,
            ConstLoadOp::B64,
        ];
        let ops = match (
            self.cfg.emit_wide_memory,
            self.cfg.emit_bit_memory,
            allow_narrow_bit,
        ) {
            (true, true, true) => &wide_bit_ops[..],
            (true, true, false) => &wide_bit_no_narrow_ops[..],
            (true, false, _) => &wide_ops[..],
            (false, true, true) => &bit_ops[..],
            (false, true, false) => &bit_no_narrow_ops[..],
            (false, false, _) => &base_ops[..],
        };
        let op = *u.choose(ops)?;
        let width = op.width();
        let max_offset = CONST_MEM_BYTES - width;
        let offset = u.int_in_range(0..=max_offset / width)? * width;
        let dst = self.pick_dst(u)?;
        if predicated {
            let (cmp, ca, cb, pred) = self.pick_predicate_guard(u)?;
            return Ok(Inst::PredicatedConstLoad {
                op,
                dst,
                offset,
                cmp,
                ca,
                cb,
                pred,
            });
        }
        Ok(Inst::ConstLoad { op, dst, offset })
    }

    fn pick_local_mem(&mut self, u: &mut Unstructured) -> Result<Inst> {
        let predicated = self.cfg.emit_predicated_memory && u.arbitrary::<bool>()?;
        let allow_narrow_bit =
            self.cfg.emit_bitwise_binops && (!predicated || self.cfg.emit_predicated_alu);
        let base_ops = [
            LocalMemOp::U8,
            LocalMemOp::S8,
            LocalMemOp::U16,
            LocalMemOp::S16,
            LocalMemOp::U32,
        ];
        let bit_ops = [
            LocalMemOp::U8,
            LocalMemOp::S8,
            LocalMemOp::U16,
            LocalMemOp::S16,
            LocalMemOp::U32,
            LocalMemOp::B8,
            LocalMemOp::B16,
            LocalMemOp::B32,
        ];
        let bit_no_narrow_ops = [
            LocalMemOp::U8,
            LocalMemOp::S8,
            LocalMemOp::U16,
            LocalMemOp::S16,
            LocalMemOp::U32,
            LocalMemOp::B32,
        ];
        let wide_ops = [
            LocalMemOp::U8,
            LocalMemOp::S8,
            LocalMemOp::U16,
            LocalMemOp::S16,
            LocalMemOp::U32,
            LocalMemOp::U64,
            LocalMemOp::S64,
        ];
        let wide_bit_ops = [
            LocalMemOp::U8,
            LocalMemOp::S8,
            LocalMemOp::U16,
            LocalMemOp::S16,
            LocalMemOp::U32,
            LocalMemOp::U64,
            LocalMemOp::S64,
            LocalMemOp::B8,
            LocalMemOp::B16,
            LocalMemOp::B32,
            LocalMemOp::B64,
        ];
        let wide_bit_no_narrow_ops = [
            LocalMemOp::U8,
            LocalMemOp::S8,
            LocalMemOp::U16,
            LocalMemOp::S16,
            LocalMemOp::U32,
            LocalMemOp::U64,
            LocalMemOp::S64,
            LocalMemOp::B32,
            LocalMemOp::B64,
        ];
        let ops = match (
            self.cfg.emit_wide_memory,
            self.cfg.emit_bit_memory,
            allow_narrow_bit,
        ) {
            (true, true, true) => &wide_bit_ops[..],
            (true, true, false) => &wide_bit_no_narrow_ops[..],
            (true, false, _) => &wide_ops[..],
            (false, true, true) => &bit_ops[..],
            (false, true, false) => &bit_no_narrow_ops[..],
            (false, false, _) => &base_ops[..],
        };
        let op = *u.choose(ops)?;
        let width = op.width();
        let max_offset = LOCAL_MEM_BYTES - width;
        let offset = u.int_in_range(0..=max_offset / width)? * width;
        let dst = self.pick_dst(u)?;
        let src = self.pick_safe_reg(u)?;
        if predicated {
            let (cmp, ca, cb, pred) = self.pick_predicate_guard(u)?;
            return Ok(Inst::PredicatedLocalMem {
                op,
                dst,
                src,
                offset,
                cmp,
                ca,
                cb,
                pred,
            });
        }
        Ok(Inst::LocalMem {
            op,
            dst,
            src,
            offset,
        })
    }

    fn pick_shared_mem(&mut self, u: &mut Unstructured) -> Result<Inst> {
        let predicated = self.cfg.emit_predicated_memory && u.arbitrary::<bool>()?;
        let allow_narrow_bit =
            self.cfg.emit_bitwise_binops && (!predicated || self.cfg.emit_predicated_alu);
        let base_ops = [
            SharedMemOp::U8,
            SharedMemOp::S8,
            SharedMemOp::U16,
            SharedMemOp::S16,
            SharedMemOp::U32,
        ];
        let bit_ops = [
            SharedMemOp::U8,
            SharedMemOp::S8,
            SharedMemOp::U16,
            SharedMemOp::S16,
            SharedMemOp::U32,
            SharedMemOp::B8,
            SharedMemOp::B16,
            SharedMemOp::B32,
        ];
        let bit_no_narrow_ops = [
            SharedMemOp::U8,
            SharedMemOp::S8,
            SharedMemOp::U16,
            SharedMemOp::S16,
            SharedMemOp::U32,
            SharedMemOp::B32,
        ];
        let wide_ops = [
            SharedMemOp::U8,
            SharedMemOp::S8,
            SharedMemOp::U16,
            SharedMemOp::S16,
            SharedMemOp::U32,
            SharedMemOp::U64,
            SharedMemOp::S64,
        ];
        let wide_bit_ops = [
            SharedMemOp::U8,
            SharedMemOp::S8,
            SharedMemOp::U16,
            SharedMemOp::S16,
            SharedMemOp::U32,
            SharedMemOp::U64,
            SharedMemOp::S64,
            SharedMemOp::B8,
            SharedMemOp::B16,
            SharedMemOp::B32,
            SharedMemOp::B64,
        ];
        let wide_bit_no_narrow_ops = [
            SharedMemOp::U8,
            SharedMemOp::S8,
            SharedMemOp::U16,
            SharedMemOp::S16,
            SharedMemOp::U32,
            SharedMemOp::U64,
            SharedMemOp::S64,
            SharedMemOp::B32,
            SharedMemOp::B64,
        ];
        let ops = match (
            self.cfg.emit_wide_memory,
            self.cfg.emit_bit_memory,
            allow_narrow_bit,
        ) {
            (true, true, true) => &wide_bit_ops[..],
            (true, true, false) => &wide_bit_no_narrow_ops[..],
            (true, false, _) => &wide_ops[..],
            (false, true, true) => &bit_ops[..],
            (false, true, false) => &bit_no_narrow_ops[..],
            (false, false, _) => &base_ops[..],
        };
        let op = *u.choose(ops)?;
        let width = op.width();
        let max_offset = SHARED_SLOT_BYTES - width;
        let offset = u.int_in_range(0..=max_offset / width)? * width;
        let dst = self.pick_dst(u)?;
        let src = self.pick_safe_reg(u)?;
        let volatile = self.pick_volatile_memory(u)?;
        if predicated {
            let (cmp, ca, cb, pred) = self.pick_predicate_guard(u)?;
            return Ok(Inst::PredicatedSharedMem {
                op,
                volatile,
                dst,
                src,
                offset,
                cmp,
                ca,
                cb,
                pred,
            });
        }
        Ok(Inst::SharedMem {
            op,
            volatile,
            dst,
            src,
            offset,
        })
    }

    fn can_emit_vector_memory(&self) -> bool {
        self.cfg.emit_vector_memory
            && self.n_working - N_OUTPUTS >= 4
            && (self.cfg.emit_global_loads
                || (self.cfg.emit_global_store_roundtrips
                    && self.cfg.emit_mul_wide
                    && self.cfg.emit_wide_int)
                || self.cfg.emit_const_memory
                || self.cfg.emit_local_memory
                || (self.cfg.emit_shared_memory
                    && self.cfg.emit_mul_wide
                    && self.cfg.emit_wide_int))
    }

    fn pick_vector_op(&mut self, u: &mut Unstructured) -> Result<VectorMemOp> {
        let base_ops = [VectorMemOp::V2, VectorMemOp::V4];
        let bit_ops = [
            VectorMemOp::V2,
            VectorMemOp::V4,
            VectorMemOp::V2B32,
            VectorMemOp::V4B32,
        ];
        let wide_ops = [VectorMemOp::V2, VectorMemOp::V4, VectorMemOp::V2U64];
        let wide_bit_ops = [
            VectorMemOp::V2,
            VectorMemOp::V4,
            VectorMemOp::V2U64,
            VectorMemOp::V2B32,
            VectorMemOp::V4B32,
            VectorMemOp::V2B64,
        ];
        let ops = match (self.cfg.emit_wide_memory, self.cfg.emit_bit_memory) {
            (true, true) => &wide_bit_ops[..],
            (true, false) => &wide_ops[..],
            (false, true) => &bit_ops[..],
            (false, false) => &base_ops[..],
        };
        Ok(*u.choose(ops)?)
    }

    fn pick_vector_offset(&mut self, u: &mut Unstructured, bytes: u32, limit: u32) -> Result<u32> {
        let max_offset = limit - bytes;
        Ok(u.int_in_range(0..=max_offset / bytes)? * bytes)
    }

    fn pick_vector_dsts(&mut self, u: &mut Unstructured, lanes: usize) -> Result<[u32; 4]> {
        let mut dsts = [N_OUTPUTS; 4];
        let mut available: Vec<u32> = (N_OUTPUTS..self.n_working).collect();
        for dst in dsts.iter_mut().take(lanes) {
            let idx = u.int_in_range(0..=available.len() - 1)?;
            *dst = available.swap_remove(idx);
        }
        Ok(dsts)
    }

    fn pick_vector_srcs(&mut self, u: &mut Unstructured, lanes: usize) -> Result<[u32; 4]> {
        let mut srcs = [0; 4];
        for src in srcs.iter_mut().take(lanes) {
            *src = self.pick_safe_reg(u)?;
        }
        Ok(srcs)
    }

    fn pick_vector_memory(&mut self, u: &mut Unstructured) -> Result<Inst> {
        let mut spaces = Vec::new();
        if self.cfg.emit_global_loads {
            spaces.push(0);
        }
        if self.cfg.emit_global_store_roundtrips && self.cfg.emit_mul_wide && self.cfg.emit_wide_int
        {
            spaces.push(1);
        }
        if self.cfg.emit_const_memory {
            spaces.push(2);
        }
        if self.cfg.emit_local_memory {
            spaces.push(3);
        }
        if self.cfg.emit_shared_memory && self.cfg.emit_mul_wide && self.cfg.emit_wide_int {
            spaces.push(4);
        }

        let space = *u.choose(&spaces)?;
        let op = self.pick_vector_op(u)?;
        let lanes = op.lanes();
        let bytes = op.bytes();
        let guard = if self.cfg.emit_predicated_memory && u.arbitrary::<bool>()? {
            Some(self.pick_predicate_guard(u)?)
        } else {
            None
        };
        match (space, guard) {
            (0, guard) => {
                let volatile = self.pick_volatile_memory(u)?;
                let uniform = !volatile
                    && op.supports_uniform_global_load()
                    && self.cfg.emit_uniform_global_loads
                    && u.int_in_range(0..=3)? == 0;
                let cache = if volatile || uniform {
                    GlobalLoadCacheOp::Default
                } else {
                    self.pick_global_load_cache(u)?
                };
                let dsts = self.pick_vector_dsts(u, lanes)?;
                let offset = self.pick_vector_offset(u, bytes, input_len() as u32)?;
                if let Some((cmp, ca, cb, pred)) = guard {
                    Ok(Inst::PredicatedGlobalVectorLoad {
                        op,
                        volatile,
                        uniform,
                        cache,
                        dsts,
                        offset,
                        cmp,
                        ca,
                        cb,
                        pred,
                    })
                } else {
                    Ok(Inst::GlobalVectorLoad {
                        op,
                        volatile,
                        uniform,
                        cache,
                        dsts,
                        offset,
                    })
                }
            }
            (1, Some((cmp, ca, cb, pred))) => Ok(Inst::PredicatedGlobalVectorStoreRoundtrip {
                op,
                volatile: self.pick_volatile_memory(u)?,
                store_cache: self.pick_global_store_cache(u)?,
                dsts: self.pick_vector_dsts(u, lanes)?,
                srcs: self.pick_vector_srcs(u, lanes)?,
                offset: self.pick_vector_offset(u, bytes, N_OUTPUTS * 4)?,
                cmp,
                ca,
                cb,
                pred,
            }),
            (1, None) => Ok(Inst::GlobalVectorStoreRoundtrip {
                op,
                volatile: self.pick_volatile_memory(u)?,
                store_cache: self.pick_global_store_cache(u)?,
                dsts: self.pick_vector_dsts(u, lanes)?,
                srcs: self.pick_vector_srcs(u, lanes)?,
                offset: self.pick_vector_offset(u, bytes, N_OUTPUTS * 4)?,
            }),
            (2, Some((cmp, ca, cb, pred))) => Ok(Inst::PredicatedConstVectorLoad {
                op,
                dsts: self.pick_vector_dsts(u, lanes)?,
                offset: self.pick_vector_offset(u, bytes, CONST_MEM_BYTES)?,
                cmp,
                ca,
                cb,
                pred,
            }),
            (2, None) => Ok(Inst::ConstVectorLoad {
                op,
                dsts: self.pick_vector_dsts(u, lanes)?,
                offset: self.pick_vector_offset(u, bytes, CONST_MEM_BYTES)?,
            }),
            (3, Some((cmp, ca, cb, pred))) => Ok(Inst::PredicatedLocalVectorMem {
                op,
                dsts: self.pick_vector_dsts(u, lanes)?,
                srcs: self.pick_vector_srcs(u, lanes)?,
                offset: self.pick_vector_offset(u, bytes, LOCAL_MEM_BYTES)?,
                cmp,
                ca,
                cb,
                pred,
            }),
            (3, None) => Ok(Inst::LocalVectorMem {
                op,
                dsts: self.pick_vector_dsts(u, lanes)?,
                srcs: self.pick_vector_srcs(u, lanes)?,
                offset: self.pick_vector_offset(u, bytes, LOCAL_MEM_BYTES)?,
            }),
            (4, Some((cmp, ca, cb, pred))) => Ok(Inst::PredicatedSharedVectorMem {
                op,
                volatile: self.pick_volatile_memory(u)?,
                dsts: self.pick_vector_dsts(u, lanes)?,
                srcs: self.pick_vector_srcs(u, lanes)?,
                offset: self.pick_vector_offset(u, bytes, SHARED_SLOT_BYTES)?,
                cmp,
                ca,
                cb,
                pred,
            }),
            (4, None) => Ok(Inst::SharedVectorMem {
                op,
                volatile: self.pick_volatile_memory(u)?,
                dsts: self.pick_vector_dsts(u, lanes)?,
                srcs: self.pick_vector_srcs(u, lanes)?,
                offset: self.pick_vector_offset(u, bytes, SHARED_SLOT_BYTES)?,
            }),
            _ => unreachable!(),
        }
    }

    fn pick_f32_arith(&mut self, u: &mut Unstructured) -> Result<Inst> {
        let ops = [
            F32ArithOp::Add,
            F32ArithOp::Sub,
            F32ArithOp::Mul,
            F32ArithOp::Div,
            F32ArithOp::DivApprox,
            F32ArithOp::Fma,
            F32ArithOp::AddSat,
            F32ArithOp::SubSat,
            F32ArithOp::MulSat,
            F32ArithOp::FmaSat,
            F32ArithOp::Copysign,
            F32ArithOp::Min,
            F32ArithOp::Max,
            F32ArithOp::MinFtz,
            F32ArithOp::MaxFtz,
        ];
        let op = *u.choose(&ops)?;
        if self.cfg.emit_predicated_alu && u.arbitrary::<bool>()? {
            Ok(Inst::PredicatedF32Arith {
                op,
                dst: self.pick_dst(u)?,
                a: self.pick_cvt_operand(u)?,
                b: self.pick_cvt_operand(u)?,
                c: self.pick_cvt_operand(u)?,
                cmp: pick_cmp(u, self.cfg.emit_signed_cmp)?,
                ca: self.pick_guard_operand(u)?,
                cb: self.pick_guard_operand(u)?,
                pred: self.alloc_inst_pred(u)?,
            })
        } else {
            Ok(Inst::F32Arith {
                op,
                dst: self.pick_dst(u)?,
                a: self.pick_cvt_operand(u)?,
                b: self.pick_cvt_operand(u)?,
                c: self.pick_cvt_operand(u)?,
            })
        }
    }

    fn pick_f32_rounding_arith(&mut self, u: &mut Unstructured) -> Result<Inst> {
        let ops = [
            F32RoundingArithOp::AddRz,
            F32RoundingArithOp::AddRm,
            F32RoundingArithOp::AddRp,
            F32RoundingArithOp::AddRnFtz,
            F32RoundingArithOp::AddRzFtz,
            F32RoundingArithOp::AddRmFtz,
            F32RoundingArithOp::AddRpFtz,
            F32RoundingArithOp::SubRz,
            F32RoundingArithOp::SubRm,
            F32RoundingArithOp::SubRp,
            F32RoundingArithOp::SubRnFtz,
            F32RoundingArithOp::SubRzFtz,
            F32RoundingArithOp::SubRmFtz,
            F32RoundingArithOp::SubRpFtz,
            F32RoundingArithOp::MulRz,
            F32RoundingArithOp::MulRm,
            F32RoundingArithOp::MulRp,
            F32RoundingArithOp::MulRnFtz,
            F32RoundingArithOp::MulRzFtz,
            F32RoundingArithOp::MulRmFtz,
            F32RoundingArithOp::MulRpFtz,
            F32RoundingArithOp::DivRz,
            F32RoundingArithOp::DivRm,
            F32RoundingArithOp::DivRp,
            F32RoundingArithOp::DivRnFtz,
            F32RoundingArithOp::DivRzFtz,
            F32RoundingArithOp::DivRmFtz,
            F32RoundingArithOp::DivRpFtz,
            F32RoundingArithOp::FmaRz,
            F32RoundingArithOp::FmaRm,
            F32RoundingArithOp::FmaRp,
            F32RoundingArithOp::FmaRnFtz,
            F32RoundingArithOp::FmaRzFtz,
            F32RoundingArithOp::FmaRmFtz,
            F32RoundingArithOp::FmaRpFtz,
        ];
        let op = *u.choose(&ops)?;
        if self.cfg.emit_predicated_alu && u.arbitrary::<bool>()? {
            Ok(Inst::PredicatedF32RoundingArith {
                op,
                dst: self.pick_dst(u)?,
                a: self.pick_cvt_operand(u)?,
                b: self.pick_cvt_operand(u)?,
                c: self.pick_cvt_operand(u)?,
                cmp: pick_cmp(u, self.cfg.emit_signed_cmp)?,
                ca: self.pick_guard_operand(u)?,
                cb: self.pick_guard_operand(u)?,
                pred: self.alloc_inst_pred(u)?,
            })
        } else {
            Ok(Inst::F32RoundingArith {
                op,
                dst: self.pick_dst(u)?,
                a: self.pick_cvt_operand(u)?,
                b: self.pick_cvt_operand(u)?,
                c: self.pick_cvt_operand(u)?,
            })
        }
    }

    fn pick_f32_unary(&mut self, u: &mut Unstructured) -> Result<Inst> {
        let ops = [
            F32UnaryOp::Abs,
            F32UnaryOp::Neg,
            F32UnaryOp::AbsFtz,
            F32UnaryOp::NegFtz,
        ];
        let op = *u.choose(&ops)?;
        if self.cfg.emit_predicated_unary && u.arbitrary::<bool>()? {
            Ok(Inst::PredicatedF32Unary {
                op,
                dst: self.pick_dst(u)?,
                src: self.pick_cvt_operand(u)?,
                cmp: pick_cmp(u, self.cfg.emit_signed_cmp)?,
                ca: self.pick_guard_operand(u)?,
                cb: self.pick_guard_operand(u)?,
                pred: self.alloc_inst_pred(u)?,
            })
        } else {
            Ok(Inst::F32Unary {
                op,
                dst: self.pick_dst(u)?,
                src: self.pick_cvt_operand(u)?,
            })
        }
    }

    fn pick_f32_cvt(&mut self, u: &mut Unstructured) -> Result<Inst> {
        if u.arbitrary::<bool>()? {
            let ops = [
                F32FromF64CvtOp::Rn,
                F32FromF64CvtOp::Rz,
                F32FromF64CvtOp::Rm,
                F32FromF64CvtOp::Rp,
                F32FromF64CvtOp::RnFtz,
                F32FromF64CvtOp::RzFtz,
                F32FromF64CvtOp::RmFtz,
                F32FromF64CvtOp::RpFtz,
            ];
            let op = *u.choose(&ops)?;
            if self.cfg.emit_predicated_cvt && u.arbitrary::<bool>()? {
                return Ok(Inst::PredicatedF32FloatCvt {
                    op,
                    dst: self.pick_dst(u)?,
                    src: self.pick_cvt_operand(u)?,
                    cmp: pick_cmp(u, self.cfg.emit_signed_cmp)?,
                    ca: self.pick_guard_operand(u)?,
                    cb: self.pick_guard_operand(u)?,
                    pred: self.alloc_inst_pred(u)?,
                });
            }
            return Ok(Inst::F32FloatCvt {
                op,
                dst: self.pick_dst(u)?,
                src: self.pick_cvt_operand(u)?,
            });
        }

        let from_ops = [
            F32FromIntCvtOp::U32Rn,
            F32FromIntCvtOp::U32Rz,
            F32FromIntCvtOp::U32Rm,
            F32FromIntCvtOp::U32Rp,
            F32FromIntCvtOp::U32RnFtz,
            F32FromIntCvtOp::U32RzFtz,
            F32FromIntCvtOp::U32RmFtz,
            F32FromIntCvtOp::U32RpFtz,
            F32FromIntCvtOp::S32Rn,
            F32FromIntCvtOp::S32Rz,
            F32FromIntCvtOp::S32Rm,
            F32FromIntCvtOp::S32Rp,
            F32FromIntCvtOp::S32RnFtz,
            F32FromIntCvtOp::S32RzFtz,
            F32FromIntCvtOp::S32RmFtz,
            F32FromIntCvtOp::S32RpFtz,
            F32FromIntCvtOp::U64Rn,
            F32FromIntCvtOp::U64Rz,
            F32FromIntCvtOp::U64Rm,
            F32FromIntCvtOp::U64Rp,
            F32FromIntCvtOp::U64RnFtz,
            F32FromIntCvtOp::U64RzFtz,
            F32FromIntCvtOp::U64RmFtz,
            F32FromIntCvtOp::U64RpFtz,
            F32FromIntCvtOp::S64Rn,
            F32FromIntCvtOp::S64Rz,
            F32FromIntCvtOp::S64Rm,
            F32FromIntCvtOp::S64Rp,
            F32FromIntCvtOp::S64RnFtz,
            F32FromIntCvtOp::S64RzFtz,
            F32FromIntCvtOp::S64RmFtz,
            F32FromIntCvtOp::S64RpFtz,
        ];
        let to_ops = [
            F32ToIntCvtOp::S32Rzi,
            F32ToIntCvtOp::S32Rni,
            F32ToIntCvtOp::S32Rmi,
            F32ToIntCvtOp::S32Rpi,
            F32ToIntCvtOp::S32RziFtz,
            F32ToIntCvtOp::S32RniFtz,
            F32ToIntCvtOp::S32RmiFtz,
            F32ToIntCvtOp::S32RpiFtz,
            F32ToIntCvtOp::U32Rzi,
            F32ToIntCvtOp::U32Rni,
            F32ToIntCvtOp::U32Rmi,
            F32ToIntCvtOp::U32Rpi,
            F32ToIntCvtOp::U32RziFtz,
            F32ToIntCvtOp::U32RniFtz,
            F32ToIntCvtOp::U32RmiFtz,
            F32ToIntCvtOp::U32RpiFtz,
            F32ToIntCvtOp::S32RziSat,
            F32ToIntCvtOp::S32RniSat,
            F32ToIntCvtOp::S32RmiSat,
            F32ToIntCvtOp::S32RpiSat,
            F32ToIntCvtOp::S32RziFtzSat,
            F32ToIntCvtOp::S32RniFtzSat,
            F32ToIntCvtOp::S32RmiFtzSat,
            F32ToIntCvtOp::S32RpiFtzSat,
            F32ToIntCvtOp::U32RziSat,
            F32ToIntCvtOp::U32RniSat,
            F32ToIntCvtOp::U32RmiSat,
            F32ToIntCvtOp::U32RpiSat,
            F32ToIntCvtOp::U32RziFtzSat,
            F32ToIntCvtOp::U32RniFtzSat,
            F32ToIntCvtOp::U32RmiFtzSat,
            F32ToIntCvtOp::U32RpiFtzSat,
            F32ToIntCvtOp::S64Rzi,
            F32ToIntCvtOp::S64Rni,
            F32ToIntCvtOp::S64Rmi,
            F32ToIntCvtOp::S64Rpi,
            F32ToIntCvtOp::S64RziFtz,
            F32ToIntCvtOp::S64RniFtz,
            F32ToIntCvtOp::S64RmiFtz,
            F32ToIntCvtOp::S64RpiFtz,
            F32ToIntCvtOp::U64Rzi,
            F32ToIntCvtOp::U64Rni,
            F32ToIntCvtOp::U64Rmi,
            F32ToIntCvtOp::U64Rpi,
            F32ToIntCvtOp::U64RziFtz,
            F32ToIntCvtOp::U64RniFtz,
            F32ToIntCvtOp::U64RmiFtz,
            F32ToIntCvtOp::U64RpiFtz,
            F32ToIntCvtOp::S64RziSat,
            F32ToIntCvtOp::S64RniSat,
            F32ToIntCvtOp::S64RmiSat,
            F32ToIntCvtOp::S64RpiSat,
            F32ToIntCvtOp::S64RziFtzSat,
            F32ToIntCvtOp::S64RniFtzSat,
            F32ToIntCvtOp::S64RmiFtzSat,
            F32ToIntCvtOp::S64RpiFtzSat,
            F32ToIntCvtOp::U64RziSat,
            F32ToIntCvtOp::U64RniSat,
            F32ToIntCvtOp::U64RmiSat,
            F32ToIntCvtOp::U64RpiSat,
            F32ToIntCvtOp::U64RziFtzSat,
            F32ToIntCvtOp::U64RniFtzSat,
            F32ToIntCvtOp::U64RmiFtzSat,
            F32ToIntCvtOp::U64RpiFtzSat,
        ];
        let from_int = *u.choose(&from_ops)?;
        let to_int = *u.choose(&to_ops)?;
        if self.cfg.emit_predicated_cvt && u.arbitrary::<bool>()? {
            Ok(Inst::PredicatedF32Cvt {
                from_int,
                to_int,
                dst: self.pick_dst(u)?,
                src: self.pick_cvt_operand(u)?,
                cmp: pick_cmp(u, self.cfg.emit_signed_cmp)?,
                ca: self.pick_guard_operand(u)?,
                cb: self.pick_guard_operand(u)?,
                pred: self.alloc_inst_pred(u)?,
            })
        } else {
            Ok(Inst::F32Cvt {
                from_int,
                to_int,
                dst: self.pick_dst(u)?,
                src: self.pick_cvt_operand(u)?,
            })
        }
    }

    fn pick_f32_special_math(&mut self, u: &mut Unstructured) -> Result<Inst> {
        let ops = [
            F32SpecialMathOp::SqrtRn,
            F32SpecialMathOp::SqrtRz,
            F32SpecialMathOp::SqrtRm,
            F32SpecialMathOp::SqrtRp,
            F32SpecialMathOp::SqrtRnFtz,
            F32SpecialMathOp::SqrtRzFtz,
            F32SpecialMathOp::SqrtRmFtz,
            F32SpecialMathOp::SqrtRpFtz,
            F32SpecialMathOp::RcpRn,
            F32SpecialMathOp::RcpRz,
            F32SpecialMathOp::RcpRm,
            F32SpecialMathOp::RcpRp,
            F32SpecialMathOp::RcpRnFtz,
            F32SpecialMathOp::RcpRzFtz,
            F32SpecialMathOp::RcpRmFtz,
            F32SpecialMathOp::RcpRpFtz,
            F32SpecialMathOp::RcpApprox,
            F32SpecialMathOp::RsqrtApprox,
            F32SpecialMathOp::Ex2Approx,
            F32SpecialMathOp::Lg2Approx,
            F32SpecialMathOp::SinApprox,
            F32SpecialMathOp::CosApprox,
        ];
        let op = *u.choose(&ops)?;
        if self.cfg.emit_predicated_unary && u.arbitrary::<bool>()? {
            Ok(Inst::PredicatedF32SpecialMath {
                op,
                dst: self.pick_dst(u)?,
                src: self.pick_cvt_operand(u)?,
                cmp: pick_cmp(u, self.cfg.emit_signed_cmp)?,
                ca: self.pick_guard_operand(u)?,
                cb: self.pick_guard_operand(u)?,
                pred: self.alloc_inst_pred(u)?,
            })
        } else {
            Ok(Inst::F32SpecialMath {
                op,
                dst: self.pick_dst(u)?,
                src: self.pick_cvt_operand(u)?,
            })
        }
    }

    fn pick_float_testp_op(&mut self, u: &mut Unstructured) -> Result<FloatTestpOp> {
        let ops = [
            FloatTestpOp::Finite,
            FloatTestpOp::Infinite,
            FloatTestpOp::Number,
            FloatTestpOp::NotANumber,
            FloatTestpOp::Normal,
            FloatTestpOp::Subnormal,
        ];
        Ok(*u.choose(&ops)?)
    }

    fn pick_f32_set(&mut self, u: &mut Unstructured) -> Result<Inst> {
        if self.cfg.emit_predicated_set && u.arbitrary::<bool>()? {
            Ok(Inst::PredicatedF32Set {
                cmp: pick_f32_cmp(u)?,
                dst: self.pick_non_output_dst(u)?,
                a: self.pick_cvt_operand(u)?,
                b: self.pick_cvt_operand(u)?,
                guard_cmp: pick_cmp(u, self.cfg.emit_signed_cmp)?,
                guard_ca: self.pick_guard_operand(u)?,
                guard_cb: self.pick_guard_operand(u)?,
                guard_pred: self.alloc_inst_pred(u)?,
            })
        } else {
            Ok(Inst::F32Set {
                cmp: pick_f32_cmp(u)?,
                dst: self.pick_non_output_dst(u)?,
                a: self.pick_cvt_operand(u)?,
                b: self.pick_cvt_operand(u)?,
            })
        }
    }

    fn pick_f32_setp_bool(&mut self, u: &mut Unstructured) -> Result<Inst> {
        if self.cfg.emit_predicated_set && u.arbitrary::<bool>()? {
            Ok(Inst::PredicatedF32SetpBool {
                bool_op: pick_predicate_bool_op(u)?,
                base_cmp: pick_cmp(u, self.cfg.emit_signed_cmp)?,
                base_a: self.pick_guard_operand(u)?,
                base_b: self.pick_guard_operand(u)?,
                cmp: pick_f32_cmp(u)?,
                dst: self.pick_non_output_dst(u)?,
                a: self.pick_cvt_operand(u)?,
                b: self.pick_cvt_operand(u)?,
                base_pred: self.alloc_pred(),
                pred: self.alloc_pred(),
                guard_cmp: pick_cmp(u, self.cfg.emit_signed_cmp)?,
                guard_ca: self.pick_guard_operand(u)?,
                guard_cb: self.pick_guard_operand(u)?,
                guard_pred: self.alloc_inst_pred(u)?,
            })
        } else {
            Ok(Inst::F32SetpBool {
                bool_op: pick_predicate_bool_op(u)?,
                base_cmp: pick_cmp(u, self.cfg.emit_signed_cmp)?,
                base_a: self.pick_guard_operand(u)?,
                base_b: self.pick_guard_operand(u)?,
                cmp: pick_f32_cmp(u)?,
                dst: self.pick_non_output_dst(u)?,
                a: self.pick_cvt_operand(u)?,
                b: self.pick_cvt_operand(u)?,
                base_pred: self.alloc_pred(),
                pred: self.alloc_pred(),
            })
        }
    }

    fn pick_f32_testp(&mut self, u: &mut Unstructured) -> Result<Inst> {
        if self.cfg.emit_predicated_set && u.arbitrary::<bool>()? {
            Ok(Inst::PredicatedF32Testp {
                op: self.pick_float_testp_op(u)?,
                dst: self.pick_non_output_dst(u)?,
                src: self.pick_reg_operand(u)?,
                pred: self.alloc_pred(),
                guard_cmp: pick_cmp(u, self.cfg.emit_signed_cmp)?,
                guard_ca: self.pick_guard_operand(u)?,
                guard_cb: self.pick_guard_operand(u)?,
                guard_pred: self.alloc_inst_pred(u)?,
            })
        } else {
            Ok(Inst::F32Testp {
                op: self.pick_float_testp_op(u)?,
                dst: self.pick_non_output_dst(u)?,
                src: self.pick_reg_operand(u)?,
                pred: self.alloc_pred(),
            })
        }
    }

    fn pick_f32_selp(&mut self, u: &mut Unstructured) -> Result<Inst> {
        if self.cfg.emit_predicated_selp && u.arbitrary::<bool>()? {
            Ok(Inst::PredicatedF32Selp {
                cmp: pick_f32_cmp(u)?,
                dst: self.pick_dst(u)?,
                a: self.pick_cvt_operand(u)?,
                b: self.pick_cvt_operand(u)?,
                pred: self.alloc_pred(),
                guard_cmp: pick_cmp(u, self.cfg.emit_signed_cmp)?,
                guard_ca: self.pick_guard_operand(u)?,
                guard_cb: self.pick_guard_operand(u)?,
                guard_pred: self.alloc_inst_pred(u)?,
            })
        } else {
            Ok(Inst::F32Selp {
                cmp: pick_f32_cmp(u)?,
                dst: self.pick_dst(u)?,
                a: self.pick_cvt_operand(u)?,
                b: self.pick_cvt_operand(u)?,
                pred: self.alloc_pred(),
            })
        }
    }

    fn pick_f64_arith(&mut self, u: &mut Unstructured) -> Result<Inst> {
        let ops = [
            F64ArithOp::Add,
            F64ArithOp::Sub,
            F64ArithOp::Mul,
            F64ArithOp::Div,
            F64ArithOp::Fma,
            F64ArithOp::Copysign,
            F64ArithOp::Min,
            F64ArithOp::Max,
        ];
        let op = *u.choose(&ops)?;
        if self.cfg.emit_predicated_alu && u.arbitrary::<bool>()? {
            Ok(Inst::PredicatedF64Arith {
                op,
                dst: self.pick_dst(u)?,
                a: self.pick_cvt_operand(u)?,
                b: self.pick_cvt_operand(u)?,
                c: self.pick_cvt_operand(u)?,
                cmp: pick_cmp(u, self.cfg.emit_signed_cmp)?,
                ca: self.pick_guard_operand(u)?,
                cb: self.pick_guard_operand(u)?,
                pred: self.alloc_inst_pred(u)?,
            })
        } else {
            Ok(Inst::F64Arith {
                op,
                dst: self.pick_dst(u)?,
                a: self.pick_cvt_operand(u)?,
                b: self.pick_cvt_operand(u)?,
                c: self.pick_cvt_operand(u)?,
            })
        }
    }

    fn pick_f64_rounding_arith(&mut self, u: &mut Unstructured) -> Result<Inst> {
        let ops = [
            F64RoundingArithOp::AddRz,
            F64RoundingArithOp::AddRm,
            F64RoundingArithOp::AddRp,
            F64RoundingArithOp::SubRz,
            F64RoundingArithOp::SubRm,
            F64RoundingArithOp::SubRp,
            F64RoundingArithOp::MulRz,
            F64RoundingArithOp::MulRm,
            F64RoundingArithOp::MulRp,
            F64RoundingArithOp::DivRz,
            F64RoundingArithOp::DivRm,
            F64RoundingArithOp::DivRp,
            F64RoundingArithOp::FmaRz,
            F64RoundingArithOp::FmaRm,
            F64RoundingArithOp::FmaRp,
        ];
        let op = *u.choose(&ops)?;
        if self.cfg.emit_predicated_alu && u.arbitrary::<bool>()? {
            Ok(Inst::PredicatedF64RoundingArith {
                op,
                dst: self.pick_dst(u)?,
                a: self.pick_cvt_operand(u)?,
                b: self.pick_cvt_operand(u)?,
                c: self.pick_cvt_operand(u)?,
                cmp: pick_cmp(u, self.cfg.emit_signed_cmp)?,
                ca: self.pick_guard_operand(u)?,
                cb: self.pick_guard_operand(u)?,
                pred: self.alloc_inst_pred(u)?,
            })
        } else {
            Ok(Inst::F64RoundingArith {
                op,
                dst: self.pick_dst(u)?,
                a: self.pick_cvt_operand(u)?,
                b: self.pick_cvt_operand(u)?,
                c: self.pick_cvt_operand(u)?,
            })
        }
    }

    fn pick_f64_unary(&mut self, u: &mut Unstructured) -> Result<Inst> {
        let ops = [F64UnaryOp::Abs, F64UnaryOp::Neg];
        let op = *u.choose(&ops)?;
        if self.cfg.emit_predicated_unary && u.arbitrary::<bool>()? {
            Ok(Inst::PredicatedF64Unary {
                op,
                dst: self.pick_dst(u)?,
                src: self.pick_cvt_operand(u)?,
                cmp: pick_cmp(u, self.cfg.emit_signed_cmp)?,
                ca: self.pick_guard_operand(u)?,
                cb: self.pick_guard_operand(u)?,
                pred: self.alloc_inst_pred(u)?,
            })
        } else {
            Ok(Inst::F64Unary {
                op,
                dst: self.pick_dst(u)?,
                src: self.pick_cvt_operand(u)?,
            })
        }
    }

    fn pick_f64_cvt(&mut self, u: &mut Unstructured) -> Result<Inst> {
        if u.arbitrary::<bool>()? {
            if self.cfg.emit_predicated_cvt && u.arbitrary::<bool>()? {
                return Ok(Inst::PredicatedF64FloatCvt {
                    op: F64FromF32CvtOp::Default,
                    dst: self.pick_dst(u)?,
                    src: self.pick_cvt_operand(u)?,
                    cmp: pick_cmp(u, self.cfg.emit_signed_cmp)?,
                    ca: self.pick_guard_operand(u)?,
                    cb: self.pick_guard_operand(u)?,
                    pred: self.alloc_inst_pred(u)?,
                });
            }
            return Ok(Inst::F64FloatCvt {
                op: F64FromF32CvtOp::Default,
                dst: self.pick_dst(u)?,
                src: self.pick_cvt_operand(u)?,
            });
        }

        let from_ops = [
            F64FromIntCvtOp::U32Rn,
            F64FromIntCvtOp::U32Rz,
            F64FromIntCvtOp::U32Rm,
            F64FromIntCvtOp::U32Rp,
            F64FromIntCvtOp::S32Rn,
            F64FromIntCvtOp::S32Rz,
            F64FromIntCvtOp::S32Rm,
            F64FromIntCvtOp::S32Rp,
            F64FromIntCvtOp::U64Rn,
            F64FromIntCvtOp::U64Rz,
            F64FromIntCvtOp::U64Rm,
            F64FromIntCvtOp::U64Rp,
            F64FromIntCvtOp::S64Rn,
            F64FromIntCvtOp::S64Rz,
            F64FromIntCvtOp::S64Rm,
            F64FromIntCvtOp::S64Rp,
        ];
        let to_ops = [
            F64ToIntCvtOp::S32Rzi,
            F64ToIntCvtOp::S32Rni,
            F64ToIntCvtOp::S32Rmi,
            F64ToIntCvtOp::S32Rpi,
            F64ToIntCvtOp::U32Rzi,
            F64ToIntCvtOp::U32Rni,
            F64ToIntCvtOp::U32Rmi,
            F64ToIntCvtOp::U32Rpi,
            F64ToIntCvtOp::S32RziSat,
            F64ToIntCvtOp::S32RniSat,
            F64ToIntCvtOp::S32RmiSat,
            F64ToIntCvtOp::S32RpiSat,
            F64ToIntCvtOp::U32RziSat,
            F64ToIntCvtOp::U32RniSat,
            F64ToIntCvtOp::U32RmiSat,
            F64ToIntCvtOp::U32RpiSat,
            F64ToIntCvtOp::S64Rzi,
            F64ToIntCvtOp::S64Rni,
            F64ToIntCvtOp::S64Rmi,
            F64ToIntCvtOp::S64Rpi,
            F64ToIntCvtOp::U64Rzi,
            F64ToIntCvtOp::U64Rni,
            F64ToIntCvtOp::U64Rmi,
            F64ToIntCvtOp::U64Rpi,
            F64ToIntCvtOp::S64RziSat,
            F64ToIntCvtOp::S64RniSat,
            F64ToIntCvtOp::S64RmiSat,
            F64ToIntCvtOp::S64RpiSat,
            F64ToIntCvtOp::U64RziSat,
            F64ToIntCvtOp::U64RniSat,
            F64ToIntCvtOp::U64RmiSat,
            F64ToIntCvtOp::U64RpiSat,
        ];
        let from_int = *u.choose(&from_ops)?;
        let to_int = *u.choose(&to_ops)?;
        if self.cfg.emit_predicated_cvt && u.arbitrary::<bool>()? {
            Ok(Inst::PredicatedF64Cvt {
                from_int,
                to_int,
                dst: self.pick_dst(u)?,
                src: self.pick_cvt_operand(u)?,
                cmp: pick_cmp(u, self.cfg.emit_signed_cmp)?,
                ca: self.pick_guard_operand(u)?,
                cb: self.pick_guard_operand(u)?,
                pred: self.alloc_inst_pred(u)?,
            })
        } else {
            Ok(Inst::F64Cvt {
                from_int,
                to_int,
                dst: self.pick_dst(u)?,
                src: self.pick_cvt_operand(u)?,
            })
        }
    }

    fn pick_f64_special_math(&mut self, u: &mut Unstructured) -> Result<Inst> {
        let ops = [
            F64SpecialMathOp::SqrtRn,
            F64SpecialMathOp::SqrtRz,
            F64SpecialMathOp::SqrtRm,
            F64SpecialMathOp::SqrtRp,
            F64SpecialMathOp::RcpRn,
            F64SpecialMathOp::RcpRz,
            F64SpecialMathOp::RcpRm,
            F64SpecialMathOp::RcpRp,
        ];
        let op = *u.choose(&ops)?;
        if self.cfg.emit_predicated_unary && u.arbitrary::<bool>()? {
            Ok(Inst::PredicatedF64SpecialMath {
                op,
                dst: self.pick_dst(u)?,
                src: self.pick_cvt_operand(u)?,
                cmp: pick_cmp(u, self.cfg.emit_signed_cmp)?,
                ca: self.pick_guard_operand(u)?,
                cb: self.pick_guard_operand(u)?,
                pred: self.alloc_inst_pred(u)?,
            })
        } else {
            Ok(Inst::F64SpecialMath {
                op,
                dst: self.pick_dst(u)?,
                src: self.pick_cvt_operand(u)?,
            })
        }
    }

    fn pick_f64_set(&mut self, u: &mut Unstructured) -> Result<Inst> {
        if self.cfg.emit_predicated_set && u.arbitrary::<bool>()? {
            Ok(Inst::PredicatedF64Set {
                cmp: pick_float_cmp(u)?,
                dst: self.pick_non_output_dst(u)?,
                a: self.pick_cvt_operand(u)?,
                b: self.pick_cvt_operand(u)?,
                guard_cmp: pick_cmp(u, self.cfg.emit_signed_cmp)?,
                guard_ca: self.pick_guard_operand(u)?,
                guard_cb: self.pick_guard_operand(u)?,
                guard_pred: self.alloc_inst_pred(u)?,
            })
        } else {
            Ok(Inst::F64Set {
                cmp: pick_float_cmp(u)?,
                dst: self.pick_non_output_dst(u)?,
                a: self.pick_cvt_operand(u)?,
                b: self.pick_cvt_operand(u)?,
            })
        }
    }

    fn pick_f64_setp_bool(&mut self, u: &mut Unstructured) -> Result<Inst> {
        if self.cfg.emit_predicated_set && u.arbitrary::<bool>()? {
            Ok(Inst::PredicatedF64SetpBool {
                bool_op: pick_predicate_bool_op(u)?,
                base_cmp: pick_cmp(u, self.cfg.emit_signed_cmp)?,
                base_a: self.pick_guard_operand(u)?,
                base_b: self.pick_guard_operand(u)?,
                cmp: pick_float_cmp(u)?,
                dst: self.pick_non_output_dst(u)?,
                a: self.pick_cvt_operand(u)?,
                b: self.pick_cvt_operand(u)?,
                base_pred: self.alloc_pred(),
                pred: self.alloc_pred(),
                guard_cmp: pick_cmp(u, self.cfg.emit_signed_cmp)?,
                guard_ca: self.pick_guard_operand(u)?,
                guard_cb: self.pick_guard_operand(u)?,
                guard_pred: self.alloc_inst_pred(u)?,
            })
        } else {
            Ok(Inst::F64SetpBool {
                bool_op: pick_predicate_bool_op(u)?,
                base_cmp: pick_cmp(u, self.cfg.emit_signed_cmp)?,
                base_a: self.pick_guard_operand(u)?,
                base_b: self.pick_guard_operand(u)?,
                cmp: pick_float_cmp(u)?,
                dst: self.pick_non_output_dst(u)?,
                a: self.pick_cvt_operand(u)?,
                b: self.pick_cvt_operand(u)?,
                base_pred: self.alloc_pred(),
                pred: self.alloc_pred(),
            })
        }
    }

    fn pick_f64_testp(&mut self, u: &mut Unstructured) -> Result<Inst> {
        if self.cfg.emit_predicated_set && u.arbitrary::<bool>()? {
            Ok(Inst::PredicatedF64Testp {
                op: self.pick_float_testp_op(u)?,
                dst: self.pick_non_output_dst(u)?,
                src_lo: self.pick_reg_operand(u)?,
                src_hi: self.pick_reg_operand(u)?,
                pred: self.alloc_pred(),
                guard_cmp: pick_cmp(u, self.cfg.emit_signed_cmp)?,
                guard_ca: self.pick_guard_operand(u)?,
                guard_cb: self.pick_guard_operand(u)?,
                guard_pred: self.alloc_inst_pred(u)?,
            })
        } else {
            Ok(Inst::F64Testp {
                op: self.pick_float_testp_op(u)?,
                dst: self.pick_non_output_dst(u)?,
                src_lo: self.pick_reg_operand(u)?,
                src_hi: self.pick_reg_operand(u)?,
                pred: self.alloc_pred(),
            })
        }
    }

    fn pick_f64_selp(&mut self, u: &mut Unstructured) -> Result<Inst> {
        if self.cfg.emit_predicated_selp && u.arbitrary::<bool>()? {
            Ok(Inst::PredicatedF64Selp {
                cmp: pick_float_cmp(u)?,
                dst: self.pick_dst(u)?,
                a: self.pick_cvt_operand(u)?,
                b: self.pick_cvt_operand(u)?,
                pred: self.alloc_pred(),
                guard_cmp: pick_cmp(u, self.cfg.emit_signed_cmp)?,
                guard_ca: self.pick_guard_operand(u)?,
                guard_cb: self.pick_guard_operand(u)?,
                guard_pred: self.alloc_inst_pred(u)?,
            })
        } else {
            Ok(Inst::F64Selp {
                cmp: pick_float_cmp(u)?,
                dst: self.pick_dst(u)?,
                a: self.pick_cvt_operand(u)?,
                b: self.pick_cvt_operand(u)?,
                pred: self.alloc_pred(),
            })
        }
    }

    fn gen_inst(&mut self, u: &mut Unstructured) -> Result<Inst> {
        // Distribution (out of 256). Lop3 gets disproportionate weight because
        // it's both novel coverage and the biggest constant-folding hotspot in
        // the optimizer; mul/bfe/bfi/prmt/shf are smaller but each adds a
        // distinct lowering path.
        //   0..90    (35%) Bin
        //   90..115  (10%) Sel/Set
        //   115..140 (10%) Shift (immediate amount)
        //   140..160 ( 8%) Unary
        //   160..200 (16%) Lop3, or Mad when explicit Lop3 generation is off
        //   200..215 ( 6%) Prmt
        //   215..225 ( 4%) Funnel
        //   225..235 ( 4%) Bfe/Bmsk
        //   235..245 ( 4%) Bfi
        //   245..248 ( 1%) Cvt
        //   248..251 ( 1%) Bfind, or Mad24 when Bfind generation is off
        //   251..253 ( 1%) DivRem/MulWide/WideInt
        //   253..254 (<1%) Sad/Video
        //   254..255 (<1%) Slct
        //   255..256 (<1%) Dp4a/Dp2a
        let pick: u8 = u.arbitrary()?;
        if pick < 90 {
            if self.cfg.emit_global_loads && u.int_in_range(0..=7)? == 0 {
                return self.pick_global_load(u);
            }
            if self.cfg.emit_global_store_roundtrips
                && self.cfg.emit_mul_wide
                && self.cfg.emit_wide_int
                && u.int_in_range(0..=7)? == 0
            {
                return self.pick_global_store_roundtrip(u);
            }
            if self.cfg.emit_const_memory && u.int_in_range(0..=7)? == 0 {
                return self.pick_const_load(u);
            }
            if self.cfg.emit_local_memory && u.int_in_range(0..=7)? == 0 {
                return self.pick_local_mem(u);
            }
            if self.cfg.emit_shared_memory
                && self.cfg.emit_mul_wide
                && self.cfg.emit_wide_int
                && u.int_in_range(0..=7)? == 0
            {
                return self.pick_shared_mem(u);
            }
            if self.can_emit_vector_memory() && u.int_in_range(0..=7)? == 0 {
                return self.pick_vector_memory(u);
            }
            if self.cfg.emit_f32_arith
                && self.cfg.emit_bitwise_binops
                && u.int_in_range(0..=7)? == 0
            {
                return self.pick_f32_arith(u);
            }
            if self.cfg.emit_f32_arith
                && self.cfg.emit_f32_rounding
                && self.cfg.emit_bitwise_binops
                && u.int_in_range(0..=7)? == 0
            {
                return self.pick_f32_rounding_arith(u);
            }
            if self.cfg.emit_f32_unary
                && self.cfg.emit_bitwise_binops
                && u.int_in_range(0..=7)? == 0
            {
                return self.pick_f32_unary(u);
            }
            if self.cfg.emit_f32_cvt && self.cfg.emit_bitwise_binops && u.int_in_range(0..=7)? == 0
            {
                return self.pick_f32_cvt(u);
            }
            if self.cfg.emit_f32_special_math
                && self.cfg.emit_bitwise_binops
                && u.int_in_range(0..=7)? == 0
            {
                return self.pick_f32_special_math(u);
            }
            if self.cfg.emit_f64_arith
                && self.cfg.emit_bitwise_binops
                && u.int_in_range(0..=7)? == 0
            {
                return self.pick_f64_arith(u);
            }
            if self.cfg.emit_f64_arith
                && self.cfg.emit_f64_rounding
                && self.cfg.emit_bitwise_binops
                && u.int_in_range(0..=7)? == 0
            {
                return self.pick_f64_rounding_arith(u);
            }
            if self.cfg.emit_f64_unary
                && self.cfg.emit_bitwise_binops
                && u.int_in_range(0..=7)? == 0
            {
                return self.pick_f64_unary(u);
            }
            if self.cfg.emit_f64_cvt && self.cfg.emit_bitwise_binops && u.int_in_range(0..=7)? == 0
            {
                return self.pick_f64_cvt(u);
            }
            if self.cfg.emit_f64_special_math
                && self.cfg.emit_bitwise_binops
                && u.int_in_range(0..=7)? == 0
            {
                return self.pick_f64_special_math(u);
            }
            if (self.cfg.emit_packed_add || self.cfg.emit_packed_minmax)
                && u.int_in_range(0..=7)? == 0
            {
                let use_minmax = self.cfg.emit_packed_minmax
                    && (!self.cfg.emit_packed_add || u.arbitrary::<bool>()?);
                if use_minmax {
                    let op = pick_packed_minmax(u, self.cfg.emit_signed_packed_minmax)?;
                    if self.cfg.emit_predicated_alu
                        && self.cfg.emit_predicated_packed_minmax
                        && u.arbitrary::<bool>()?
                    {
                        return Ok(Inst::PredicatedPackedMinMax {
                            op,
                            dst: self.pick_dst(u)?,
                            a: self.pick_reg_operand(u)?,
                            b: self.pick_reg_operand(u)?,
                            cmp: pick_cmp(u, self.cfg.emit_signed_cmp)?,
                            ca: self.pick_guard_operand(u)?,
                            cb: self.pick_guard_operand(u)?,
                            pred: self.alloc_inst_pred(u)?,
                        });
                    }
                    return Ok(Inst::PackedMinMax {
                        op,
                        dst: self.pick_dst(u)?,
                        a: self.pick_reg_operand(u)?,
                        b: self.pick_reg_operand(u)?,
                    });
                } else {
                    let op = pick_packed_add(u, self.cfg.emit_signed_packed_add)?;
                    if self.cfg.emit_predicated_alu
                        && self.cfg.emit_predicated_packed_add
                        && u.arbitrary::<bool>()?
                    {
                        return Ok(Inst::PredicatedPackedAdd {
                            op,
                            dst: self.pick_dst(u)?,
                            a: self.pick_reg_operand(u)?,
                            b: self.pick_reg_operand(u)?,
                            cmp: pick_cmp(u, self.cfg.emit_signed_cmp)?,
                            ca: self.pick_guard_operand(u)?,
                            cb: self.pick_guard_operand(u)?,
                            pred: self.alloc_inst_pred(u)?,
                        });
                    }
                    return Ok(Inst::PackedAdd {
                        op,
                        dst: self.pick_dst(u)?,
                        a: self.pick_reg_operand(u)?,
                        b: self.pick_reg_operand(u)?,
                    });
                }
            }
            if self.cfg.emit_scalar_16bit && u.int_in_range(0..=7)? == 0 {
                let op = pick_scalar_16(
                    u,
                    self.cfg.emit_signed_scalar_16bit,
                    self.cfg.emit_scalar_16bit_min,
                    self.cfg.emit_scalar_16bit_signed_unary,
                    self.cfg.emit_scalar_16bit_bitwise,
                    self.cfg.emit_scalar_16bit_shifts,
                )?;
                if self.cfg.emit_predicated_alu
                    && self.cfg.emit_predicated_scalar_16bit
                    && u.arbitrary::<bool>()?
                {
                    let b = if op.is_shift() {
                        Operand::Imm(u.int_in_range(0..=15)?)
                    } else {
                        self.pick_cvt_operand(u)?
                    };
                    return Ok(Inst::PredicatedScalar16 {
                        op,
                        dst: self.pick_dst(u)?,
                        a: self.pick_cvt_operand(u)?,
                        b,
                        cmp: pick_cmp(u, self.cfg.emit_signed_cmp)?,
                        ca: self.pick_guard_operand(u)?,
                        cb: self.pick_guard_operand(u)?,
                        pred: self.alloc_inst_pred(u)?,
                    });
                }
                let b = if op.is_shift() {
                    Operand::Imm(u.int_in_range(0..=15)?)
                } else {
                    self.pick_cvt_operand(u)?
                };
                return Ok(Inst::Scalar16 {
                    op,
                    dst: self.pick_dst(u)?,
                    a: self.pick_cvt_operand(u)?,
                    b,
                });
            }
            let op = pick_binop(
                u,
                self.cfg.emit_minmax,
                self.cfg.emit_sub,
                self.cfg.emit_mul_lo,
                self.cfg.emit_signed_lo_alu,
                self.cfg.emit_sat_arith,
                self.cfg.emit_mulhi,
                self.cfg.emit_signed_mulhi,
                self.cfg.emit_bitwise_binops,
                self.cfg.emit_or,
                self.cfg.emit_xor,
            )?;
            Ok(Inst::Bin {
                op,
                dst: self.pick_dst(u)?,
                a: self.pick_bin_operand(u, op)?,
                b: self.pick_bin_operand(u, op)?,
            })
        } else if pick < 115 {
            if self.cfg.emit_f32_compare
                && self.cfg.emit_setp_bool
                && self.cfg.emit_predicated_alu
                && self.cfg.emit_bitwise_binops
                && u.int_in_range(0..=3)? == 0
            {
                self.pick_f32_setp_bool(u)
            } else if self.cfg.emit_f32_compare
                && self.cfg.emit_bitwise_binops
                && u.int_in_range(0..=3)? == 0
            {
                self.pick_f32_testp(u)
            } else if self.cfg.emit_f32_compare
                && self.cfg.emit_bitwise_binops
                && (self.cfg.emit_f32_selp || self.cfg.emit_set)
                && u.int_in_range(0..=3)? == 0
            {
                let use_selp = self.cfg.emit_f32_selp && u.arbitrary::<bool>()?;
                if use_selp {
                    self.pick_f32_selp(u)
                } else if self.cfg.emit_set {
                    self.pick_f32_set(u)
                } else {
                    self.pick_f32_selp(u)
                }
            } else if self.cfg.emit_f64_compare
                && self.cfg.emit_setp_bool
                && self.cfg.emit_predicated_alu
                && self.cfg.emit_bitwise_binops
                && u.int_in_range(0..=3)? == 0
            {
                self.pick_f64_setp_bool(u)
            } else if self.cfg.emit_f64_compare
                && self.cfg.emit_bitwise_binops
                && u.int_in_range(0..=3)? == 0
            {
                self.pick_f64_testp(u)
            } else if self.cfg.emit_f64_compare
                && self.cfg.emit_bitwise_binops
                && (self.cfg.emit_f64_selp || self.cfg.emit_set)
                && u.int_in_range(0..=3)? == 0
            {
                let use_selp = self.cfg.emit_f64_selp && u.arbitrary::<bool>()?;
                if use_selp {
                    self.pick_f64_selp(u)
                } else if self.cfg.emit_set {
                    self.pick_f64_set(u)
                } else {
                    self.pick_f64_selp(u)
                }
            } else if self.cfg.emit_scalar_16bit
                && self.cfg.emit_scalar_16bit_compare
                && (self.cfg.emit_set || self.cfg.emit_selp || self.cfg.emit_scalar_16bit_selp)
                && u.int_in_range(0..=3)? == 0
            {
                let cmp = pick_cmp(
                    u,
                    self.cfg.emit_signed_cmp && self.cfg.emit_signed_scalar_16bit,
                )?;
                let use_selp = self.cfg.emit_scalar_16bit_selp && u.arbitrary::<bool>()?;
                if use_selp {
                    Ok(Inst::Scalar16Selp {
                        cmp,
                        dst: self.pick_dst(u)?,
                        a: self.pick_cvt_operand(u)?,
                        b: self.pick_cvt_operand(u)?,
                        pred: self.alloc_pred(),
                    })
                } else if self.cfg.emit_selp && (!self.cfg.emit_set || u.arbitrary::<bool>()?) {
                    Ok(Inst::Scalar16Setp {
                        cmp,
                        dst: self.pick_dst(u)?,
                        a: self.pick_cvt_operand(u)?,
                        b: self.pick_cvt_operand(u)?,
                        pred: self.alloc_pred(),
                    })
                } else {
                    Ok(Inst::Scalar16Set {
                        cmp,
                        dst: self.pick_non_output_dst(u)?,
                        a: self.pick_cvt_operand(u)?,
                        b: self.pick_cvt_operand(u)?,
                    })
                }
            } else if self.cfg.emit_pred_logic
                && self.cfg.emit_predicated_alu
                && u.arbitrary::<bool>()?
            {
                self.pick_pred_logic_bin(u)
            } else if self.cfg.emit_setp_dual
                && self.cfg.emit_predicated_alu
                && u.arbitrary::<bool>()?
            {
                self.pick_setp_dual_bin(u)
            } else if self.cfg.emit_setp_bool
                && self.cfg.emit_predicated_alu
                && u.arbitrary::<bool>()?
            {
                self.pick_setp_bool_bin(u)
            } else if self.cfg.emit_predicated_alu && u.arbitrary::<bool>()? {
                let op = pick_binop(
                    u,
                    self.cfg.emit_minmax,
                    self.cfg.emit_sub,
                    self.cfg.emit_mul_lo,
                    self.cfg.emit_signed_lo_alu,
                    self.cfg.emit_sat_arith,
                    self.cfg.emit_mulhi,
                    self.cfg.emit_signed_mulhi,
                    self.cfg.emit_bitwise_binops,
                    self.cfg.emit_or,
                    self.cfg.emit_xor,
                )?;
                Ok(Inst::PredicatedBin {
                    op,
                    dst: self.pick_dst(u)?,
                    a: self.pick_bin_operand(u, op)?,
                    b: self.pick_bin_operand(u, op)?,
                    cmp: pick_cmp(u, self.cfg.emit_signed_cmp)?,
                    ca: self.pick_guard_operand(u)?,
                    cb: self.pick_guard_operand(u)?,
                    pred: self.alloc_inst_pred(u)?,
                })
            } else if self.cfg.emit_set && (!self.cfg.emit_selp || u.arbitrary::<bool>()?) {
                if self.cfg.emit_predicated_set && u.arbitrary::<bool>()? {
                    Ok(Inst::PredicatedSet {
                        dst: self.pick_non_output_dst(u)?,
                        cmp: pick_cmp(u, self.cfg.emit_signed_cmp)?,
                        a: self.pick_operand(u)?,
                        b: self.pick_operand(u)?,
                        guard_cmp: pick_cmp(u, self.cfg.emit_signed_cmp)?,
                        guard_ca: self.pick_guard_operand(u)?,
                        guard_cb: self.pick_guard_operand(u)?,
                        guard_pred: self.alloc_inst_pred(u)?,
                    })
                } else {
                    Ok(Inst::Set {
                        dst: self.pick_non_output_dst(u)?,
                        cmp: pick_cmp(u, self.cfg.emit_signed_cmp)?,
                        a: self.pick_operand(u)?,
                        b: self.pick_operand(u)?,
                    })
                }
            } else if self.cfg.emit_selp {
                if self.cfg.emit_predicated_selp && u.arbitrary::<bool>()? {
                    Ok(Inst::PredicatedSel {
                        op: pick_selp(u, self.cfg.emit_typed_selp)?,
                        dst: self.pick_dst(u)?,
                        a: self.pick_operand(u)?,
                        b: self.pick_operand(u)?,
                        cmp: pick_cmp(u, self.cfg.emit_signed_cmp)?,
                        ca: self.pick_guard_operand(u)?,
                        cb: self.pick_guard_operand(u)?,
                        pred: self.alloc_pred(),
                        guard_cmp: pick_cmp(u, self.cfg.emit_signed_cmp)?,
                        guard_ca: self.pick_guard_operand(u)?,
                        guard_cb: self.pick_guard_operand(u)?,
                        guard_pred: self.alloc_inst_pred(u)?,
                    })
                } else {
                    Ok(Inst::Sel {
                        op: pick_selp(u, self.cfg.emit_typed_selp)?,
                        dst: self.pick_dst(u)?,
                        a: self.pick_operand(u)?,
                        b: self.pick_operand(u)?,
                        cmp: pick_cmp(u, self.cfg.emit_signed_cmp)?,
                        ca: self.pick_guard_operand(u)?,
                        cb: self.pick_guard_operand(u)?,
                        pred: self.alloc_pred(),
                    })
                }
            } else {
                Ok(Inst::Bin {
                    op: BinOp::Add,
                    dst: self.pick_dst(u)?,
                    a: self.pick_operand(u)?,
                    b: self.pick_operand(u)?,
                })
            }
        } else if pick < 140 && (self.cfg.emit_shl || self.cfg.emit_shr || self.cfg.emit_signed_shr)
        {
            let op = pick_shift(
                u,
                self.cfg.emit_shl,
                self.cfg.emit_shr,
                self.cfg.emit_signed_shr,
            )?;
            if self.cfg.emit_predicated_shifts && u.arbitrary::<bool>()? {
                Ok(Inst::PredicatedShift {
                    op,
                    dst: self.pick_dst(u)?,
                    src: self.pick_operand(u)?,
                    amount: u.int_in_range(0..=31)?,
                    cmp: pick_cmp(u, self.cfg.emit_signed_cmp)?,
                    ca: self.pick_guard_operand(u)?,
                    cb: self.pick_guard_operand(u)?,
                    pred: self.alloc_inst_pred(u)?,
                })
            } else if self.cfg.emit_reg_shifts
                && self.cfg.emit_bitwise_binops
                && u.arbitrary::<bool>()?
            {
                if self.cfg.emit_predicated_reg_shifts && u.arbitrary::<bool>()? {
                    Ok(Inst::PredicatedRegShift {
                        op,
                        dst: self.pick_dst(u)?,
                        src: self.pick_operand(u)?,
                        amount: self.pick_operand(u)?,
                        cmp: pick_cmp(u, self.cfg.emit_signed_cmp)?,
                        ca: self.pick_guard_operand(u)?,
                        cb: self.pick_guard_operand(u)?,
                        pred: self.alloc_inst_pred(u)?,
                    })
                } else {
                    Ok(Inst::RegShift {
                        op,
                        dst: self.pick_dst(u)?,
                        src: self.pick_operand(u)?,
                        amount: self.pick_operand(u)?,
                    })
                }
            } else {
                Ok(Inst::Shift {
                    op,
                    dst: self.pick_dst(u)?,
                    src: self.pick_operand(u)?,
                    amount: u.int_in_range(0..=31)?,
                })
            }
        } else if pick < 160 {
            if !self.can_emit_unary() && !self.cfg.emit_special_regs {
                return self.pick_mad_or_add(u);
            }
            if self.cfg.emit_special_regs && (!self.can_emit_unary() || u.arbitrary::<bool>()?) {
                let op = pick_special_reg(u)?;
                if self.cfg.emit_predicated_unary
                    && self.cfg.emit_predicated_special_regs
                    && u.arbitrary::<bool>()?
                {
                    return Ok(Inst::PredicatedSpecialReg {
                        op,
                        dst: self.pick_dst(u)?,
                        cmp: pick_cmp(u, self.cfg.emit_signed_cmp)?,
                        ca: self.pick_guard_operand(u)?,
                        cb: self.pick_guard_operand(u)?,
                        pred: self.alloc_inst_pred(u)?,
                    });
                }
                return Ok(Inst::SpecialReg {
                    op,
                    dst: self.pick_dst(u)?,
                });
            }
            let op = pick_unary(
                u,
                self.cfg.emit_not,
                self.cfg.emit_clz,
                self.cfg.emit_brev,
                self.cfg.emit_neg,
                self.cfg.emit_cnot,
                self.cfg.emit_popc,
                self.cfg.emit_abs,
            )?;
            if self.cfg.emit_predicated_unary && u.arbitrary::<bool>()? {
                Ok(Inst::PredicatedUnary {
                    op,
                    dst: self.pick_dst(u)?,
                    src: self.pick_operand(u)?,
                    cmp: pick_cmp(u, self.cfg.emit_signed_cmp)?,
                    ca: self.pick_guard_operand(u)?,
                    cb: self.pick_guard_operand(u)?,
                    pred: self.alloc_inst_pred(u)?,
                })
            } else {
                Ok(Inst::Unary {
                    op,
                    dst: self.pick_dst(u)?,
                    src: self.pick_operand(u)?,
                })
            }
        } else if pick < 200 {
            if self.cfg.emit_lop3 {
                if self.cfg.emit_predicated_lop3 && u.arbitrary::<bool>()? {
                    Ok(Inst::PredicatedLop3 {
                        dst: self.pick_dst(u)?,
                        a: self.pick_operand(u)?,
                        b: self.pick_operand(u)?,
                        c: self.pick_operand(u)?,
                        imm: u.arbitrary()?,
                        cmp: pick_cmp(u, self.cfg.emit_signed_cmp)?,
                        ca: self.pick_guard_operand(u)?,
                        cb: self.pick_guard_operand(u)?,
                        pred: self.alloc_inst_pred(u)?,
                    })
                } else {
                    Ok(Inst::Lop3 {
                        dst: self.pick_dst(u)?,
                        a: self.pick_operand(u)?,
                        b: self.pick_operand(u)?,
                        c: self.pick_operand(u)?,
                        imm: u.arbitrary()?,
                    })
                }
            } else {
                self.pick_mad_or_add(u)
            }
        } else if pick < 215 {
            if self.cfg.emit_prmt {
                let mode = pick_prmt_mode(u, self.cfg.emit_prmt_modes)?;
                if self.cfg.emit_predicated_prmt && u.arbitrary::<bool>()? {
                    Ok(Inst::PredicatedPrmt {
                        mode,
                        dst: self.pick_dst(u)?,
                        a: self.pick_operand(u)?,
                        b: self.pick_operand(u)?,
                        ctrl: self.pick_prmt_ctrl(
                            u,
                            mode,
                            self.cfg.emit_reg_prmt && self.cfg.emit_predicated_reg_prmt,
                        )?,
                        cmp: pick_cmp(u, self.cfg.emit_signed_cmp)?,
                        ca: self.pick_guard_operand(u)?,
                        cb: self.pick_guard_operand(u)?,
                        pred: self.alloc_inst_pred(u)?,
                    })
                } else {
                    Ok(Inst::Prmt {
                        mode,
                        dst: self.pick_dst(u)?,
                        a: self.pick_operand(u)?,
                        b: self.pick_operand(u)?,
                        ctrl: self.pick_prmt_ctrl(u, mode, self.cfg.emit_reg_prmt)?,
                    })
                }
            } else {
                self.pick_mad_or_add(u)
            }
        } else if pick < 225 {
            if self.cfg.emit_funnel {
                let dir = if u.arbitrary::<bool>()? {
                    FunnelDir::Left
                } else {
                    FunnelDir::Right
                };
                let mode = pick_funnel_mode(u, self.cfg.emit_funnel_clamp)?;
                if self.cfg.emit_predicated_funnel && u.arbitrary::<bool>()? {
                    Ok(Inst::PredicatedFunnel {
                        dir,
                        mode,
                        dst: self.pick_dst(u)?,
                        a: self.pick_operand(u)?,
                        b: self.pick_operand(u)?,
                        amount: if self.cfg.emit_reg_funnel && u.arbitrary::<bool>()? {
                            self.pick_reg_operand(u)?
                        } else {
                            Operand::Imm(u.int_in_range(0..=mode.max_immediate_amount())?)
                        },
                        cmp: pick_cmp(u, self.cfg.emit_signed_cmp)?,
                        ca: self.pick_guard_operand(u)?,
                        cb: self.pick_guard_operand(u)?,
                        pred: self.alloc_inst_pred(u)?,
                    })
                } else if self.cfg.emit_reg_funnel && u.arbitrary::<bool>()? {
                    Ok(Inst::RegFunnel {
                        dir,
                        mode,
                        dst: self.pick_dst(u)?,
                        a: self.pick_operand(u)?,
                        b: self.pick_operand(u)?,
                        amount: self.pick_reg_operand(u)?,
                    })
                } else {
                    Ok(Inst::Funnel {
                        dir,
                        mode,
                        dst: self.pick_dst(u)?,
                        a: self.pick_operand(u)?,
                        b: self.pick_operand(u)?,
                        amount: u.int_in_range(0..=mode.max_immediate_amount())?,
                    })
                }
            } else {
                self.pick_mad_or_add(u)
            }
        } else if pick < 235 {
            if !self.cfg.emit_wide_bfe && !self.cfg.emit_bfe && !self.cfg.emit_bmsk {
                return self.pick_mad_or_add(u);
            }
            if self.cfg.emit_wide_bfe && u.arbitrary::<bool>()? {
                let op = pick_wide_bfe(u, self.cfg.emit_signed_wide_bfe)?;
                let predicated = self.cfg.emit_predicated_wide_bitfield && u.arbitrary::<bool>()?;
                let use_reg_param = self.cfg.emit_reg_wide_bitfield
                    && self.cfg.emit_bitwise_binops
                    && (!predicated || self.cfg.emit_predicated_reg_wide_bitfield)
                    && u.arbitrary::<bool>()?;
                if predicated && use_reg_param {
                    let (param, slot, imm) = self.pick_reg_wide_bitfield_param(u)?;
                    Ok(Inst::PredicatedRegWideBfe {
                        op,
                        dst: self.pick_dst(u)?,
                        src: self.pick_operand(u)?,
                        param,
                        slot,
                        imm,
                        cmp: pick_cmp(u, self.cfg.emit_signed_cmp)?,
                        ca: self.pick_guard_operand(u)?,
                        cb: self.pick_guard_operand(u)?,
                        pred: self.alloc_inst_pred(u)?,
                    })
                } else if use_reg_param {
                    let (param, slot, imm) = self.pick_reg_wide_bitfield_param(u)?;
                    Ok(Inst::RegWideBfe {
                        op,
                        dst: self.pick_dst(u)?,
                        src: self.pick_operand(u)?,
                        param,
                        slot,
                        imm,
                    })
                } else if predicated {
                    Ok(Inst::PredicatedWideBfe {
                        op,
                        dst: self.pick_dst(u)?,
                        src: self.pick_operand(u)?,
                        pos: u.int_in_range(0..=63)?,
                        len: u.int_in_range(0..=63)?,
                        cmp: pick_cmp(u, self.cfg.emit_signed_cmp)?,
                        ca: self.pick_guard_operand(u)?,
                        cb: self.pick_guard_operand(u)?,
                        pred: self.alloc_inst_pred(u)?,
                    })
                } else {
                    Ok(Inst::WideBfe {
                        op,
                        dst: self.pick_dst(u)?,
                        src: self.pick_operand(u)?,
                        pos: u.int_in_range(0..=63)?,
                        len: u.int_in_range(0..=63)?,
                    })
                }
            } else if self.cfg.emit_predicated_bitfield
                && (self.cfg.emit_bfe || self.cfg.emit_bmsk)
                && u.arbitrary::<bool>()?
            {
                let use_bmsk =
                    self.cfg.emit_bmsk && (!self.cfg.emit_bfe || u.arbitrary::<bool>()?);
                if use_bmsk {
                    Ok(Inst::PredicatedBmsk {
                        mode: pick_bmsk_mode(u, self.cfg.emit_bmsk_wrap)?,
                        dst: self.pick_dst(u)?,
                        pos: self.pick_bitfield_param(
                            u,
                            self.cfg.emit_reg_bitfield && self.cfg.emit_predicated_reg_bitfield,
                        )?,
                        len: self.pick_bitfield_param(
                            u,
                            self.cfg.emit_reg_bitfield && self.cfg.emit_predicated_reg_bitfield,
                        )?,
                        cmp: pick_cmp(u, self.cfg.emit_signed_cmp)?,
                        ca: self.pick_guard_operand(u)?,
                        cb: self.pick_guard_operand(u)?,
                        pred: self.alloc_inst_pred(u)?,
                    })
                } else if self.cfg.emit_bfe {
                    Ok(Inst::PredicatedBfe {
                        op: pick_bfe(u)?,
                        dst: self.pick_dst(u)?,
                        src: self.pick_operand(u)?,
                        pos: self.pick_bitfield_param(
                            u,
                            self.cfg.emit_reg_bitfield && self.cfg.emit_predicated_reg_bitfield,
                        )?,
                        len: self.pick_bitfield_param(
                            u,
                            self.cfg.emit_reg_bitfield && self.cfg.emit_predicated_reg_bitfield,
                        )?,
                        cmp: pick_cmp(u, self.cfg.emit_signed_cmp)?,
                        ca: self.pick_guard_operand(u)?,
                        cb: self.pick_guard_operand(u)?,
                        pred: self.alloc_inst_pred(u)?,
                    })
                } else {
                    self.pick_mad_or_add(u)
                }
            } else if self.cfg.emit_bmsk && u.arbitrary::<bool>()? {
                Ok(Inst::Bmsk {
                    mode: pick_bmsk_mode(u, self.cfg.emit_bmsk_wrap)?,
                    dst: self.pick_dst(u)?,
                    pos: self.pick_bitfield_param(u, self.cfg.emit_reg_bitfield)?,
                    len: self.pick_bitfield_param(u, self.cfg.emit_reg_bitfield)?,
                })
            } else if self.cfg.emit_bfe {
                Ok(Inst::Bfe {
                    op: pick_bfe(u)?,
                    dst: self.pick_dst(u)?,
                    src: self.pick_operand(u)?,
                    pos: self.pick_bitfield_param(u, self.cfg.emit_reg_bitfield)?,
                    len: self.pick_bitfield_param(u, self.cfg.emit_reg_bitfield)?,
                })
            } else {
                self.pick_mad_or_add(u)
            }
        } else if pick < 245 {
            if self.cfg.emit_bfi || self.cfg.emit_wide_bfi {
                let use_wide_bfi =
                    self.cfg.emit_wide_bfi && (!self.cfg.emit_bfi || u.arbitrary::<bool>()?);
                if use_wide_bfi {
                    let predicated =
                        self.cfg.emit_predicated_wide_bitfield && u.arbitrary::<bool>()?;
                    let use_reg_param = self.cfg.emit_reg_wide_bitfield
                        && self.cfg.emit_bitwise_binops
                        && (!predicated || self.cfg.emit_predicated_reg_wide_bitfield)
                        && u.arbitrary::<bool>()?;
                    if predicated && use_reg_param {
                        let (param, slot, imm) = self.pick_reg_wide_bitfield_param(u)?;
                        Ok(Inst::PredicatedRegWideBfi {
                            dst: self.pick_dst(u)?,
                            src: self.pick_operand(u)?,
                            base: self.pick_operand(u)?,
                            param,
                            slot,
                            imm,
                            cmp: pick_cmp(u, self.cfg.emit_signed_cmp)?,
                            ca: self.pick_guard_operand(u)?,
                            cb: self.pick_guard_operand(u)?,
                            pred: self.alloc_inst_pred(u)?,
                        })
                    } else if use_reg_param {
                        let (param, slot, imm) = self.pick_reg_wide_bitfield_param(u)?;
                        Ok(Inst::RegWideBfi {
                            dst: self.pick_dst(u)?,
                            src: self.pick_operand(u)?,
                            base: self.pick_operand(u)?,
                            param,
                            slot,
                            imm,
                        })
                    } else if predicated {
                        Ok(Inst::PredicatedWideBfi {
                            dst: self.pick_dst(u)?,
                            src: self.pick_operand(u)?,
                            base: self.pick_operand(u)?,
                            pos: u.int_in_range(0..=63)?,
                            len: u.int_in_range(0..=63)?,
                            cmp: pick_cmp(u, self.cfg.emit_signed_cmp)?,
                            ca: self.pick_guard_operand(u)?,
                            cb: self.pick_guard_operand(u)?,
                            pred: self.alloc_inst_pred(u)?,
                        })
                    } else {
                        Ok(Inst::WideBfi {
                            dst: self.pick_dst(u)?,
                            src: self.pick_operand(u)?,
                            base: self.pick_operand(u)?,
                            pos: u.int_in_range(0..=63)?,
                            len: u.int_in_range(0..=63)?,
                        })
                    }
                } else if self.cfg.emit_predicated_bitfield && u.arbitrary::<bool>()? {
                    Ok(Inst::PredicatedBfi {
                        dst: self.pick_dst(u)?,
                        src: self.pick_operand(u)?,
                        base: self.pick_operand(u)?,
                        pos: self.pick_bitfield_param(
                            u,
                            self.cfg.emit_reg_bitfield && self.cfg.emit_predicated_reg_bitfield,
                        )?,
                        len: self.pick_bitfield_param(
                            u,
                            self.cfg.emit_reg_bitfield && self.cfg.emit_predicated_reg_bitfield,
                        )?,
                        cmp: pick_cmp(u, self.cfg.emit_signed_cmp)?,
                        ca: self.pick_guard_operand(u)?,
                        cb: self.pick_guard_operand(u)?,
                        pred: self.alloc_inst_pred(u)?,
                    })
                } else {
                    Ok(Inst::Bfi {
                        dst: self.pick_dst(u)?,
                        src: self.pick_operand(u)?,
                        base: self.pick_operand(u)?,
                        pos: self.pick_bitfield_param(u, self.cfg.emit_reg_bitfield)?,
                        len: self.pick_bitfield_param(u, self.cfg.emit_reg_bitfield)?,
                    })
                }
            } else {
                self.pick_mad_or_add(u)
            }
        } else if pick < 248 {
            let cvt_pick: u8 = u.int_in_range(0..=3)?;
            if self.cfg.emit_wide_cvt && cvt_pick == 0 {
                self.pick_wide_cvt(u)
            } else if self.cfg.emit_narrow_cvt && cvt_pick == 1 {
                self.pick_narrow_cvt(u)
            } else if self.cfg.emit_szext && u.arbitrary::<bool>()? {
                let op = pick_szext(u, self.cfg.emit_signed_szext)?;
                if self.cfg.emit_predicated_szext && u.arbitrary::<bool>()? {
                    Ok(Inst::PredicatedSzext {
                        op,
                        dst: self.pick_dst(u)?,
                        src: self.pick_operand(u)?,
                        width: self.pick_operand(u)?,
                        cmp: pick_cmp(u, self.cfg.emit_signed_cmp)?,
                        ca: self.pick_guard_operand(u)?,
                        cb: self.pick_guard_operand(u)?,
                        pred: self.alloc_inst_pred(u)?,
                    })
                } else {
                    Ok(Inst::Szext {
                        op,
                        dst: self.pick_dst(u)?,
                        src: self.pick_operand(u)?,
                        width: self.pick_operand(u)?,
                    })
                }
            } else if self.cfg.emit_cvt && self.cfg.emit_predicated_cvt && u.arbitrary::<bool>()? {
                Ok(Inst::PredicatedCvt {
                    op: pick_cvt(u)?,
                    dst: self.pick_dst(u)?,
                    src: self.pick_cvt_operand(u)?,
                    cmp: pick_cmp(u, self.cfg.emit_signed_cmp)?,
                    ca: self.pick_guard_operand(u)?,
                    cb: self.pick_guard_operand(u)?,
                    pred: self.alloc_inst_pred(u)?,
                })
            } else if self.cfg.emit_cvt {
                Ok(Inst::Cvt {
                    op: pick_cvt(u)?,
                    dst: self.pick_dst(u)?,
                    src: self.pick_cvt_operand(u)?,
                })
            } else {
                self.pick_mad_or_add(u)
            }
        } else if pick < 251 {
            let aux_pick: u8 = u.int_in_range(0..=5)?;
            if self.cfg.emit_mad_carry && aux_pick == 0 {
                self.pick_mad_carry(u)
            } else if (self.cfg.emit_addc || self.cfg.emit_subc)
                && self.cfg.emit_carry_chain
                && aux_pick == 1
            {
                self.pick_carry_chain(u)
            } else if self.cfg.emit_subword_wide && aux_pick == 2 {
                self.pick_subword_wide(u)
            } else if (self.cfg.emit_addc || self.cfg.emit_subc) && u.arbitrary::<bool>()? {
                let op = pick_add_carry(u, self.cfg.emit_addc, self.cfg.emit_subc)?;
                if self.cfg.emit_predicated_carry && u.arbitrary::<bool>()? {
                    Ok(Inst::PredicatedAddCarry {
                        op,
                        dst_lo: self.pick_dst(u)?,
                        dst_hi: self.pick_dst(u)?,
                        a: self.pick_operand(u)?,
                        b: self.pick_operand(u)?,
                        c: self.pick_operand(u)?,
                        d: self.pick_operand(u)?,
                        cmp: pick_cmp(u, self.cfg.emit_signed_cmp)?,
                        ca: self.pick_guard_operand(u)?,
                        cb: self.pick_guard_operand(u)?,
                        pred: self.alloc_inst_pred(u)?,
                    })
                } else {
                    Ok(Inst::AddCarry {
                        op,
                        dst_lo: self.pick_dst(u)?,
                        dst_hi: self.pick_dst(u)?,
                        a: self.pick_operand(u)?,
                        b: self.pick_operand(u)?,
                        c: self.pick_operand(u)?,
                        d: self.pick_operand(u)?,
                    })
                }
            } else if self.cfg.emit_fns
                && (!self.cfg.emit_bfind || u.arbitrary::<bool>()?)
                && (!(self.cfg.emit_mad24 || self.cfg.emit_mul24) || u.arbitrary::<bool>()?)
            {
                if self.cfg.emit_reg_fns && self.cfg.emit_bitwise_binops && u.arbitrary::<bool>()? {
                    self.pick_reg_fns(u)
                } else if self.cfg.emit_predicated_fns && u.arbitrary::<bool>()? {
                    Ok(Inst::PredicatedFns {
                        dst: self.pick_dst(u)?,
                        mask: self.pick_operand(u)?,
                        base: u.int_in_range(0..=31)?,
                        offset: pick_fns_offset(u)?,
                        cmp: pick_cmp(u, self.cfg.emit_signed_cmp)?,
                        ca: self.pick_guard_operand(u)?,
                        cb: self.pick_guard_operand(u)?,
                        pred: self.alloc_inst_pred(u)?,
                    })
                } else {
                    Ok(Inst::Fns {
                        dst: self.pick_dst(u)?,
                        mask: self.pick_operand(u)?,
                        base: u.int_in_range(0..=31)?,
                        offset: pick_fns_offset(u)?,
                    })
                }
            } else if self.cfg.emit_bfind {
                let op = pick_bfind(
                    u,
                    self.cfg.emit_signed_bfind,
                    self.cfg.emit_wide_bfind,
                    self.cfg.emit_signed_wide_bfind,
                )?;
                let can_predicate = self.cfg.emit_predicated_bfind
                    && (!op.is_wide() || self.cfg.emit_predicated_wide_bfind);
                if can_predicate && u.arbitrary::<bool>()? {
                    Ok(Inst::PredicatedBfind {
                        op,
                        dst: self.pick_dst(u)?,
                        src: self.pick_operand(u)?,
                        cmp: pick_cmp(u, self.cfg.emit_signed_cmp)?,
                        ca: self.pick_guard_operand(u)?,
                        cb: self.pick_guard_operand(u)?,
                        pred: self.alloc_inst_pred(u)?,
                    })
                } else {
                    Ok(Inst::Bfind {
                        op,
                        dst: self.pick_dst(u)?,
                        src: self.pick_operand(u)?,
                    })
                }
            } else if self.cfg.emit_mad24 || self.cfg.emit_mul24 {
                let emit_mul24 = if self.cfg.emit_mad24 && self.cfg.emit_mul24 {
                    u.arbitrary::<bool>()?
                } else {
                    self.cfg.emit_mul24
                };
                if emit_mul24 {
                    if self.cfg.emit_predicated_24bit && u.arbitrary::<bool>()? {
                        Ok(Inst::PredicatedMul24 {
                            op: pick_mul24(u)?,
                            dst: self.pick_dst(u)?,
                            a: self.pick_operand(u)?,
                            b: self.pick_operand(u)?,
                            cmp: pick_cmp(u, self.cfg.emit_signed_cmp)?,
                            ca: self.pick_guard_operand(u)?,
                            cb: self.pick_guard_operand(u)?,
                            pred: self.alloc_inst_pred(u)?,
                        })
                    } else {
                        Ok(Inst::Mul24 {
                            op: pick_mul24(u)?,
                            dst: self.pick_dst(u)?,
                            a: self.pick_operand(u)?,
                            b: self.pick_operand(u)?,
                        })
                    }
                } else {
                    if self.cfg.emit_predicated_24bit && u.arbitrary::<bool>()? {
                        Ok(Inst::PredicatedMad24 {
                            op: pick_mad24(u)?,
                            dst: self.pick_dst(u)?,
                            a: self.pick_operand(u)?,
                            b: self.pick_operand(u)?,
                            c: self.pick_operand(u)?,
                            cmp: pick_cmp(u, self.cfg.emit_signed_cmp)?,
                            ca: self.pick_guard_operand(u)?,
                            cb: self.pick_guard_operand(u)?,
                            pred: self.alloc_inst_pred(u)?,
                        })
                    } else {
                        Ok(Inst::Mad24 {
                            op: pick_mad24(u)?,
                            dst: self.pick_dst(u)?,
                            a: self.pick_operand(u)?,
                            b: self.pick_operand(u)?,
                            c: self.pick_operand(u)?,
                        })
                    }
                }
            } else {
                self.pick_mad_or_add(u)
            }
        } else if pick < 253 {
            let wide_pick: u8 = u.int_in_range(0..=14)?;
            if self.cfg.emit_wide_int && wide_pick == 0 {
                let op = pick_wide_int(u, self.cfg.emit_wide_minmax, self.cfg.emit_wide_mulhi)?;
                if self.cfg.emit_predicated_wide_int && u.arbitrary::<bool>()? {
                    Ok(Inst::PredicatedWideInt {
                        op,
                        dst: self.pick_dst(u)?,
                        a: self.pick_operand(u)?,
                        b: self.pick_operand(u)?,
                        cmp: pick_cmp(u, self.cfg.emit_signed_cmp)?,
                        ca: self.pick_guard_operand(u)?,
                        cb: self.pick_guard_operand(u)?,
                        pred: self.alloc_inst_pred(u)?,
                    })
                } else {
                    Ok(Inst::WideInt {
                        op,
                        dst: self.pick_dst(u)?,
                        a: self.pick_operand(u)?,
                        b: self.pick_operand(u)?,
                    })
                }
            } else if self.cfg.emit_mul_wide && wide_pick == 1 {
                let op = pick_mul_wide(u)?;
                let keep_high = self.cfg.emit_wide_high_result && u.arbitrary::<bool>()?;
                if self.cfg.emit_predicated_mul_wide && u.arbitrary::<bool>()? {
                    Ok(Inst::PredicatedMulWide {
                        op,
                        dst: self.pick_dst(u)?,
                        a: self.pick_operand(u)?,
                        b: self.pick_operand(u)?,
                        keep_high,
                        cmp: pick_cmp(u, self.cfg.emit_signed_cmp)?,
                        ca: self.pick_guard_operand(u)?,
                        cb: self.pick_guard_operand(u)?,
                        pred: self.alloc_inst_pred(u)?,
                    })
                } else {
                    Ok(Inst::MulWide {
                        op,
                        dst: self.pick_dst(u)?,
                        a: self.pick_operand(u)?,
                        b: self.pick_operand(u)?,
                        keep_high,
                    })
                }
            } else if self.cfg.emit_mad_wide && wide_pick == 8 {
                let op = pick_mad_wide(u, self.cfg.emit_signed_mad_wide)?;
                let keep_high = self.cfg.emit_wide_high_result && u.arbitrary::<bool>()?;
                if self.cfg.emit_predicated_mad_wide && u.arbitrary::<bool>()? {
                    Ok(Inst::PredicatedMadWide {
                        op,
                        dst: self.pick_dst(u)?,
                        a: self.pick_operand(u)?,
                        b: self.pick_operand(u)?,
                        c: self.pick_operand(u)?,
                        keep_high,
                        cmp: pick_cmp(u, self.cfg.emit_signed_cmp)?,
                        ca: self.pick_guard_operand(u)?,
                        cb: self.pick_guard_operand(u)?,
                        pred: self.alloc_inst_pred(u)?,
                    })
                } else {
                    Ok(Inst::MadWide {
                        op,
                        dst: self.pick_dst(u)?,
                        a: self.pick_operand(u)?,
                        b: self.pick_operand(u)?,
                        c: self.pick_operand(u)?,
                        keep_high,
                    })
                }
            } else if self.cfg.emit_wide_mad64 && wide_pick == 10 {
                self.pick_wide_mad64(u)
            } else if self.cfg.emit_wide_shifts && wide_pick == 2 {
                let op = pick_wide_shift(u)?;
                if self.cfg.emit_wide_reg_shifts
                    && self.cfg.emit_bitwise_binops
                    && u.arbitrary::<bool>()?
                {
                    if self.cfg.emit_predicated_wide_reg_shifts && u.arbitrary::<bool>()? {
                        Ok(Inst::PredicatedRegWideShift {
                            op,
                            dst: self.pick_dst(u)?,
                            src: self.pick_operand(u)?,
                            amount: self.pick_operand(u)?,
                            cmp: pick_cmp(u, self.cfg.emit_signed_cmp)?,
                            ca: self.pick_guard_operand(u)?,
                            cb: self.pick_guard_operand(u)?,
                            pred: self.alloc_inst_pred(u)?,
                        })
                    } else {
                        Ok(Inst::RegWideShift {
                            op,
                            dst: self.pick_dst(u)?,
                            src: self.pick_operand(u)?,
                            amount: self.pick_operand(u)?,
                        })
                    }
                } else if self.cfg.emit_predicated_wide_shifts && u.arbitrary::<bool>()? {
                    Ok(Inst::PredicatedWideShift {
                        op,
                        dst: self.pick_dst(u)?,
                        src: self.pick_operand(u)?,
                        amount: u.int_in_range(0..=63)?,
                        cmp: pick_cmp(u, self.cfg.emit_signed_cmp)?,
                        ca: self.pick_guard_operand(u)?,
                        cb: self.pick_guard_operand(u)?,
                        pred: self.alloc_inst_pred(u)?,
                    })
                } else {
                    Ok(Inst::WideShift {
                        op,
                        dst: self.pick_dst(u)?,
                        src: self.pick_operand(u)?,
                        amount: u.int_in_range(0..=63)?,
                    })
                }
            } else if self.cfg.emit_wide_setp && self.cfg.emit_predicated_alu && wide_pick == 3 {
                self.pick_wide_setp_bin(u)
            } else if self.cfg.emit_wide_selp && wide_pick == 4 {
                self.pick_wide_selp(u)
            } else if self.cfg.emit_wide_setp_bool && self.cfg.emit_predicated_alu && wide_pick == 5
            {
                self.pick_wide_setp_bool_bin(u)
            } else if self.cfg.emit_wide_set && self.cfg.emit_set && wide_pick == 9 {
                self.pick_wide_set(u)
            } else if self.cfg.emit_wide_unary && wide_pick == 6 {
                self.pick_wide_unary(u)
            } else if self.cfg.emit_wide_divrem && wide_pick == 7 {
                self.pick_wide_divrem(u)
            } else if (self.cfg.emit_wide_addc || self.cfg.emit_wide_subc) && wide_pick == 11 {
                self.pick_wide_carry(u)
            } else if (self.cfg.emit_wide_addc || self.cfg.emit_wide_subc)
                && self.cfg.emit_wide_carry_chain
                && wide_pick == 12
            {
                self.pick_wide_carry_chain(u)
            } else {
                let can_emit_reg_divrem =
                    self.cfg.emit_reg_divrem && self.cfg.emit_bitwise_binops && self.cfg.emit_or;
                let use_reg_divisor = can_emit_reg_divrem && u.arbitrary::<bool>()?;
                let op = if use_reg_divisor {
                    pick_unsigned_divrem(u)?
                } else {
                    pick_divrem(u, self.cfg.emit_signed_divrem)?
                };
                if use_reg_divisor {
                    if self.cfg.emit_predicated_reg_divrem && u.arbitrary::<bool>()? {
                        Ok(Inst::PredicatedRegDivRem {
                            op,
                            dst: self.pick_dst(u)?,
                            src: self.pick_operand(u)?,
                            divisor: self.pick_operand(u)?,
                            cmp: pick_cmp(u, self.cfg.emit_signed_cmp)?,
                            ca: self.pick_guard_operand(u)?,
                            cb: self.pick_guard_operand(u)?,
                            pred: self.alloc_inst_pred(u)?,
                        })
                    } else {
                        Ok(Inst::RegDivRem {
                            op,
                            dst: self.pick_dst(u)?,
                            src: self.pick_operand(u)?,
                            divisor: self.pick_operand(u)?,
                        })
                    }
                } else if self.cfg.emit_predicated_divrem && u.arbitrary::<bool>()? {
                    let divisor = self.pick_divisor(u, op)?;
                    Ok(Inst::PredicatedDivRem {
                        op,
                        dst: self.pick_dst(u)?,
                        src: self.pick_operand(u)?,
                        divisor,
                        cmp: pick_cmp(u, self.cfg.emit_signed_cmp)?,
                        ca: self.pick_guard_operand(u)?,
                        cb: self.pick_guard_operand(u)?,
                        pred: self.alloc_inst_pred(u)?,
                    })
                } else {
                    let divisor = self.pick_divisor(u, op)?;
                    Ok(Inst::DivRem {
                        op,
                        dst: self.pick_dst(u)?,
                        src: self.pick_operand(u)?,
                        divisor,
                    })
                }
            }
        } else if pick < 254 {
            if self.cfg.emit_video && u.arbitrary::<bool>()? {
                let op = pick_video(
                    u,
                    self.cfg.emit_vsub4,
                    self.cfg.emit_signed_video,
                    self.cfg.emit_video_sat,
                )?;
                if self.cfg.emit_predicated_video && u.arbitrary::<bool>()? {
                    Ok(Inst::PredicatedVideo {
                        op,
                        dst: self.pick_dst(u)?,
                        a: self.pick_reg_operand(u)?,
                        b: self.pick_reg_operand(u)?,
                        c: self.pick_reg_operand(u)?,
                        cmp: pick_cmp(u, self.cfg.emit_signed_cmp)?,
                        ca: self.pick_guard_operand(u)?,
                        cb: self.pick_guard_operand(u)?,
                        pred: self.alloc_inst_pred(u)?,
                    })
                } else {
                    Ok(Inst::Video {
                        op,
                        dst: self.pick_dst(u)?,
                        a: self.pick_reg_operand(u)?,
                        b: self.pick_reg_operand(u)?,
                        c: self.pick_reg_operand(u)?,
                    })
                }
            } else if self.cfg.emit_sad {
                let op = pick_sad(u)?;
                if self.cfg.emit_predicated_sad && u.arbitrary::<bool>()? {
                    Ok(Inst::PredicatedSad {
                        op,
                        dst: self.pick_dst(u)?,
                        a: self.pick_operand(u)?,
                        b: self.pick_operand(u)?,
                        c: self.pick_operand(u)?,
                        cmp: pick_cmp(u, self.cfg.emit_signed_cmp)?,
                        ca: self.pick_guard_operand(u)?,
                        cb: self.pick_guard_operand(u)?,
                        pred: self.alloc_inst_pred(u)?,
                    })
                } else {
                    Ok(Inst::Sad {
                        op,
                        dst: self.pick_dst(u)?,
                        a: self.pick_operand(u)?,
                        b: self.pick_operand(u)?,
                        c: self.pick_operand(u)?,
                    })
                }
            } else {
                self.pick_mad_or_add(u)
            }
        } else if pick < 255 {
            if self.cfg.emit_slct {
                let op = pick_slct(
                    u,
                    self.cfg.emit_s32_slct,
                    self.cfg.emit_f32_slct,
                    self.cfg.emit_wide_slct,
                    self.cfg.emit_f64_slct,
                )?;
                if self.cfg.emit_predicated_slct && u.arbitrary::<bool>()? {
                    Ok(Inst::PredicatedSlct {
                        op,
                        dst: self.pick_dst(u)?,
                        a: self.pick_operand(u)?,
                        b: self.pick_operand(u)?,
                        c: self.pick_operand(u)?,
                        cmp: pick_cmp(u, self.cfg.emit_signed_cmp)?,
                        ca: self.pick_guard_operand(u)?,
                        cb: self.pick_guard_operand(u)?,
                        pred: self.alloc_inst_pred(u)?,
                    })
                } else {
                    Ok(Inst::Slct {
                        op,
                        dst: self.pick_dst(u)?,
                        a: self.pick_operand(u)?,
                        b: self.pick_operand(u)?,
                        c: self.pick_operand(u)?,
                    })
                }
            } else {
                self.pick_mad_or_add(u)
            }
        } else if self.cfg.emit_dp2a || self.cfg.emit_dp4a {
            let use_dp2a = if self.cfg.emit_dp2a && self.cfg.emit_dp4a {
                u.arbitrary::<bool>()?
            } else {
                self.cfg.emit_dp2a
            };
            if use_dp2a {
                let op = pick_dp2a(u)?;
                if self.cfg.emit_predicated_dp && u.arbitrary::<bool>()? {
                    Ok(Inst::PredicatedDp2a {
                        op,
                        dst: self.pick_dst(u)?,
                        a: self.pick_operand(u)?,
                        b: self.pick_operand(u)?,
                        c: self.pick_operand(u)?,
                        cmp: pick_cmp(u, self.cfg.emit_signed_cmp)?,
                        ca: self.pick_guard_operand(u)?,
                        cb: self.pick_guard_operand(u)?,
                        pred: self.alloc_inst_pred(u)?,
                    })
                } else {
                    Ok(Inst::Dp2a {
                        op,
                        dst: self.pick_dst(u)?,
                        a: self.pick_operand(u)?,
                        b: self.pick_operand(u)?,
                        c: self.pick_operand(u)?,
                    })
                }
            } else {
                let op = pick_dp4a(u)?;
                if self.cfg.emit_predicated_dp && u.arbitrary::<bool>()? {
                    Ok(Inst::PredicatedDp4a {
                        op,
                        dst: self.pick_dst(u)?,
                        a: self.pick_operand(u)?,
                        b: self.pick_operand(u)?,
                        c: self.pick_operand(u)?,
                        cmp: pick_cmp(u, self.cfg.emit_signed_cmp)?,
                        ca: self.pick_guard_operand(u)?,
                        cb: self.pick_guard_operand(u)?,
                        pred: self.alloc_inst_pred(u)?,
                    })
                } else {
                    Ok(Inst::Dp4a {
                        op,
                        dst: self.pick_dst(u)?,
                        a: self.pick_operand(u)?,
                        b: self.pick_operand(u)?,
                        c: self.pick_operand(u)?,
                    })
                }
            }
        } else {
            self.pick_mad_or_add(u)
        }
    }

    fn forget_tracked_write(&mut self, dst: u32) {
        self.prmt_result_regs.retain(|reg| *reg != dst);
        self.set_result_regs.retain(|reg| *reg != dst);
    }

    fn remember_prmt_write(&mut self, dst: u32) {
        self.set_result_regs.retain(|reg| *reg != dst);
        if !self.prmt_result_regs.contains(&dst) {
            self.prmt_result_regs.push(dst);
        }
    }

    fn remember_set_write(&mut self, dst: u32) {
        self.prmt_result_regs.retain(|reg| *reg != dst);
        if !self.set_result_regs.contains(&dst) {
            self.set_result_regs.push(dst);
        }
    }

    fn note_inst(&mut self, inst: &Inst) {
        match inst {
            Inst::Set { dst, .. }
            | Inst::Scalar16Set { dst, .. }
            | Inst::F32Set { dst, .. }
            | Inst::PredicatedF32Set { dst, .. }
            | Inst::F32SetpBool { dst, .. }
            | Inst::PredicatedF32SetpBool { dst, .. }
            | Inst::F32Testp { dst, .. }
            | Inst::PredicatedF32Testp { dst, .. }
            | Inst::F64Set { dst, .. }
            | Inst::PredicatedF64Set { dst, .. }
            | Inst::F64SetpBool { dst, .. }
            | Inst::PredicatedF64SetpBool { dst, .. }
            | Inst::F64Testp { dst, .. }
            | Inst::PredicatedF64Testp { dst, .. }
            | Inst::PredicatedSet { dst, .. }
            | Inst::WideSet { dst, .. }
            | Inst::PredicatedWideSet { dst, .. } => {
                self.remember_set_write(*dst);
            }
            Inst::Prmt { dst, .. } | Inst::PredicatedPrmt { dst, .. } => {
                self.remember_prmt_write(*dst);
            }
            Inst::AddCarry { dst_lo, dst_hi, .. }
            | Inst::PredicatedAddCarry { dst_lo, dst_hi, .. }
            | Inst::WideCarry { dst_lo, dst_hi, .. }
            | Inst::PredicatedWideCarry { dst_lo, dst_hi, .. } => {
                self.forget_tracked_write(*dst_lo);
                self.forget_tracked_write(*dst_hi);
            }
            Inst::CarryChain {
                dst0, dst1, dst2, ..
            }
            | Inst::PredicatedCarryChain {
                dst0, dst1, dst2, ..
            }
            | Inst::WideCarryChain {
                dst0, dst1, dst2, ..
            }
            | Inst::PredicatedWideCarryChain {
                dst0, dst1, dst2, ..
            }
            | Inst::MadCarry {
                dst0, dst1, dst2, ..
            }
            | Inst::PredicatedMadCarry {
                dst0, dst1, dst2, ..
            } => {
                self.forget_tracked_write(*dst0);
                self.forget_tracked_write(*dst1);
                self.forget_tracked_write(*dst2);
            }
            Inst::GlobalVectorLoad { op, dsts, .. }
            | Inst::PredicatedGlobalVectorLoad { op, dsts, .. }
            | Inst::GlobalVectorStoreRoundtrip { op, dsts, .. }
            | Inst::PredicatedGlobalVectorStoreRoundtrip { op, dsts, .. }
            | Inst::ConstVectorLoad { op, dsts, .. }
            | Inst::PredicatedConstVectorLoad { op, dsts, .. }
            | Inst::LocalVectorMem { op, dsts, .. }
            | Inst::PredicatedLocalVectorMem { op, dsts, .. }
            | Inst::SharedVectorMem { op, dsts, .. }
            | Inst::PredicatedSharedVectorMem { op, dsts, .. } => {
                for dst in dsts.iter().take(op.lanes()) {
                    self.forget_tracked_write(*dst);
                }
            }
            Inst::Bin { dst, .. }
            | Inst::PackedAdd { dst, .. }
            | Inst::PackedMinMax { dst, .. }
            | Inst::Scalar16 { dst, .. }
            | Inst::Scalar16Setp { dst, .. }
            | Inst::Scalar16Selp { dst, .. }
            | Inst::GlobalLoad { dst, .. }
            | Inst::PredicatedGlobalLoad { dst, .. }
            | Inst::GlobalStoreRoundtrip { dst, .. }
            | Inst::PredicatedGlobalStoreRoundtrip { dst, .. }
            | Inst::ConstLoad { dst, .. }
            | Inst::PredicatedConstLoad { dst, .. }
            | Inst::LocalMem { dst, .. }
            | Inst::PredicatedLocalMem { dst, .. }
            | Inst::SharedMem { dst, .. }
            | Inst::PredicatedSharedMem { dst, .. }
            | Inst::F32Arith { dst, .. }
            | Inst::PredicatedF32Arith { dst, .. }
            | Inst::F32RoundingArith { dst, .. }
            | Inst::PredicatedF32RoundingArith { dst, .. }
            | Inst::F32Unary { dst, .. }
            | Inst::PredicatedF32Unary { dst, .. }
            | Inst::F32Cvt { dst, .. }
            | Inst::PredicatedF32Cvt { dst, .. }
            | Inst::F32FloatCvt { dst, .. }
            | Inst::PredicatedF32FloatCvt { dst, .. }
            | Inst::F32SpecialMath { dst, .. }
            | Inst::PredicatedF32SpecialMath { dst, .. }
            | Inst::F32Selp { dst, .. }
            | Inst::PredicatedF32Selp { dst, .. }
            | Inst::F64Arith { dst, .. }
            | Inst::PredicatedF64Arith { dst, .. }
            | Inst::F64RoundingArith { dst, .. }
            | Inst::PredicatedF64RoundingArith { dst, .. }
            | Inst::F64Unary { dst, .. }
            | Inst::PredicatedF64Unary { dst, .. }
            | Inst::F64Cvt { dst, .. }
            | Inst::PredicatedF64Cvt { dst, .. }
            | Inst::F64FloatCvt { dst, .. }
            | Inst::PredicatedF64FloatCvt { dst, .. }
            | Inst::F64SpecialMath { dst, .. }
            | Inst::PredicatedF64SpecialMath { dst, .. }
            | Inst::F64Selp { dst, .. }
            | Inst::PredicatedF64Selp { dst, .. }
            | Inst::Sel { dst, .. }
            | Inst::PredicatedSel { dst, .. }
            | Inst::PredicatedBin { dst, .. }
            | Inst::PredicatedPackedAdd { dst, .. }
            | Inst::PredicatedPackedMinMax { dst, .. }
            | Inst::PredicatedScalar16 { dst, .. }
            | Inst::SetpBoolBin { dst, .. }
            | Inst::SetpDualBin { dst, .. }
            | Inst::PredLogicBin { dst, .. }
            | Inst::PredicatedShift { dst, .. }
            | Inst::Shift { dst, .. }
            | Inst::RegShift { dst, .. }
            | Inst::PredicatedRegShift { dst, .. }
            | Inst::Unary { dst, .. }
            | Inst::PredicatedUnary { dst, .. }
            | Inst::SpecialReg { dst, .. }
            | Inst::PredicatedSpecialReg { dst, .. }
            | Inst::Cvt { dst, .. }
            | Inst::PredicatedCvt { dst, .. }
            | Inst::NarrowCvt { dst, .. }
            | Inst::PredicatedNarrowCvt { dst, .. }
            | Inst::WideCvt { dst, .. }
            | Inst::PredicatedWideCvt { dst, .. }
            | Inst::Szext { dst, .. }
            | Inst::PredicatedSzext { dst, .. }
            | Inst::Bfind { dst, .. }
            | Inst::PredicatedBfind { dst, .. }
            | Inst::Fns { dst, .. }
            | Inst::PredicatedFns { dst, .. }
            | Inst::RegFns { dst, .. }
            | Inst::PredicatedRegFns { dst, .. }
            | Inst::DivRem { dst, .. }
            | Inst::RegDivRem { dst, .. }
            | Inst::PredicatedDivRem { dst, .. }
            | Inst::PredicatedRegDivRem { dst, .. }
            | Inst::Mad24 { dst, .. }
            | Inst::PredicatedMad24 { dst, .. }
            | Inst::Mul24 { dst, .. }
            | Inst::PredicatedMul24 { dst, .. }
            | Inst::SubwordWide { dst, .. }
            | Inst::PredicatedSubwordWide { dst, .. }
            | Inst::MulWide { dst, .. }
            | Inst::PredicatedMulWide { dst, .. }
            | Inst::MadWide { dst, .. }
            | Inst::PredicatedMadWide { dst, .. }
            | Inst::WideInt { dst, .. }
            | Inst::PredicatedWideInt { dst, .. }
            | Inst::WideMad64 { dst, .. }
            | Inst::PredicatedWideMad64 { dst, .. }
            | Inst::WideSetpBin { dst, .. }
            | Inst::WideSetpBoolBin { dst, .. }
            | Inst::WideSelp { dst, .. }
            | Inst::WideUnary { dst, .. }
            | Inst::PredicatedWideUnary { dst, .. }
            | Inst::WideShift { dst, .. }
            | Inst::RegWideShift { dst, .. }
            | Inst::PredicatedWideShift { dst, .. }
            | Inst::PredicatedRegWideShift { dst, .. }
            | Inst::WideDivRem { dst, .. }
            | Inst::PredicatedWideDivRem { dst, .. }
            | Inst::Sad { dst, .. }
            | Inst::PredicatedSad { dst, .. }
            | Inst::Slct { dst, .. }
            | Inst::PredicatedSlct { dst, .. }
            | Inst::Dp4a { dst, .. }
            | Inst::PredicatedDp4a { dst, .. }
            | Inst::Dp2a { dst, .. }
            | Inst::PredicatedDp2a { dst, .. }
            | Inst::Video { dst, .. }
            | Inst::PredicatedVideo { dst, .. }
            | Inst::Mad { dst, .. }
            | Inst::PredicatedMad { dst, .. }
            | Inst::MadHi { dst, .. }
            | Inst::PredicatedMadHi { dst, .. }
            | Inst::Lop3 { dst, .. }
            | Inst::PredicatedLop3 { dst, .. }
            | Inst::Funnel { dst, .. }
            | Inst::RegFunnel { dst, .. }
            | Inst::PredicatedFunnel { dst, .. }
            | Inst::Bfe { dst, .. }
            | Inst::PredicatedBfe { dst, .. }
            | Inst::WideBfe { dst, .. }
            | Inst::RegWideBfe { dst, .. }
            | Inst::PredicatedWideBfe { dst, .. }
            | Inst::PredicatedRegWideBfe { dst, .. }
            | Inst::Bfi { dst, .. }
            | Inst::PredicatedBfi { dst, .. }
            | Inst::WideBfi { dst, .. }
            | Inst::RegWideBfi { dst, .. }
            | Inst::PredicatedWideBfi { dst, .. }
            | Inst::PredicatedRegWideBfi { dst, .. }
            | Inst::Bmsk { dst, .. }
            | Inst::PredicatedBmsk { dst, .. } => self.forget_tracked_write(*dst),
        }
    }

    fn gen_terminator(&mut self, u: &mut Unstructured, i: usize, n_blocks: usize) -> Result<Term> {
        let pick: u8 = u.arbitrary()?;
        let fwd_lo = i + 1;
        let fwd_hi = n_blocks - 1;
        if pick < 102 {
            Ok(Term::Branch(u.int_in_range(fwd_lo..=fwd_hi)?))
        } else if pick < 178 || !self.cfg.emit_arbitrary_loops {
            Ok(Term::CondBranch {
                cmp: pick_cmp(u, self.cfg.emit_signed_cmp)?,
                a: self.pick_operand(u)?,
                b: self.pick_operand(u)?,
                pred: self.alloc_pred(),
                t: u.int_in_range(fwd_lo..=fwd_hi)?,
                f: u.int_in_range(fwd_lo..=fwd_hi)?,
            })
        } else {
            let init = u.int_in_range(0..=self.cfg.max_loop_iters)?;
            let ctr = self.alloc_counter(init);
            Ok(Term::Loop {
                ctr,
                pred: self.alloc_pred(),
                back: u.int_in_range(0..=i)?,
                fwd: u.int_in_range(fwd_lo..=fwd_hi)?,
            })
        }
    }

    fn gen_basic(&mut self, u: &mut Unstructured) -> Result<StructuredStmt> {
        let n_insts = u.int_in_range(
            self.cfg.min_insts_per_block
                ..=self
                    .cfg
                    .max_insts_per_block
                    .max(self.cfg.min_insts_per_block),
        )?;
        let mut insts = Vec::with_capacity(n_insts);
        for _ in 0..n_insts {
            let inst = self.gen_inst(u)?;
            self.note_inst(&inst);
            insts.push(inst);
        }
        Ok(StructuredStmt::Basic(insts))
    }

    fn gen_structured_seq(
        &mut self,
        u: &mut Unstructured,
        budget: usize,
        depth: usize,
    ) -> Result<Vec<StructuredStmt>> {
        let mut remaining = budget.max(1);
        let mut out = Vec::new();
        while remaining > 0 {
            let pick: u8 = u.arbitrary()?;
            let can_nest = depth < self.cfg.max_structured_depth;
            if can_nest && self.cfg.emit_structured_loops && pick < 64 {
                let body_budget = u.int_in_range(1..=remaining)?;
                let init = u.int_in_range(0..=self.cfg.max_loop_iters)?;
                let ctr = self.alloc_counter(init);
                let pred = self.alloc_pred();
                let body = self.gen_structured_seq(u, body_budget, depth + 1)?;
                out.push(StructuredStmt::Loop { ctr, pred, body });
                remaining -= body_budget;
            } else if can_nest && pick < 128 && remaining >= 2 {
                let total_budget = u.int_in_range(2..=remaining)?;
                let then_budget = u.int_in_range(1..=total_budget - 1)?;
                let else_budget = total_budget - then_budget;
                out.push(StructuredStmt::IfElse {
                    cmp: pick_cmp(u, self.cfg.emit_signed_cmp)?,
                    a: self.pick_operand(u)?,
                    b: self.pick_operand(u)?,
                    pred: self.alloc_pred(),
                    then_body: self.gen_structured_seq(u, then_budget, depth + 1)?,
                    else_body: self.gen_structured_seq(u, else_budget, depth + 1)?,
                });
                remaining -= total_budget;
            } else {
                out.push(self.gen_basic(u)?);
                remaining -= 1;
            }
        }
        Ok(out)
    }

    fn emit(&self) -> String {
        let mut s = String::with_capacity(4096);
        self.emit_prologue(&mut s);
        writeln!(s, "    bra             block_0;").unwrap();
        writeln!(s).unwrap();

        for (i, blk) in self.blocks.iter().enumerate() {
            writeln!(s, "block_{i}:").unwrap();
            for inst in &blk.insts {
                self.emit_inst(&mut s, inst);
            }
            self.emit_terminator(&mut s, &blk.term);
            writeln!(s).unwrap();
        }

        self.emit_epilogue(&mut s);
        s
    }

    fn emit_structured(&self, body: &[StructuredStmt]) -> String {
        let mut s = String::with_capacity(4096);
        let mut next_label = 0u32;
        self.emit_prologue(&mut s);
        self.emit_structured_seq(&mut s, body, &mut next_label);
        writeln!(s, "    bra             exit;").unwrap();
        writeln!(s).unwrap();
        self.emit_epilogue(&mut s);
        s
    }

    fn emit_prologue(&self, s: &mut String) {
        let tid_reg = self.tid_reg();
        let total_regs = (self.wide_scratch_hi_reg() + 1).max(1);

        writeln!(s, ".version 8.8").unwrap();
        writeln!(s, ".target {TARGET_ARCH}").unwrap();
        writeln!(s, ".address_size 64").unwrap();
        write!(
            s,
            ".const .align 16 .b8 fuzzx_const[{CONST_MEM_BYTES}] = {{"
        )
        .unwrap();
        for i in 0..CONST_MEM_BYTES {
            if i > 0 {
                write!(s, ",").unwrap();
            }
            write!(s, "{}", (i.wrapping_mul(17).wrapping_add(3)) & 0xff).unwrap();
        }
        writeln!(s, "}};").unwrap();
        writeln!(s).unwrap();
        writeln!(s, ".visible .entry {KERNEL_NAME}(").unwrap();
        writeln!(s, "    .param .u64 in_ptr,").unwrap();
        writeln!(s, "    .param .u64 out_ptr,").unwrap();
        writeln!(s, "    .param .u32 in_n").unwrap();
        writeln!(s, ")").unwrap();
        writeln!(s, "{{").unwrap();

        if self.n_pred > 0 {
            writeln!(s, "    .reg .pred  %p<{}>;", self.n_pred).unwrap();
        }
        writeln!(s, "    .reg .b16   %h<4>;").unwrap();
        writeln!(s, "    .reg .b32   %r<{total_regs}>;").unwrap();
        writeln!(s, "    .reg .b64   %rd<10>;").unwrap();
        writeln!(s, "    .reg .f32   %f<4>;").unwrap();
        writeln!(s, "    .reg .f64   %fd<4>;").unwrap();
        writeln!(
            s,
            "    .local .align 16 .b8 fuzzx_local[{LOCAL_MEM_BYTES}];"
        )
        .unwrap();
        writeln!(
            s,
            "    .shared .align 16 .b8 fuzzx_shared[{SHARED_MEM_BYTES}];"
        )
        .unwrap();
        writeln!(s).unwrap();

        // Prologue: load params; compute tid into the reserved tid reg; load
        // input into a working reg; initialize remaining working regs and
        // loop counters.
        writeln!(s, "    ld.param.u64    %rd0, [in_ptr];").unwrap();
        writeln!(s, "    ld.param.u64    %rd1, [out_ptr];").unwrap();
        writeln!(s, "    ld.param.u32    %r0, [in_n];").unwrap();
        writeln!(s, "    mov.u32         %r{tid_reg}, %tid.x;").unwrap();
        writeln!(s, "    cvta.to.global.u64 %rd2, %rd0;").unwrap();
        writeln!(s, "    mul.wide.u32    %rd3, %r{tid_reg}, 4;").unwrap();
        writeln!(s, "    add.s64         %rd2, %rd2, %rd3;").unwrap();
        writeln!(s, "    ld.global.u32   %r2, [%rd2];").unwrap();
        writeln!(s, "    mov.u32         %r1, %r{tid_reg};").unwrap();
        for i in 3..self.n_working {
            // Round-robin between small constants and prior regs to spread state.
            let init = match i % 4 {
                0 => format!("{i}"),
                1 => format!("%r{tid_reg}"),
                2 => "%r2".to_string(),
                _ => "%r0".to_string(),
            };
            writeln!(s, "    mov.u32         %r{i}, {init};").unwrap();
        }
        for &(reg, init) in &self.counters {
            writeln!(s, "    mov.u32         %r{reg}, {init};").unwrap();
        }
        writeln!(s).unwrap();
    }

    fn emit_epilogue(&self, s: &mut String) {
        let tid_reg = self.tid_reg();

        // Epilogue: store output regs to out[tid * N_OUTPUTS * 4 ..]. Uses the
        // reserved tid reg, NOT %r1 (which the body is free to clobber).
        writeln!(s, "exit:").unwrap();
        writeln!(s, "    cvta.to.global.u64 %rd4, %rd1;").unwrap();
        writeln!(
            s,
            "    mul.wide.u32    %rd5, %r{tid_reg}, {};",
            N_OUTPUTS * 4
        )
        .unwrap();
        writeln!(s, "    add.s64         %rd4, %rd4, %rd5;").unwrap();
        for k in 0..N_OUTPUTS {
            writeln!(s, "    st.global.u32   [%rd4 + {}], %r{k};", k * 4).unwrap();
        }
        writeln!(s, "    ret;").unwrap();
        writeln!(s, "}}").unwrap();
    }

    fn emit_structured_seq(&self, s: &mut String, body: &[StructuredStmt], next_label: &mut u32) {
        for stmt in body {
            self.emit_structured_stmt(s, stmt, next_label);
        }
    }

    fn emit_structured_stmt(&self, s: &mut String, stmt: &StructuredStmt, next_label: &mut u32) {
        match stmt {
            StructuredStmt::Basic(insts) => {
                for inst in insts {
                    self.emit_inst(s, inst);
                }
            }
            StructuredStmt::IfElse {
                cmp,
                a,
                b,
                pred,
                then_body,
                else_body,
            } => {
                let id = *next_label;
                *next_label += 1;
                write!(s, "    {:<13} %p{pred}, ", cmp.mnemonic()).unwrap();
                a.emit(s);
                write!(s, ", ").unwrap();
                b.emit(s);
                writeln!(s, ";").unwrap();
                writeln!(s, "    @%p{pred} bra   structured_if_{id}_then;").unwrap();
                writeln!(s, "    bra             structured_if_{id}_else;").unwrap();
                writeln!(s, "structured_if_{id}_then:").unwrap();
                self.emit_structured_seq(s, then_body, next_label);
                writeln!(s, "    bra             structured_if_{id}_done;").unwrap();
                writeln!(s, "structured_if_{id}_else:").unwrap();
                self.emit_structured_seq(s, else_body, next_label);
                writeln!(s, "    bra             structured_if_{id}_done;").unwrap();
                writeln!(s, "structured_if_{id}_done:").unwrap();
            }
            StructuredStmt::Loop { ctr, pred, body } => {
                let id = *next_label;
                *next_label += 1;
                writeln!(s, "structured_loop_{id}_header:").unwrap();
                writeln!(s, "    setp.eq.u32   %p{pred}, %r{ctr}, 0;").unwrap();
                writeln!(s, "    @%p{pred} bra   structured_loop_{id}_done;").unwrap();
                writeln!(s, "    sub.u32         %r{ctr}, %r{ctr}, 1;").unwrap();
                self.emit_structured_seq(s, body, next_label);
                writeln!(s, "    bra             structured_loop_{id}_header;").unwrap();
                writeln!(s, "structured_loop_{id}_done:").unwrap();
            }
        }
    }

    fn emit_inst_predicate_setup(
        &self,
        s: &mut String,
        cmp: CmpOp,
        ca: Operand,
        cb: Operand,
        pred: u32,
    ) {
        write!(s, "    {:<13} %p{}, ", cmp.mnemonic(), pred_id(pred)).unwrap();
        ca.emit(s);
        write!(s, ", ").unwrap();
        cb.emit(s);
        writeln!(s, ";").unwrap();
    }

    fn emit_vector_regs(s: &mut String, regs: [u32; 4], lanes: usize) {
        write!(s, "{{").unwrap();
        for (i, reg) in regs.iter().take(lanes).enumerate() {
            if i > 0 {
                write!(s, ", ").unwrap();
            }
            write!(s, "%r{reg}").unwrap();
        }
        write!(s, "}}").unwrap();
    }

    fn emit_vector_wide_regs(s: &mut String, lanes: usize) {
        write!(s, "{{").unwrap();
        for i in 0..lanes {
            if i > 0 {
                write!(s, ", ").unwrap();
            }
            write!(s, "%rd{}", 4 + i).unwrap();
        }
        write!(s, "}}").unwrap();
    }

    fn emit_vector_memory_load(
        &self,
        s: &mut String,
        mnemonic: &str,
        op: VectorMemOp,
        dsts: [u32; 4],
        addr: &str,
        offset: u32,
        pred: Option<u32>,
    ) {
        let lanes = op.lanes();
        if let Some(pred) = pred {
            write!(s, "    {} {:<12} ", pred_guard(pred), mnemonic).unwrap();
        } else {
            write!(s, "    {mnemonic:<17} ").unwrap();
        }
        if op.is_wide() {
            Self::emit_vector_wide_regs(s, lanes);
        } else {
            Self::emit_vector_regs(s, dsts, lanes);
        }
        writeln!(s, ", [{addr} + {offset}];").unwrap();

        if op.is_wide() {
            let scratch = self.wide_scratch_hi_reg();
            for (i, dst) in dsts.iter().take(lanes).enumerate() {
                if let Some(pred) = pred {
                    writeln!(
                        s,
                        "    {} mov.b64 {{%r{dst}, %r{scratch}}}, %rd{};",
                        pred_guard(pred),
                        4 + i
                    )
                    .unwrap();
                } else {
                    writeln!(
                        s,
                        "    mov.b64       {{%r{dst}, %r{scratch}}}, %rd{};",
                        4 + i
                    )
                    .unwrap();
                }
            }
        }
    }

    fn emit_vector_memory_store(
        &self,
        s: &mut String,
        mnemonic: &str,
        op: VectorMemOp,
        srcs: [u32; 4],
        addr: &str,
        offset: u32,
        pred: Option<u32>,
    ) {
        let lanes = op.lanes();
        if op.is_wide() {
            for (i, src) in srcs.iter().take(lanes).enumerate() {
                writeln!(s, "    mov.b64       %rd{}, {{%r{src}, %r{src}}};", 4 + i).unwrap();
            }
        }
        if let Some(pred) = pred {
            write!(
                s,
                "    {} {:<12} [{addr} + {offset}], ",
                pred_guard(pred),
                mnemonic
            )
            .unwrap();
        } else {
            write!(s, "    {mnemonic:<17} [{addr} + {offset}], ").unwrap();
        }
        if op.is_wide() {
            Self::emit_vector_wide_regs(s, lanes);
        } else {
            Self::emit_vector_regs(s, srcs, lanes);
        }
        writeln!(s, ";").unwrap();
    }

    fn emit_memory_load(
        &self,
        s: &mut String,
        mnemonic: &str,
        dst: u32,
        addr: &str,
        offset: u32,
        is_wide: bool,
        pred: Option<u32>,
    ) {
        let narrow_bit_mask = if mnemonic.ends_with(".b8") {
            Some(0xff)
        } else if mnemonic.ends_with(".b16") {
            Some(0xffff)
        } else {
            None
        };
        if is_wide {
            let scratch = self.wide_scratch_hi_reg();
            if let Some(pred) = pred {
                let guard = pred_guard(pred);
                writeln!(s, "    {guard} {mnemonic:<8} %rd7, [{addr} + {offset}];").unwrap();
                writeln!(s, "    {guard} mov.b64 {{%r{dst}, %r{scratch}}}, %rd7;").unwrap();
            } else {
                writeln!(s, "    {mnemonic:<13} %rd7, [{addr} + {offset}];").unwrap();
                writeln!(s, "    mov.b64       {{%r{dst}, %r{scratch}}}, %rd7;").unwrap();
            }
        } else if let Some(pred) = pred {
            let guard = pred_guard(pred);
            writeln!(s, "    {guard} {mnemonic:<8} %r{dst}, [{addr} + {offset}];").unwrap();
            if let Some(mask) = narrow_bit_mask {
                writeln!(s, "    {guard} and.b32  %r{dst}, %r{dst}, {mask};").unwrap();
            }
        } else {
            writeln!(s, "    {mnemonic:<13} %r{dst}, [{addr} + {offset}];").unwrap();
            if let Some(mask) = narrow_bit_mask {
                writeln!(s, "    and.b32       %r{dst}, %r{dst}, {mask};").unwrap();
            }
        }
    }

    fn emit_memory_store(
        &self,
        s: &mut String,
        mnemonic: &str,
        src: u32,
        addr: &str,
        offset: u32,
        is_wide: bool,
        pred: Option<u32>,
    ) {
        if is_wide {
            writeln!(s, "    mov.b64       %rd7, {{%r{src}, %r{src}}};").unwrap();
            if let Some(pred) = pred {
                writeln!(
                    s,
                    "    {} {:<8} [{addr} + {offset}], %rd7;",
                    pred_guard(pred),
                    mnemonic
                )
                .unwrap();
            } else {
                writeln!(s, "    {mnemonic:<13} [{addr} + {offset}], %rd7;").unwrap();
            }
        } else if let Some(pred) = pred {
            writeln!(
                s,
                "    {} {:<8} [{addr} + {offset}], %r{src};",
                pred_guard(pred),
                mnemonic
            )
            .unwrap();
        } else {
            writeln!(s, "    {mnemonic:<13} [{addr} + {offset}], %r{src};").unwrap();
        }
    }

    fn emit_wide_divisor(&self, s: &mut String, op: WideDivRemOp, divisor: WideDivisor) -> String {
        match divisor {
            WideDivisor::Imm(value) => value.to_string(),
            WideDivisor::Reg(src) => {
                let scratch = self.wide_scratch_hi_reg();
                write!(s, "    or.b32        %r{scratch}, ").unwrap();
                src.emit(s);
                writeln!(s, ", 1;").unwrap();
                write!(s, "    {:<13} %rd7, ", op.cvt_mnemonic()).unwrap();
                writeln!(s, "%r{scratch};").unwrap();
                "%rd7".to_string()
            }
        }
    }

    fn emit_sanitized_f32_operand(&self, s: &mut String, freg: u32, operand: Operand) {
        let scratch = self.wide_scratch_hi_reg();
        write!(s, "    and.b32       %r{scratch}, ").unwrap();
        operand.emit(s);
        writeln!(s, ", {FLOAT_INPUT_MASK};").unwrap();
        writeln!(s, "    cvt.rn.f32.u32 %f{freg}, %r{scratch};").unwrap();
    }

    fn emit_raw_f32_operand(&self, s: &mut String, freg: u32, operand: Operand, signed: bool) {
        let cvt = if signed {
            "cvt.rn.f32.s32"
        } else {
            "cvt.rn.f32.u32"
        };
        write!(s, "    {cvt:<13} %f{freg}, ").unwrap();
        operand.emit(s);
        writeln!(s, ";").unwrap();
    }

    fn emit_raw_f64_operand(&self, s: &mut String, freg: u32, operand: Operand, signed: bool) {
        let cvt = if signed {
            "cvt.rn.f64.s32"
        } else {
            "cvt.rn.f64.u32"
        };
        write!(s, "    {cvt:<13} %fd{freg}, ").unwrap();
        operand.emit(s);
        writeln!(s, ";").unwrap();
    }

    fn emit_raw_wide_operand(&self, s: &mut String, wreg: u32, operand: Operand, signed: bool) {
        let cvt = if signed { "cvt.s64.s32" } else { "cvt.u64.u32" };
        write!(s, "    {cvt:<13} %rd{wreg}, ").unwrap();
        operand.emit(s);
        writeln!(s, ";").unwrap();
    }

    fn emit_sanitized_f32_math_operand(
        &self,
        s: &mut String,
        freg: u32,
        operand: Operand,
        domain: FloatInputDomain,
    ) {
        let scratch = self.wide_scratch_hi_reg();
        write!(s, "    and.b32       %r{scratch}, ").unwrap();
        operand.emit(s);
        let mask = match domain {
            FloatInputDomain::NonNegative | FloatInputDomain::Positive => FLOAT_INPUT_MASK,
            FloatInputDomain::SmallNonNegative => 7,
        };
        writeln!(s, ", {mask};").unwrap();
        if matches!(domain, FloatInputDomain::Positive) {
            writeln!(s, "    add.u32       %r{scratch}, %r{scratch}, 1;").unwrap();
        }
        writeln!(s, "    cvt.rn.f32.u32 %f{freg}, %r{scratch};").unwrap();
    }

    fn emit_sanitized_f64_operand(&self, s: &mut String, freg: u32, operand: Operand) {
        let scratch = self.wide_scratch_hi_reg();
        write!(s, "    and.b32       %r{scratch}, ").unwrap();
        operand.emit(s);
        writeln!(s, ", {FLOAT_INPUT_MASK};").unwrap();
        writeln!(s, "    cvt.rn.f64.u32 %fd{freg}, %r{scratch};").unwrap();
    }

    fn emit_sanitized_f64_math_operand(
        &self,
        s: &mut String,
        freg: u32,
        operand: Operand,
        domain: FloatInputDomain,
    ) {
        let scratch = self.wide_scratch_hi_reg();
        write!(s, "    and.b32       %r{scratch}, ").unwrap();
        operand.emit(s);
        let mask = match domain {
            FloatInputDomain::NonNegative | FloatInputDomain::Positive => FLOAT_INPUT_MASK,
            FloatInputDomain::SmallNonNegative => 7,
        };
        writeln!(s, ", {mask};").unwrap();
        if matches!(domain, FloatInputDomain::Positive) {
            writeln!(s, "    add.u32       %r{scratch}, %r{scratch}, 1;").unwrap();
        }
        writeln!(s, "    cvt.rn.f64.u32 %fd{freg}, %r{scratch};").unwrap();
    }

    fn emit_inst(&self, s: &mut String, inst: &Inst) {
        match *inst {
            Inst::Bin { op, dst, a, b } => {
                write!(s, "    {:<13} %r{dst}, ", op.mnemonic()).unwrap();
                a.emit(s);
                write!(s, ", ").unwrap();
                b.emit(s);
                writeln!(s, ";").unwrap();
            }
            Inst::PackedAdd { op, dst, a, b } => {
                write!(s, "    {:<13} %r{dst}, ", op.mnemonic()).unwrap();
                a.emit(s);
                write!(s, ", ").unwrap();
                b.emit(s);
                writeln!(s, ";").unwrap();
            }
            Inst::PackedMinMax { op, dst, a, b } => {
                write!(s, "    {:<13} %r{dst}, ", op.mnemonic()).unwrap();
                a.emit(s);
                write!(s, ", ").unwrap();
                b.emit(s);
                writeln!(s, ";").unwrap();
            }
            Inst::Scalar16 { op, dst, a, b } => {
                write!(s, "    {:<13} %h0, ", op.input_cvt_mnemonic()).unwrap();
                a.emit(s);
                writeln!(s, ";").unwrap();
                if op.uses_h1() {
                    write!(s, "    {:<13} %h1, ", op.input_cvt_mnemonic()).unwrap();
                    b.emit(s);
                    writeln!(s, ";").unwrap();
                }
                if op.is_unary() {
                    writeln!(s, "    {:<13} %h2, %h0;", op.mnemonic()).unwrap();
                } else if op.is_shift() {
                    write!(s, "    {:<13} %h2, %h0, ", op.mnemonic()).unwrap();
                    b.emit(s);
                    writeln!(s, ";").unwrap();
                } else {
                    writeln!(s, "    {:<13} %h2, %h0, %h1;", op.mnemonic()).unwrap();
                }
                writeln!(s, "    {:<13} %r{dst}, %h2;", op.output_cvt_mnemonic()).unwrap();
            }
            Inst::Scalar16Setp {
                cmp,
                dst,
                a,
                b,
                pred,
            } => {
                write!(s, "    {:<13} %h0, ", cmp.scalar16_input_cvt_mnemonic()).unwrap();
                a.emit(s);
                writeln!(s, ";").unwrap();
                write!(s, "    {:<13} %h1, ", cmp.scalar16_input_cvt_mnemonic()).unwrap();
                b.emit(s);
                writeln!(s, ";").unwrap();
                writeln!(
                    s,
                    "    {:<13} %p{pred}, %h0, %h1;",
                    cmp.scalar16_setp_mnemonic()
                )
                .unwrap();
                write!(s, "    selp.b32      %r{dst}, ").unwrap();
                a.emit(s);
                write!(s, ", ").unwrap();
                b.emit(s);
                writeln!(s, ", %p{pred};").unwrap();
            }
            Inst::Scalar16Set { cmp, dst, a, b } => {
                write!(s, "    {:<13} %h0, ", cmp.scalar16_input_cvt_mnemonic()).unwrap();
                a.emit(s);
                writeln!(s, ";").unwrap();
                write!(s, "    {:<13} %h1, ", cmp.scalar16_input_cvt_mnemonic()).unwrap();
                b.emit(s);
                writeln!(s, ";").unwrap();
                writeln!(
                    s,
                    "    {:<13} %r{dst}, %h0, %h1;",
                    cmp.scalar16_set_mnemonic()
                )
                .unwrap();
            }
            Inst::Scalar16Selp {
                cmp,
                dst,
                a,
                b,
                pred,
            } => {
                write!(s, "    {:<13} %h0, ", cmp.scalar16_input_cvt_mnemonic()).unwrap();
                a.emit(s);
                writeln!(s, ";").unwrap();
                write!(s, "    {:<13} %h1, ", cmp.scalar16_input_cvt_mnemonic()).unwrap();
                b.emit(s);
                writeln!(s, ";").unwrap();
                writeln!(
                    s,
                    "    {:<13} %p{pred}, %h0, %h1;",
                    cmp.scalar16_setp_mnemonic()
                )
                .unwrap();
                writeln!(
                    s,
                    "    {:<13} %h2, %h0, %h1, %p{pred};",
                    cmp.scalar16_selp_mnemonic()
                )
                .unwrap();
                writeln!(
                    s,
                    "    {:<13} %r{dst}, %h2;",
                    cmp.scalar16_output_cvt_mnemonic()
                )
                .unwrap();
            }
            Inst::GlobalLoad {
                op,
                cache,
                volatile,
                uniform,
                dst,
                offset,
            } => {
                writeln!(s, "    cvta.to.global.u64 %rd6, %rd0;").unwrap();
                let mnemonic = if volatile {
                    op.volatile_mnemonic()
                } else if uniform {
                    op.uniform_mnemonic()
                } else {
                    op.mnemonic_with_cache(cache)
                };
                self.emit_memory_load(s, &mnemonic, dst, "%rd6", offset, op.is_wide(), None);
            }
            Inst::PredicatedGlobalLoad {
                op,
                cache,
                volatile,
                uniform,
                dst,
                offset,
                cmp,
                ca,
                cb,
                pred,
            } => {
                self.emit_inst_predicate_setup(s, cmp, ca, cb, pred);
                writeln!(s, "    cvta.to.global.u64 %rd6, %rd0;").unwrap();
                let mnemonic = if volatile {
                    op.volatile_mnemonic()
                } else if uniform {
                    op.uniform_mnemonic()
                } else {
                    op.mnemonic_with_cache(cache)
                };
                self.emit_memory_load(s, &mnemonic, dst, "%rd6", offset, op.is_wide(), Some(pred));
            }
            Inst::GlobalStoreRoundtrip {
                op,
                store_cache,
                volatile,
                dst,
                src,
                offset,
            } => {
                let tid_reg = self.tid_reg();
                writeln!(s, "    cvta.to.global.u64 %rd8, %rd1;").unwrap();
                writeln!(s, "    mul.wide.u32  %rd9, %r{tid_reg}, {};", N_OUTPUTS * 4).unwrap();
                writeln!(s, "    add.s64       %rd8, %rd8, %rd9;").unwrap();
                let store_mnemonic = if volatile {
                    op.volatile_store_mnemonic()
                } else {
                    op.store_mnemonic_with_cache(store_cache)
                };
                let load_mnemonic = if volatile {
                    op.volatile_load_mnemonic()
                } else {
                    op.load_mnemonic().to_string()
                };
                self.emit_memory_store(s, &store_mnemonic, src, "%rd8", offset, op.is_wide(), None);
                self.emit_memory_load(s, &load_mnemonic, dst, "%rd8", offset, op.is_wide(), None);
            }
            Inst::PredicatedGlobalStoreRoundtrip {
                op,
                store_cache,
                volatile,
                dst,
                src,
                offset,
                cmp,
                ca,
                cb,
                pred,
            } => {
                self.emit_inst_predicate_setup(s, cmp, ca, cb, pred);
                let tid_reg = self.tid_reg();
                writeln!(s, "    cvta.to.global.u64 %rd8, %rd1;").unwrap();
                writeln!(s, "    mul.wide.u32  %rd9, %r{tid_reg}, {};", N_OUTPUTS * 4).unwrap();
                writeln!(s, "    add.s64       %rd8, %rd8, %rd9;").unwrap();
                let store_mnemonic = if volatile {
                    op.volatile_store_mnemonic()
                } else {
                    op.store_mnemonic_with_cache(store_cache)
                };
                let load_mnemonic = if volatile {
                    op.volatile_load_mnemonic()
                } else {
                    op.load_mnemonic().to_string()
                };
                self.emit_memory_store(
                    s,
                    &store_mnemonic,
                    src,
                    "%rd8",
                    offset,
                    op.is_wide(),
                    Some(pred),
                );
                self.emit_memory_load(
                    s,
                    &load_mnemonic,
                    dst,
                    "%rd8",
                    offset,
                    op.is_wide(),
                    Some(pred),
                );
            }
            Inst::ConstLoad { op, dst, offset } => {
                writeln!(s, "    mov.u64       %rd6, fuzzx_const;").unwrap();
                self.emit_memory_load(s, op.mnemonic(), dst, "%rd6", offset, op.is_wide(), None);
            }
            Inst::PredicatedConstLoad {
                op,
                dst,
                offset,
                cmp,
                ca,
                cb,
                pred,
            } => {
                self.emit_inst_predicate_setup(s, cmp, ca, cb, pred);
                writeln!(s, "    mov.u64       %rd6, fuzzx_const;").unwrap();
                self.emit_memory_load(
                    s,
                    op.mnemonic(),
                    dst,
                    "%rd6",
                    offset,
                    op.is_wide(),
                    Some(pred),
                );
            }
            Inst::LocalMem {
                op,
                dst,
                src,
                offset,
            } => {
                writeln!(s, "    mov.u64       %rd6, fuzzx_local;").unwrap();
                self.emit_memory_store(
                    s,
                    op.store_mnemonic(),
                    src,
                    "%rd6",
                    offset,
                    op.is_wide(),
                    None,
                );
                self.emit_memory_load(
                    s,
                    op.load_mnemonic(),
                    dst,
                    "%rd6",
                    offset,
                    op.is_wide(),
                    None,
                );
            }
            Inst::PredicatedLocalMem {
                op,
                dst,
                src,
                offset,
                cmp,
                ca,
                cb,
                pred,
            } => {
                self.emit_inst_predicate_setup(s, cmp, ca, cb, pred);
                writeln!(s, "    mov.u64       %rd6, fuzzx_local;").unwrap();
                self.emit_memory_store(
                    s,
                    op.store_mnemonic(),
                    src,
                    "%rd6",
                    offset,
                    op.is_wide(),
                    Some(pred),
                );
                self.emit_memory_load(
                    s,
                    op.load_mnemonic(),
                    dst,
                    "%rd6",
                    offset,
                    op.is_wide(),
                    Some(pred),
                );
            }
            Inst::SharedMem {
                op,
                volatile,
                dst,
                src,
                offset,
            } => {
                let tid_reg = self.tid_reg();
                writeln!(s, "    mov.u64       %rd6, fuzzx_shared;").unwrap();
                writeln!(
                    s,
                    "    mul.wide.u32  %rd7, %r{tid_reg}, {SHARED_SLOT_BYTES};"
                )
                .unwrap();
                writeln!(s, "    add.s64       %rd6, %rd6, %rd7;").unwrap();
                let store_mnemonic = if volatile {
                    op.volatile_store_mnemonic()
                } else {
                    op.store_mnemonic().to_string()
                };
                let load_mnemonic = if volatile {
                    op.volatile_load_mnemonic()
                } else {
                    op.load_mnemonic().to_string()
                };
                self.emit_memory_store(s, &store_mnemonic, src, "%rd6", offset, op.is_wide(), None);
                self.emit_memory_load(s, &load_mnemonic, dst, "%rd6", offset, op.is_wide(), None);
            }
            Inst::PredicatedSharedMem {
                op,
                volatile,
                dst,
                src,
                offset,
                cmp,
                ca,
                cb,
                pred,
            } => {
                self.emit_inst_predicate_setup(s, cmp, ca, cb, pred);
                let tid_reg = self.tid_reg();
                writeln!(s, "    mov.u64       %rd6, fuzzx_shared;").unwrap();
                writeln!(
                    s,
                    "    mul.wide.u32  %rd7, %r{tid_reg}, {SHARED_SLOT_BYTES};"
                )
                .unwrap();
                writeln!(s, "    add.s64       %rd6, %rd6, %rd7;").unwrap();
                let store_mnemonic = if volatile {
                    op.volatile_store_mnemonic()
                } else {
                    op.store_mnemonic().to_string()
                };
                let load_mnemonic = if volatile {
                    op.volatile_load_mnemonic()
                } else {
                    op.load_mnemonic().to_string()
                };
                self.emit_memory_store(
                    s,
                    &store_mnemonic,
                    src,
                    "%rd6",
                    offset,
                    op.is_wide(),
                    Some(pred),
                );
                self.emit_memory_load(
                    s,
                    &load_mnemonic,
                    dst,
                    "%rd6",
                    offset,
                    op.is_wide(),
                    Some(pred),
                );
            }
            Inst::GlobalVectorLoad {
                op,
                cache,
                volatile,
                uniform,
                dsts,
                offset,
            } => {
                writeln!(s, "    cvta.to.global.u64 %rd6, %rd0;").unwrap();
                let mnemonic = if volatile {
                    op.volatile_global_load_mnemonic()
                } else if uniform {
                    op.uniform_global_load_mnemonic()
                } else {
                    op.global_load_mnemonic_with_cache(cache)
                };
                self.emit_vector_memory_load(s, &mnemonic, op, dsts, "%rd6", offset, None);
            }
            Inst::PredicatedGlobalVectorLoad {
                op,
                cache,
                volatile,
                uniform,
                dsts,
                offset,
                cmp,
                ca,
                cb,
                pred,
            } => {
                self.emit_inst_predicate_setup(s, cmp, ca, cb, pred);
                writeln!(s, "    cvta.to.global.u64 %rd6, %rd0;").unwrap();
                let mnemonic = if volatile {
                    op.volatile_global_load_mnemonic()
                } else if uniform {
                    op.uniform_global_load_mnemonic()
                } else {
                    op.global_load_mnemonic_with_cache(cache)
                };
                self.emit_vector_memory_load(s, &mnemonic, op, dsts, "%rd6", offset, Some(pred));
            }
            Inst::GlobalVectorStoreRoundtrip {
                op,
                store_cache,
                volatile,
                dsts,
                srcs,
                offset,
            } => {
                let tid_reg = self.tid_reg();
                writeln!(s, "    cvta.to.global.u64 %rd8, %rd1;").unwrap();
                writeln!(s, "    mul.wide.u32  %rd9, %r{tid_reg}, {};", N_OUTPUTS * 4).unwrap();
                writeln!(s, "    add.s64       %rd8, %rd8, %rd9;").unwrap();
                let store_mnemonic = if volatile {
                    op.volatile_global_store_mnemonic()
                } else {
                    op.global_store_mnemonic_with_cache(store_cache)
                };
                let load_mnemonic = if volatile {
                    op.volatile_global_load_mnemonic()
                } else {
                    op.global_load_mnemonic().to_string()
                };
                self.emit_vector_memory_store(s, &store_mnemonic, op, srcs, "%rd8", offset, None);
                self.emit_vector_memory_load(s, &load_mnemonic, op, dsts, "%rd8", offset, None);
            }
            Inst::PredicatedGlobalVectorStoreRoundtrip {
                op,
                store_cache,
                volatile,
                dsts,
                srcs,
                offset,
                cmp,
                ca,
                cb,
                pred,
            } => {
                self.emit_inst_predicate_setup(s, cmp, ca, cb, pred);
                let tid_reg = self.tid_reg();
                writeln!(s, "    cvta.to.global.u64 %rd8, %rd1;").unwrap();
                writeln!(s, "    mul.wide.u32  %rd9, %r{tid_reg}, {};", N_OUTPUTS * 4).unwrap();
                writeln!(s, "    add.s64       %rd8, %rd8, %rd9;").unwrap();
                let store_mnemonic = if volatile {
                    op.volatile_global_store_mnemonic()
                } else {
                    op.global_store_mnemonic_with_cache(store_cache)
                };
                let load_mnemonic = if volatile {
                    op.volatile_global_load_mnemonic()
                } else {
                    op.global_load_mnemonic().to_string()
                };
                self.emit_vector_memory_store(
                    s,
                    &store_mnemonic,
                    op,
                    srcs,
                    "%rd8",
                    offset,
                    Some(pred),
                );
                self.emit_vector_memory_load(
                    s,
                    &load_mnemonic,
                    op,
                    dsts,
                    "%rd8",
                    offset,
                    Some(pred),
                );
            }
            Inst::ConstVectorLoad { op, dsts, offset } => {
                writeln!(s, "    mov.u64       %rd6, fuzzx_const;").unwrap();
                self.emit_vector_memory_load(
                    s,
                    op.const_load_mnemonic(),
                    op,
                    dsts,
                    "%rd6",
                    offset,
                    None,
                );
            }
            Inst::PredicatedConstVectorLoad {
                op,
                dsts,
                offset,
                cmp,
                ca,
                cb,
                pred,
            } => {
                self.emit_inst_predicate_setup(s, cmp, ca, cb, pred);
                writeln!(s, "    mov.u64       %rd6, fuzzx_const;").unwrap();
                self.emit_vector_memory_load(
                    s,
                    op.const_load_mnemonic(),
                    op,
                    dsts,
                    "%rd6",
                    offset,
                    Some(pred),
                );
            }
            Inst::LocalVectorMem {
                op,
                dsts,
                srcs,
                offset,
            } => {
                writeln!(s, "    mov.u64       %rd6, fuzzx_local;").unwrap();
                self.emit_vector_memory_store(
                    s,
                    op.local_store_mnemonic(),
                    op,
                    srcs,
                    "%rd6",
                    offset,
                    None,
                );
                self.emit_vector_memory_load(
                    s,
                    op.local_load_mnemonic(),
                    op,
                    dsts,
                    "%rd6",
                    offset,
                    None,
                );
            }
            Inst::PredicatedLocalVectorMem {
                op,
                dsts,
                srcs,
                offset,
                cmp,
                ca,
                cb,
                pred,
            } => {
                self.emit_inst_predicate_setup(s, cmp, ca, cb, pred);
                writeln!(s, "    mov.u64       %rd6, fuzzx_local;").unwrap();
                self.emit_vector_memory_store(
                    s,
                    op.local_store_mnemonic(),
                    op,
                    srcs,
                    "%rd6",
                    offset,
                    Some(pred),
                );
                self.emit_vector_memory_load(
                    s,
                    op.local_load_mnemonic(),
                    op,
                    dsts,
                    "%rd6",
                    offset,
                    Some(pred),
                );
            }
            Inst::SharedVectorMem {
                op,
                volatile,
                dsts,
                srcs,
                offset,
            } => {
                let tid_reg = self.tid_reg();
                writeln!(s, "    mov.u64       %rd6, fuzzx_shared;").unwrap();
                writeln!(
                    s,
                    "    mul.wide.u32  %rd7, %r{tid_reg}, {SHARED_SLOT_BYTES};"
                )
                .unwrap();
                writeln!(s, "    add.s64       %rd6, %rd6, %rd7;").unwrap();
                let store_mnemonic = if volatile {
                    op.volatile_shared_store_mnemonic()
                } else {
                    op.shared_store_mnemonic().to_string()
                };
                let load_mnemonic = if volatile {
                    op.volatile_shared_load_mnemonic()
                } else {
                    op.shared_load_mnemonic().to_string()
                };
                self.emit_vector_memory_store(s, &store_mnemonic, op, srcs, "%rd6", offset, None);
                self.emit_vector_memory_load(s, &load_mnemonic, op, dsts, "%rd6", offset, None);
            }
            Inst::PredicatedSharedVectorMem {
                op,
                volatile,
                dsts,
                srcs,
                offset,
                cmp,
                ca,
                cb,
                pred,
            } => {
                self.emit_inst_predicate_setup(s, cmp, ca, cb, pred);
                let tid_reg = self.tid_reg();
                writeln!(s, "    mov.u64       %rd6, fuzzx_shared;").unwrap();
                writeln!(
                    s,
                    "    mul.wide.u32  %rd7, %r{tid_reg}, {SHARED_SLOT_BYTES};"
                )
                .unwrap();
                writeln!(s, "    add.s64       %rd6, %rd6, %rd7;").unwrap();
                let store_mnemonic = if volatile {
                    op.volatile_shared_store_mnemonic()
                } else {
                    op.shared_store_mnemonic().to_string()
                };
                let load_mnemonic = if volatile {
                    op.volatile_shared_load_mnemonic()
                } else {
                    op.shared_load_mnemonic().to_string()
                };
                self.emit_vector_memory_store(
                    s,
                    &store_mnemonic,
                    op,
                    srcs,
                    "%rd6",
                    offset,
                    Some(pred),
                );
                self.emit_vector_memory_load(
                    s,
                    &load_mnemonic,
                    op,
                    dsts,
                    "%rd6",
                    offset,
                    Some(pred),
                );
            }
            Inst::F32Arith { op, dst, a, b, c } => {
                self.emit_sanitized_f32_operand(s, 0, a);
                if op.uses_arbitrary_sign_b() {
                    let scratch = self.wide_scratch_hi_reg();
                    write!(s, "    mov.u32       %r{scratch}, ").unwrap();
                    b.emit(s);
                    writeln!(s, ";").unwrap();
                    writeln!(s, "    mov.b32       %f1, %r{scratch};").unwrap();
                } else if op.needs_positive_b() {
                    self.emit_sanitized_f32_math_operand(s, 1, b, FloatInputDomain::Positive);
                } else {
                    self.emit_sanitized_f32_operand(s, 1, b);
                }
                if op.uses_c() {
                    self.emit_sanitized_f32_operand(s, 2, c);
                    writeln!(s, "    {:<13} %f3, %f0, %f1, %f2;", op.mnemonic()).unwrap();
                } else {
                    writeln!(s, "    {:<13} %f3, %f0, %f1;", op.mnemonic()).unwrap();
                }
                writeln!(s, "    cvt.rzi.s32.f32 %r{dst}, %f3;").unwrap();
            }
            Inst::PredicatedF32Arith {
                op,
                dst,
                a,
                b,
                c,
                cmp,
                ca,
                cb,
                pred,
            } => {
                self.emit_inst_predicate_setup(s, cmp, ca, cb, pred);
                self.emit_sanitized_f32_operand(s, 0, a);
                if op.uses_arbitrary_sign_b() {
                    let scratch = self.wide_scratch_hi_reg();
                    write!(s, "    mov.u32       %r{scratch}, ").unwrap();
                    b.emit(s);
                    writeln!(s, ";").unwrap();
                    writeln!(s, "    mov.b32       %f1, %r{scratch};").unwrap();
                } else if op.needs_positive_b() {
                    self.emit_sanitized_f32_math_operand(s, 1, b, FloatInputDomain::Positive);
                } else {
                    self.emit_sanitized_f32_operand(s, 1, b);
                }
                writeln!(s, "    mov.f32       %f3, %f0;").unwrap();
                if op.uses_c() {
                    self.emit_sanitized_f32_operand(s, 2, c);
                    writeln!(
                        s,
                        "    {} {:<8} %f3, %f0, %f1, %f2;",
                        pred_guard(pred),
                        op.mnemonic()
                    )
                    .unwrap();
                } else {
                    writeln!(
                        s,
                        "    {} {:<8} %f3, %f0, %f1;",
                        pred_guard(pred),
                        op.mnemonic()
                    )
                    .unwrap();
                }
                writeln!(s, "    cvt.rzi.s32.f32 %r{dst}, %f3;").unwrap();
            }
            Inst::F32RoundingArith { op, dst, a, b, c } => {
                self.emit_sanitized_f32_operand(s, 0, a);
                if op.needs_positive_b() {
                    self.emit_sanitized_f32_math_operand(s, 1, b, FloatInputDomain::Positive);
                } else {
                    self.emit_sanitized_f32_operand(s, 1, b);
                }
                if op.uses_c() {
                    self.emit_sanitized_f32_operand(s, 2, c);
                    writeln!(s, "    {:<13} %f3, %f0, %f1, %f2;", op.mnemonic()).unwrap();
                } else {
                    writeln!(s, "    {:<13} %f3, %f0, %f1;", op.mnemonic()).unwrap();
                }
                writeln!(s, "    cvt.rzi.s32.f32 %r{dst}, %f3;").unwrap();
            }
            Inst::PredicatedF32RoundingArith {
                op,
                dst,
                a,
                b,
                c,
                cmp,
                ca,
                cb,
                pred,
            } => {
                self.emit_inst_predicate_setup(s, cmp, ca, cb, pred);
                self.emit_sanitized_f32_operand(s, 0, a);
                if op.needs_positive_b() {
                    self.emit_sanitized_f32_math_operand(s, 1, b, FloatInputDomain::Positive);
                } else {
                    self.emit_sanitized_f32_operand(s, 1, b);
                }
                writeln!(s, "    mov.f32       %f3, %f0;").unwrap();
                if op.uses_c() {
                    self.emit_sanitized_f32_operand(s, 2, c);
                    writeln!(
                        s,
                        "    {} {:<8} %f3, %f0, %f1, %f2;",
                        pred_guard(pred),
                        op.mnemonic()
                    )
                    .unwrap();
                } else {
                    writeln!(
                        s,
                        "    {} {:<8} %f3, %f0, %f1;",
                        pred_guard(pred),
                        op.mnemonic()
                    )
                    .unwrap();
                }
                writeln!(s, "    cvt.rzi.s32.f32 %r{dst}, %f3;").unwrap();
            }
            Inst::F32Unary { op, dst, src } => {
                self.emit_sanitized_f32_operand(s, 0, src);
                writeln!(s, "    {:<13} %f1, %f0;", op.mnemonic()).unwrap();
                writeln!(s, "    cvt.rzi.s32.f32 %r{dst}, %f1;").unwrap();
            }
            Inst::PredicatedF32Unary {
                op,
                dst,
                src,
                cmp,
                ca,
                cb,
                pred,
            } => {
                self.emit_inst_predicate_setup(s, cmp, ca, cb, pred);
                self.emit_sanitized_f32_operand(s, 0, src);
                writeln!(s, "    mov.f32       %f1, %f0;").unwrap();
                writeln!(s, "    {} {:<8} %f1, %f0;", pred_guard(pred), op.mnemonic()).unwrap();
                writeln!(s, "    cvt.rzi.s32.f32 %r{dst}, %f1;").unwrap();
            }
            Inst::F32Cvt {
                from_int,
                to_int,
                dst,
                src,
            } => {
                let scratch = self.wide_scratch_hi_reg();
                write!(s, "    and.b32       %r{scratch}, ").unwrap();
                src.emit(s);
                writeln!(s, ", {FLOAT_INPUT_MASK};").unwrap();
                let from_src = if from_int.source_is_64() {
                    writeln!(
                        s,
                        "    {:<13} %rd7, %r{scratch};",
                        from_int.source_extend_mnemonic()
                    )
                    .unwrap();
                    "%rd7".to_string()
                } else {
                    format!("%r{scratch}")
                };
                writeln!(s, "    {:<13} %f0, {from_src};", from_int.mnemonic()).unwrap();
                if to_int.dest_is_64() {
                    writeln!(s, "    {:<13} %rd7, %f0;", to_int.mnemonic()).unwrap();
                    writeln!(s, "    mov.b64       {{%r{dst}, %r{scratch}}}, %rd7;").unwrap();
                } else {
                    writeln!(s, "    {:<13} %r{dst}, %f0;", to_int.mnemonic()).unwrap();
                }
            }
            Inst::PredicatedF32Cvt {
                from_int,
                to_int,
                dst,
                src,
                cmp,
                ca,
                cb,
                pred,
            } => {
                let scratch = self.wide_scratch_hi_reg();
                self.emit_inst_predicate_setup(s, cmp, ca, cb, pred);
                write!(s, "    and.b32       %r{scratch}, ").unwrap();
                src.emit(s);
                writeln!(s, ", {FLOAT_INPUT_MASK};").unwrap();
                let from_src = if from_int.source_is_64() {
                    writeln!(
                        s,
                        "    {:<13} %rd7, %r{scratch};",
                        from_int.source_extend_mnemonic()
                    )
                    .unwrap();
                    "%rd7".to_string()
                } else {
                    format!("%r{scratch}")
                };
                writeln!(s, "    mov.u32       %r{dst}, 0;").unwrap();
                writeln!(s, "    cvt.rn.f32.u32 %f0, %r{dst};").unwrap();
                writeln!(
                    s,
                    "    {} {:<8} %f0, {from_src};",
                    pred_guard(pred),
                    from_int.mnemonic()
                )
                .unwrap();
                if to_int.dest_is_64() {
                    writeln!(s, "    mov.u64       %rd7, 0;").unwrap();
                    writeln!(
                        s,
                        "    {} {:<8} %rd7, %f0;",
                        pred_guard(pred),
                        to_int.mnemonic()
                    )
                    .unwrap();
                    writeln!(s, "    mov.b64       {{%r{dst}, %r{scratch}}}, %rd7;").unwrap();
                } else {
                    writeln!(
                        s,
                        "    {} {:<8} %r{dst}, %f0;",
                        pred_guard(pred),
                        to_int.mnemonic()
                    )
                    .unwrap();
                }
            }
            Inst::F32FloatCvt { op, dst, src } => {
                self.emit_sanitized_f64_operand(s, 0, src);
                writeln!(s, "    {:<13} %f0, %fd0;", op.mnemonic()).unwrap();
                writeln!(s, "    cvt.rzi.s32.f32 %r{dst}, %f0;").unwrap();
            }
            Inst::PredicatedF32FloatCvt {
                op,
                dst,
                src,
                cmp,
                ca,
                cb,
                pred,
            } => {
                self.emit_inst_predicate_setup(s, cmp, ca, cb, pred);
                self.emit_sanitized_f64_operand(s, 0, src);
                writeln!(s, "    mov.u32       %r{dst}, 0;").unwrap();
                writeln!(s, "    cvt.rn.f32.u32 %f0, %r{dst};").unwrap();
                writeln!(
                    s,
                    "    {} {:<8} %f0, %fd0;",
                    pred_guard(pred),
                    op.mnemonic()
                )
                .unwrap();
                writeln!(s, "    {} cvt.rzi.s32.f32 %r{dst}, %f0;", pred_guard(pred)).unwrap();
            }
            Inst::F32SpecialMath { op, dst, src } => {
                self.emit_sanitized_f32_math_operand(s, 0, src, op.input_domain());
                writeln!(s, "    {:<13} %f1, %f0;", op.mnemonic()).unwrap();
                writeln!(s, "    cvt.rzi.s32.f32 %r{dst}, %f1;").unwrap();
            }
            Inst::PredicatedF32SpecialMath {
                op,
                dst,
                src,
                cmp,
                ca,
                cb,
                pred,
            } => {
                self.emit_inst_predicate_setup(s, cmp, ca, cb, pred);
                self.emit_sanitized_f32_math_operand(s, 0, src, op.input_domain());
                writeln!(s, "    mov.f32       %f1, %f0;").unwrap();
                writeln!(s, "    {} {:<8} %f1, %f0;", pred_guard(pred), op.mnemonic()).unwrap();
                writeln!(s, "    cvt.rzi.s32.f32 %r{dst}, %f1;").unwrap();
            }
            Inst::F32Set { cmp, dst, a, b } => {
                self.emit_sanitized_f32_operand(s, 0, a);
                self.emit_sanitized_f32_operand(s, 1, b);
                writeln!(s, "    {:<13} %r{dst}, %f0, %f1;", cmp.set_mnemonic()).unwrap();
            }
            Inst::PredicatedF32Set {
                cmp,
                dst,
                a,
                b,
                guard_cmp,
                guard_ca,
                guard_cb,
                guard_pred,
            } => {
                self.emit_inst_predicate_setup(s, guard_cmp, guard_ca, guard_cb, guard_pred);
                self.emit_sanitized_f32_operand(s, 0, a);
                self.emit_sanitized_f32_operand(s, 1, b);
                writeln!(s, "    mov.u32       %r{dst}, 0;").unwrap();
                writeln!(
                    s,
                    "    {} {:<8} %r{dst}, %f0, %f1;",
                    pred_guard(guard_pred),
                    cmp.set_mnemonic()
                )
                .unwrap();
            }
            Inst::F32SetpBool {
                bool_op,
                base_cmp,
                base_a,
                base_b,
                cmp,
                dst,
                a,
                b,
                base_pred,
                pred,
            } => {
                write!(s, "    {:<13} %p{base_pred}, ", base_cmp.mnemonic()).unwrap();
                base_a.emit(s);
                write!(s, ", ").unwrap();
                base_b.emit(s);
                writeln!(s, ";").unwrap();
                self.emit_sanitized_f32_operand(s, 0, a);
                self.emit_sanitized_f32_operand(s, 1, b);
                let mnemonic = cmp.setp_bool_mnemonic(bool_op);
                writeln!(s, "    {mnemonic:<13} %p{pred}, %f0, %f1, %p{base_pred};").unwrap();
                writeln!(s, "    selp.u32      %r{dst}, 1, 0, %p{pred};").unwrap();
            }
            Inst::PredicatedF32SetpBool {
                bool_op,
                base_cmp,
                base_a,
                base_b,
                cmp,
                dst,
                a,
                b,
                base_pred,
                pred,
                guard_cmp,
                guard_ca,
                guard_cb,
                guard_pred,
            } => {
                self.emit_inst_predicate_setup(s, guard_cmp, guard_ca, guard_cb, guard_pred);
                write!(s, "    {:<13} %p{base_pred}, ", base_cmp.mnemonic()).unwrap();
                base_a.emit(s);
                write!(s, ", ").unwrap();
                base_b.emit(s);
                writeln!(s, ";").unwrap();
                self.emit_sanitized_f32_operand(s, 0, a);
                self.emit_sanitized_f32_operand(s, 1, b);
                writeln!(s, "    setp.ne.u32   %p{pred}, 0, 0;").unwrap();
                let mnemonic = cmp.setp_bool_mnemonic(bool_op);
                writeln!(
                    s,
                    "    {} {mnemonic:<8} %p{pred}, %f0, %f1, %p{base_pred};",
                    pred_guard(guard_pred)
                )
                .unwrap();
                writeln!(s, "    selp.u32      %r{dst}, 1, 0, %p{pred};").unwrap();
            }
            Inst::F32Testp { op, dst, src, pred } => {
                write!(s, "    mov.u32       %r{dst}, ").unwrap();
                src.emit(s);
                writeln!(s, ";").unwrap();
                writeln!(s, "    mov.b32       %f0, %r{dst};").unwrap();
                let mnemonic = op.f32_mnemonic();
                writeln!(s, "    {mnemonic:<13} %p{pred}, %f0;").unwrap();
                writeln!(s, "    selp.u32      %r{dst}, 1, 0, %p{pred};").unwrap();
            }
            Inst::PredicatedF32Testp {
                op,
                dst,
                src,
                pred,
                guard_cmp,
                guard_ca,
                guard_cb,
                guard_pred,
            } => {
                self.emit_inst_predicate_setup(s, guard_cmp, guard_ca, guard_cb, guard_pred);
                write!(s, "    mov.u32       %r{dst}, ").unwrap();
                src.emit(s);
                writeln!(s, ";").unwrap();
                writeln!(s, "    mov.b32       %f0, %r{dst};").unwrap();
                writeln!(s, "    setp.ne.u32   %p{pred}, 0, 0;").unwrap();
                let mnemonic = op.f32_mnemonic();
                writeln!(
                    s,
                    "    {} {mnemonic:<8} %p{pred}, %f0;",
                    pred_guard(guard_pred)
                )
                .unwrap();
                writeln!(s, "    selp.u32      %r{dst}, 1, 0, %p{pred};").unwrap();
            }
            Inst::F32Selp {
                cmp,
                dst,
                a,
                b,
                pred,
            } => {
                self.emit_sanitized_f32_operand(s, 0, a);
                self.emit_sanitized_f32_operand(s, 1, b);
                writeln!(s, "    {:<13} %p{pred}, %f0, %f1;", cmp.setp_mnemonic()).unwrap();
                writeln!(s, "    selp.f32      %f2, %f0, %f1, %p{pred};").unwrap();
                writeln!(s, "    cvt.rzi.s32.f32 %r{dst}, %f2;").unwrap();
            }
            Inst::PredicatedF32Selp {
                cmp,
                dst,
                a,
                b,
                pred,
                guard_cmp,
                guard_ca,
                guard_cb,
                guard_pred,
            } => {
                self.emit_inst_predicate_setup(s, guard_cmp, guard_ca, guard_cb, guard_pred);
                self.emit_sanitized_f32_operand(s, 0, a);
                self.emit_sanitized_f32_operand(s, 1, b);
                writeln!(s, "    {:<13} %p{pred}, %f0, %f1;", cmp.setp_mnemonic()).unwrap();
                writeln!(s, "    mov.f32       %f2, %f0;").unwrap();
                writeln!(
                    s,
                    "    {} selp.f32 %f2, %f0, %f1, %p{pred};",
                    pred_guard(guard_pred)
                )
                .unwrap();
                writeln!(s, "    cvt.rzi.s32.f32 %r{dst}, %f2;").unwrap();
            }
            Inst::F64Arith { op, dst, a, b, c } => {
                self.emit_sanitized_f64_operand(s, 0, a);
                if op.uses_arbitrary_sign_b() {
                    let scratch = self.wide_scratch_hi_reg();
                    write!(s, "    mov.u32       %r{scratch}, ").unwrap();
                    b.emit(s);
                    writeln!(s, ";").unwrap();
                    writeln!(s, "    mov.u32       %r{dst}, 0;").unwrap();
                    writeln!(s, "    mov.b64       %fd1, {{%r{dst}, %r{scratch}}};").unwrap();
                } else if op.needs_positive_b() {
                    self.emit_sanitized_f64_math_operand(s, 1, b, FloatInputDomain::Positive);
                } else {
                    self.emit_sanitized_f64_operand(s, 1, b);
                }
                if op.uses_c() {
                    self.emit_sanitized_f64_operand(s, 2, c);
                    writeln!(s, "    {:<13} %fd3, %fd0, %fd1, %fd2;", op.mnemonic()).unwrap();
                } else {
                    writeln!(s, "    {:<13} %fd3, %fd0, %fd1;", op.mnemonic()).unwrap();
                }
                writeln!(s, "    cvt.rzi.s32.f64 %r{dst}, %fd3;").unwrap();
            }
            Inst::PredicatedF64Arith {
                op,
                dst,
                a,
                b,
                c,
                cmp,
                ca,
                cb,
                pred,
            } => {
                self.emit_inst_predicate_setup(s, cmp, ca, cb, pred);
                self.emit_sanitized_f64_operand(s, 0, a);
                if op.uses_arbitrary_sign_b() {
                    let scratch = self.wide_scratch_hi_reg();
                    write!(s, "    mov.u32       %r{scratch}, ").unwrap();
                    b.emit(s);
                    writeln!(s, ";").unwrap();
                    writeln!(s, "    mov.u32       %r{dst}, 0;").unwrap();
                    writeln!(s, "    mov.b64       %fd1, {{%r{dst}, %r{scratch}}};").unwrap();
                } else if op.needs_positive_b() {
                    self.emit_sanitized_f64_math_operand(s, 1, b, FloatInputDomain::Positive);
                } else {
                    self.emit_sanitized_f64_operand(s, 1, b);
                }
                writeln!(s, "    mov.f64       %fd3, %fd0;").unwrap();
                if op.uses_c() {
                    self.emit_sanitized_f64_operand(s, 2, c);
                    writeln!(
                        s,
                        "    {} {:<8} %fd3, %fd0, %fd1, %fd2;",
                        pred_guard(pred),
                        op.mnemonic()
                    )
                    .unwrap();
                } else {
                    writeln!(
                        s,
                        "    {} {:<8} %fd3, %fd0, %fd1;",
                        pred_guard(pred),
                        op.mnemonic()
                    )
                    .unwrap();
                }
                writeln!(s, "    cvt.rzi.s32.f64 %r{dst}, %fd3;").unwrap();
            }
            Inst::F64RoundingArith { op, dst, a, b, c } => {
                self.emit_sanitized_f64_operand(s, 0, a);
                if op.needs_positive_b() {
                    self.emit_sanitized_f64_math_operand(s, 1, b, FloatInputDomain::Positive);
                } else {
                    self.emit_sanitized_f64_operand(s, 1, b);
                }
                if op.uses_c() {
                    self.emit_sanitized_f64_operand(s, 2, c);
                    writeln!(s, "    {:<13} %fd3, %fd0, %fd1, %fd2;", op.mnemonic()).unwrap();
                } else {
                    writeln!(s, "    {:<13} %fd3, %fd0, %fd1;", op.mnemonic()).unwrap();
                }
                writeln!(s, "    cvt.rzi.s32.f64 %r{dst}, %fd3;").unwrap();
            }
            Inst::PredicatedF64RoundingArith {
                op,
                dst,
                a,
                b,
                c,
                cmp,
                ca,
                cb,
                pred,
            } => {
                self.emit_inst_predicate_setup(s, cmp, ca, cb, pred);
                self.emit_sanitized_f64_operand(s, 0, a);
                if op.needs_positive_b() {
                    self.emit_sanitized_f64_math_operand(s, 1, b, FloatInputDomain::Positive);
                } else {
                    self.emit_sanitized_f64_operand(s, 1, b);
                }
                writeln!(s, "    mov.f64       %fd3, %fd0;").unwrap();
                if op.uses_c() {
                    self.emit_sanitized_f64_operand(s, 2, c);
                    writeln!(
                        s,
                        "    {} {:<8} %fd3, %fd0, %fd1, %fd2;",
                        pred_guard(pred),
                        op.mnemonic()
                    )
                    .unwrap();
                } else {
                    writeln!(
                        s,
                        "    {} {:<8} %fd3, %fd0, %fd1;",
                        pred_guard(pred),
                        op.mnemonic()
                    )
                    .unwrap();
                }
                writeln!(s, "    cvt.rzi.s32.f64 %r{dst}, %fd3;").unwrap();
            }
            Inst::F64Unary { op, dst, src } => {
                self.emit_sanitized_f64_operand(s, 0, src);
                writeln!(s, "    {:<13} %fd1, %fd0;", op.mnemonic()).unwrap();
                writeln!(s, "    cvt.rzi.s32.f64 %r{dst}, %fd1;").unwrap();
            }
            Inst::PredicatedF64Unary {
                op,
                dst,
                src,
                cmp,
                ca,
                cb,
                pred,
            } => {
                self.emit_inst_predicate_setup(s, cmp, ca, cb, pred);
                self.emit_sanitized_f64_operand(s, 0, src);
                writeln!(s, "    mov.f64       %fd1, %fd0;").unwrap();
                writeln!(
                    s,
                    "    {} {:<8} %fd1, %fd0;",
                    pred_guard(pred),
                    op.mnemonic()
                )
                .unwrap();
                writeln!(s, "    cvt.rzi.s32.f64 %r{dst}, %fd1;").unwrap();
            }
            Inst::F64Cvt {
                from_int,
                to_int,
                dst,
                src,
            } => {
                let scratch = self.wide_scratch_hi_reg();
                write!(s, "    and.b32       %r{scratch}, ").unwrap();
                src.emit(s);
                writeln!(s, ", {FLOAT_INPUT_MASK};").unwrap();
                let from_src = if from_int.source_is_64() {
                    writeln!(
                        s,
                        "    {:<13} %rd7, %r{scratch};",
                        from_int.source_extend_mnemonic()
                    )
                    .unwrap();
                    "%rd7".to_string()
                } else {
                    format!("%r{scratch}")
                };
                writeln!(s, "    {:<13} %fd0, {from_src};", from_int.mnemonic()).unwrap();
                if to_int.dest_is_64() {
                    writeln!(s, "    {:<13} %rd7, %fd0;", to_int.mnemonic()).unwrap();
                    writeln!(s, "    mov.b64       {{%r{dst}, %r{scratch}}}, %rd7;").unwrap();
                } else {
                    writeln!(s, "    {:<13} %r{dst}, %fd0;", to_int.mnemonic()).unwrap();
                }
            }
            Inst::PredicatedF64Cvt {
                from_int,
                to_int,
                dst,
                src,
                cmp,
                ca,
                cb,
                pred,
            } => {
                let scratch = self.wide_scratch_hi_reg();
                self.emit_inst_predicate_setup(s, cmp, ca, cb, pred);
                write!(s, "    and.b32       %r{scratch}, ").unwrap();
                src.emit(s);
                writeln!(s, ", {FLOAT_INPUT_MASK};").unwrap();
                let from_src = if from_int.source_is_64() {
                    writeln!(
                        s,
                        "    {:<13} %rd7, %r{scratch};",
                        from_int.source_extend_mnemonic()
                    )
                    .unwrap();
                    "%rd7".to_string()
                } else {
                    format!("%r{scratch}")
                };
                writeln!(s, "    mov.u32       %r{dst}, 0;").unwrap();
                writeln!(s, "    cvt.rn.f64.u32 %fd0, %r{dst};").unwrap();
                writeln!(
                    s,
                    "    {} {:<8} %fd0, {from_src};",
                    pred_guard(pred),
                    from_int.mnemonic()
                )
                .unwrap();
                if to_int.dest_is_64() {
                    writeln!(s, "    mov.u64       %rd7, 0;").unwrap();
                    writeln!(
                        s,
                        "    {} {:<8} %rd7, %fd0;",
                        pred_guard(pred),
                        to_int.mnemonic()
                    )
                    .unwrap();
                    writeln!(s, "    mov.b64       {{%r{dst}, %r{scratch}}}, %rd7;").unwrap();
                } else {
                    writeln!(
                        s,
                        "    {} {:<8} %r{dst}, %fd0;",
                        pred_guard(pred),
                        to_int.mnemonic()
                    )
                    .unwrap();
                }
            }
            Inst::F64FloatCvt { op, dst, src } => {
                self.emit_sanitized_f32_operand(s, 0, src);
                writeln!(s, "    {:<13} %fd0, %f0;", op.mnemonic()).unwrap();
                writeln!(s, "    cvt.rzi.s32.f64 %r{dst}, %fd0;").unwrap();
            }
            Inst::PredicatedF64FloatCvt {
                op,
                dst,
                src,
                cmp,
                ca,
                cb,
                pred,
            } => {
                self.emit_inst_predicate_setup(s, cmp, ca, cb, pred);
                self.emit_sanitized_f32_operand(s, 0, src);
                writeln!(s, "    mov.u32       %r{dst}, 0;").unwrap();
                writeln!(s, "    cvt.rn.f64.u32 %fd0, %r{dst};").unwrap();
                writeln!(
                    s,
                    "    {} {:<8} %fd0, %f0;",
                    pred_guard(pred),
                    op.mnemonic()
                )
                .unwrap();
                writeln!(s, "    {} cvt.rzi.s32.f64 %r{dst}, %fd0;", pred_guard(pred)).unwrap();
            }
            Inst::F64SpecialMath { op, dst, src } => {
                self.emit_sanitized_f64_math_operand(s, 0, src, op.input_domain());
                writeln!(s, "    {:<13} %fd1, %fd0;", op.mnemonic()).unwrap();
                writeln!(s, "    cvt.rzi.s32.f64 %r{dst}, %fd1;").unwrap();
            }
            Inst::PredicatedF64SpecialMath {
                op,
                dst,
                src,
                cmp,
                ca,
                cb,
                pred,
            } => {
                self.emit_inst_predicate_setup(s, cmp, ca, cb, pred);
                self.emit_sanitized_f64_math_operand(s, 0, src, op.input_domain());
                writeln!(s, "    mov.f64       %fd1, %fd0;").unwrap();
                writeln!(
                    s,
                    "    {} {:<8} %fd1, %fd0;",
                    pred_guard(pred),
                    op.mnemonic()
                )
                .unwrap();
                writeln!(s, "    cvt.rzi.s32.f64 %r{dst}, %fd1;").unwrap();
            }
            Inst::F64Set { cmp, dst, a, b } => {
                self.emit_sanitized_f64_operand(s, 0, a);
                self.emit_sanitized_f64_operand(s, 1, b);
                writeln!(s, "    {:<13} %r{dst}, %fd0, %fd1;", cmp.f64_set_mnemonic()).unwrap();
            }
            Inst::PredicatedF64Set {
                cmp,
                dst,
                a,
                b,
                guard_cmp,
                guard_ca,
                guard_cb,
                guard_pred,
            } => {
                self.emit_inst_predicate_setup(s, guard_cmp, guard_ca, guard_cb, guard_pred);
                self.emit_sanitized_f64_operand(s, 0, a);
                self.emit_sanitized_f64_operand(s, 1, b);
                writeln!(s, "    mov.u32       %r{dst}, 0;").unwrap();
                writeln!(
                    s,
                    "    {} {:<8} %r{dst}, %fd0, %fd1;",
                    pred_guard(guard_pred),
                    cmp.f64_set_mnemonic()
                )
                .unwrap();
            }
            Inst::F64SetpBool {
                bool_op,
                base_cmp,
                base_a,
                base_b,
                cmp,
                dst,
                a,
                b,
                base_pred,
                pred,
            } => {
                write!(s, "    {:<13} %p{base_pred}, ", base_cmp.mnemonic()).unwrap();
                base_a.emit(s);
                write!(s, ", ").unwrap();
                base_b.emit(s);
                writeln!(s, ";").unwrap();
                self.emit_sanitized_f64_operand(s, 0, a);
                self.emit_sanitized_f64_operand(s, 1, b);
                let mnemonic = cmp.f64_setp_bool_mnemonic(bool_op);
                writeln!(s, "    {mnemonic:<13} %p{pred}, %fd0, %fd1, %p{base_pred};").unwrap();
                writeln!(s, "    selp.u32      %r{dst}, 1, 0, %p{pred};").unwrap();
            }
            Inst::PredicatedF64SetpBool {
                bool_op,
                base_cmp,
                base_a,
                base_b,
                cmp,
                dst,
                a,
                b,
                base_pred,
                pred,
                guard_cmp,
                guard_ca,
                guard_cb,
                guard_pred,
            } => {
                self.emit_inst_predicate_setup(s, guard_cmp, guard_ca, guard_cb, guard_pred);
                write!(s, "    {:<13} %p{base_pred}, ", base_cmp.mnemonic()).unwrap();
                base_a.emit(s);
                write!(s, ", ").unwrap();
                base_b.emit(s);
                writeln!(s, ";").unwrap();
                self.emit_sanitized_f64_operand(s, 0, a);
                self.emit_sanitized_f64_operand(s, 1, b);
                writeln!(s, "    setp.ne.u32   %p{pred}, 0, 0;").unwrap();
                let mnemonic = cmp.f64_setp_bool_mnemonic(bool_op);
                writeln!(
                    s,
                    "    {} {mnemonic:<8} %p{pred}, %fd0, %fd1, %p{base_pred};",
                    pred_guard(guard_pred)
                )
                .unwrap();
                writeln!(s, "    selp.u32      %r{dst}, 1, 0, %p{pred};").unwrap();
            }
            Inst::F64Testp {
                op,
                dst,
                src_lo,
                src_hi,
                pred,
            } => {
                let scratch = self.wide_scratch_hi_reg();
                write!(s, "    mov.u32       %r{scratch}, ").unwrap();
                src_hi.emit(s);
                writeln!(s, ";").unwrap();
                write!(s, "    mov.u32       %r{dst}, ").unwrap();
                src_lo.emit(s);
                writeln!(s, ";").unwrap();
                writeln!(s, "    mov.b64       %fd0, {{%r{dst}, %r{scratch}}};").unwrap();
                let mnemonic = op.f64_mnemonic();
                writeln!(s, "    {mnemonic:<13} %p{pred}, %fd0;").unwrap();
                writeln!(s, "    selp.u32      %r{dst}, 1, 0, %p{pred};").unwrap();
            }
            Inst::PredicatedF64Testp {
                op,
                dst,
                src_lo,
                src_hi,
                pred,
                guard_cmp,
                guard_ca,
                guard_cb,
                guard_pred,
            } => {
                let scratch = self.wide_scratch_hi_reg();
                self.emit_inst_predicate_setup(s, guard_cmp, guard_ca, guard_cb, guard_pred);
                write!(s, "    mov.u32       %r{scratch}, ").unwrap();
                src_hi.emit(s);
                writeln!(s, ";").unwrap();
                write!(s, "    mov.u32       %r{dst}, ").unwrap();
                src_lo.emit(s);
                writeln!(s, ";").unwrap();
                writeln!(s, "    mov.b64       %fd0, {{%r{dst}, %r{scratch}}};").unwrap();
                writeln!(s, "    setp.ne.u32   %p{pred}, 0, 0;").unwrap();
                let mnemonic = op.f64_mnemonic();
                writeln!(
                    s,
                    "    {} {mnemonic:<8} %p{pred}, %fd0;",
                    pred_guard(guard_pred)
                )
                .unwrap();
                writeln!(s, "    selp.u32      %r{dst}, 1, 0, %p{pred};").unwrap();
            }
            Inst::F64Selp {
                cmp,
                dst,
                a,
                b,
                pred,
            } => {
                self.emit_sanitized_f64_operand(s, 0, a);
                self.emit_sanitized_f64_operand(s, 1, b);
                writeln!(
                    s,
                    "    {:<13} %p{pred}, %fd0, %fd1;",
                    cmp.f64_setp_mnemonic()
                )
                .unwrap();
                writeln!(s, "    selp.f64      %fd2, %fd0, %fd1, %p{pred};").unwrap();
                writeln!(s, "    cvt.rzi.s32.f64 %r{dst}, %fd2;").unwrap();
            }
            Inst::PredicatedF64Selp {
                cmp,
                dst,
                a,
                b,
                pred,
                guard_cmp,
                guard_ca,
                guard_cb,
                guard_pred,
            } => {
                self.emit_inst_predicate_setup(s, guard_cmp, guard_ca, guard_cb, guard_pred);
                self.emit_sanitized_f64_operand(s, 0, a);
                self.emit_sanitized_f64_operand(s, 1, b);
                writeln!(
                    s,
                    "    {:<13} %p{pred}, %fd0, %fd1;",
                    cmp.f64_setp_mnemonic()
                )
                .unwrap();
                writeln!(s, "    mov.f64       %fd2, %fd0;").unwrap();
                writeln!(
                    s,
                    "    {} selp.f64 %fd2, %fd0, %fd1, %p{pred};",
                    pred_guard(guard_pred)
                )
                .unwrap();
                writeln!(s, "    cvt.rzi.s32.f64 %r{dst}, %fd2;").unwrap();
            }
            Inst::Sel {
                op,
                dst,
                a,
                b,
                cmp,
                ca,
                cb,
                pred,
            } => {
                write!(s, "    {:<13} %p{pred}, ", cmp.mnemonic()).unwrap();
                ca.emit(s);
                write!(s, ", ").unwrap();
                cb.emit(s);
                writeln!(s, ";").unwrap();
                write!(s, "    {:<13} %r{dst}, ", op.mnemonic()).unwrap();
                a.emit(s);
                write!(s, ", ").unwrap();
                b.emit(s);
                writeln!(s, ", %p{pred};").unwrap();
            }
            Inst::PredicatedSel {
                op,
                dst,
                a,
                b,
                cmp,
                ca,
                cb,
                pred,
                guard_cmp,
                guard_ca,
                guard_cb,
                guard_pred,
            } => {
                write!(s, "    {:<13} %p{}, ", cmp.mnemonic(), pred_id(pred)).unwrap();
                ca.emit(s);
                write!(s, ", ").unwrap();
                cb.emit(s);
                writeln!(s, ";").unwrap();
                self.emit_inst_predicate_setup(s, guard_cmp, guard_ca, guard_cb, guard_pred);
                write!(
                    s,
                    "    {} {:<8} %r{dst}, ",
                    pred_guard(guard_pred),
                    op.mnemonic()
                )
                .unwrap();
                a.emit(s);
                write!(s, ", ").unwrap();
                b.emit(s);
                writeln!(s, ", %p{};", pred_id(pred)).unwrap();
            }
            Inst::PredicatedBin {
                op,
                dst,
                a,
                b,
                cmp,
                ca,
                cb,
                pred,
            } => {
                self.emit_inst_predicate_setup(s, cmp, ca, cb, pred);
                write!(s, "    {} {:<8} %r{dst}, ", pred_guard(pred), op.mnemonic()).unwrap();
                a.emit(s);
                write!(s, ", ").unwrap();
                b.emit(s);
                writeln!(s, ";").unwrap();
            }
            Inst::PredicatedPackedAdd {
                op,
                dst,
                a,
                b,
                cmp,
                ca,
                cb,
                pred,
            } => {
                self.emit_inst_predicate_setup(s, cmp, ca, cb, pred);
                write!(s, "    {} {:<8} %r{dst}, ", pred_guard(pred), op.mnemonic()).unwrap();
                a.emit(s);
                write!(s, ", ").unwrap();
                b.emit(s);
                writeln!(s, ";").unwrap();
            }
            Inst::PredicatedPackedMinMax {
                op,
                dst,
                a,
                b,
                cmp,
                ca,
                cb,
                pred,
            } => {
                self.emit_inst_predicate_setup(s, cmp, ca, cb, pred);
                write!(s, "    {} {:<8} %r{dst}, ", pred_guard(pred), op.mnemonic()).unwrap();
                a.emit(s);
                write!(s, ", ").unwrap();
                b.emit(s);
                writeln!(s, ";").unwrap();
            }
            Inst::PredicatedScalar16 {
                op,
                dst,
                a,
                b,
                cmp,
                ca,
                cb,
                pred,
            } => {
                self.emit_inst_predicate_setup(s, cmp, ca, cb, pred);
                write!(s, "    {:<13} %h0, ", op.input_cvt_mnemonic()).unwrap();
                a.emit(s);
                writeln!(s, ";").unwrap();
                if op.uses_h1() {
                    write!(s, "    {:<13} %h1, ", op.input_cvt_mnemonic()).unwrap();
                    b.emit(s);
                    writeln!(s, ";").unwrap();
                }
                if op.is_unary() {
                    writeln!(s, "    {} {:<8} %h2, %h0;", pred_guard(pred), op.mnemonic()).unwrap();
                } else if op.is_shift() {
                    write!(
                        s,
                        "    {} {:<8} %h2, %h0, ",
                        pred_guard(pred),
                        op.mnemonic()
                    )
                    .unwrap();
                    b.emit(s);
                    writeln!(s, ";").unwrap();
                } else {
                    writeln!(
                        s,
                        "    {} {:<8} %h2, %h0, %h1;",
                        pred_guard(pred),
                        op.mnemonic()
                    )
                    .unwrap();
                }
                writeln!(
                    s,
                    "    {} {:<8} %r{dst}, %h2;",
                    pred_guard(pred),
                    op.output_cvt_mnemonic()
                )
                .unwrap();
            }
            Inst::SetpBoolBin {
                bool_op,
                base_cmp,
                base_a,
                base_b,
                cmp,
                cmp_a,
                cmp_b,
                base_pred,
                guard_pred,
                op,
                dst,
                a,
                b,
            } => {
                write!(s, "    {:<13} %p{base_pred}, ", base_cmp.mnemonic()).unwrap();
                base_a.emit(s);
                write!(s, ", ").unwrap();
                base_b.emit(s);
                writeln!(s, ";").unwrap();
                let mnemonic = cmp.setp_bool_mnemonic(bool_op);
                write!(s, "    {mnemonic:<13} %p{}, ", pred_id(guard_pred)).unwrap();
                cmp_a.emit(s);
                write!(s, ", ").unwrap();
                cmp_b.emit(s);
                writeln!(s, ", %p{base_pred};").unwrap();
                write!(
                    s,
                    "    {} {:<8} %r{dst}, ",
                    pred_guard(guard_pred),
                    op.mnemonic()
                )
                .unwrap();
                a.emit(s);
                write!(s, ", ").unwrap();
                b.emit(s);
                writeln!(s, ";").unwrap();
            }
            Inst::SetpDualBin {
                cmp,
                cmp_a,
                cmp_b,
                true_pred,
                false_pred,
                dst,
                true_op,
                true_a,
                true_b,
                false_op,
                false_a,
                false_b,
            } => {
                write!(
                    s,
                    "    {:<13} %p{true_pred}|%p{false_pred}, ",
                    cmp.mnemonic()
                )
                .unwrap();
                cmp_a.emit(s);
                write!(s, ", ").unwrap();
                cmp_b.emit(s);
                writeln!(s, ";").unwrap();
                write!(s, "    @%p{true_pred} {:<8} %r{dst}, ", true_op.mnemonic()).unwrap();
                true_a.emit(s);
                write!(s, ", ").unwrap();
                true_b.emit(s);
                writeln!(s, ";").unwrap();
                write!(
                    s,
                    "    @%p{false_pred} {:<8} %r{dst}, ",
                    false_op.mnemonic()
                )
                .unwrap();
                false_a.emit(s);
                write!(s, ", ").unwrap();
                false_b.emit(s);
                writeln!(s, ";").unwrap();
            }
            Inst::PredLogicBin {
                logic_op,
                lhs_cmp,
                lhs_a,
                lhs_b,
                rhs_cmp,
                rhs_a,
                rhs_b,
                lhs_pred,
                rhs_pred,
                guard_pred,
                op,
                dst,
                a,
                b,
            } => {
                write!(s, "    {:<13} %p{lhs_pred}, ", lhs_cmp.mnemonic()).unwrap();
                lhs_a.emit(s);
                write!(s, ", ").unwrap();
                lhs_b.emit(s);
                writeln!(s, ";").unwrap();
                if !matches!(logic_op, PredicateLogicOp::Not) {
                    write!(s, "    {:<13} %p{rhs_pred}, ", rhs_cmp.mnemonic()).unwrap();
                    rhs_a.emit(s);
                    write!(s, ", ").unwrap();
                    rhs_b.emit(s);
                    writeln!(s, ";").unwrap();
                }
                write!(
                    s,
                    "    {:<13} %p{}, %p{lhs_pred}",
                    logic_op.mnemonic(),
                    pred_id(guard_pred)
                )
                .unwrap();
                if !matches!(logic_op, PredicateLogicOp::Not) {
                    write!(s, ", %p{rhs_pred}").unwrap();
                }
                writeln!(s, ";").unwrap();
                write!(
                    s,
                    "    {} {:<8} %r{dst}, ",
                    pred_guard(guard_pred),
                    op.mnemonic()
                )
                .unwrap();
                a.emit(s);
                write!(s, ", ").unwrap();
                b.emit(s);
                writeln!(s, ";").unwrap();
            }
            Inst::PredicatedShift {
                op,
                dst,
                src,
                amount,
                cmp,
                ca,
                cb,
                pred,
            } => {
                self.emit_inst_predicate_setup(s, cmp, ca, cb, pred);
                write!(s, "    {} {:<8} %r{dst}, ", pred_guard(pred), op.mnemonic()).unwrap();
                src.emit(s);
                writeln!(s, ", {amount};").unwrap();
            }
            Inst::Set { dst, cmp, a, b } => {
                write!(s, "    {:<17} %r{dst}, ", cmp.set_mnemonic()).unwrap();
                a.emit(s);
                write!(s, ", ").unwrap();
                b.emit(s);
                writeln!(s, ";").unwrap();
            }
            Inst::PredicatedSet {
                dst,
                cmp,
                a,
                b,
                guard_cmp,
                guard_ca,
                guard_cb,
                guard_pred,
            } => {
                self.emit_inst_predicate_setup(s, guard_cmp, guard_ca, guard_cb, guard_pred);
                write!(
                    s,
                    "    {} {:<12} %r{dst}, ",
                    pred_guard(guard_pred),
                    cmp.set_mnemonic()
                )
                .unwrap();
                a.emit(s);
                write!(s, ", ").unwrap();
                b.emit(s);
                writeln!(s, ";").unwrap();
            }
            Inst::Shift {
                op,
                dst,
                src,
                amount,
            } => {
                write!(s, "    {:<13} %r{dst}, ", op.mnemonic()).unwrap();
                src.emit(s);
                writeln!(s, ", {amount};").unwrap();
            }
            Inst::RegShift {
                op,
                dst,
                src,
                amount,
            } => {
                let scratch = self.wide_scratch_hi_reg();
                write!(s, "    and.b32       %r{scratch}, ").unwrap();
                amount.emit(s);
                writeln!(s, ", 31;").unwrap();
                write!(s, "    {:<13} %r{dst}, ", op.mnemonic()).unwrap();
                src.emit(s);
                writeln!(s, ", %r{scratch};").unwrap();
            }
            Inst::PredicatedRegShift {
                op,
                dst,
                src,
                amount,
                cmp,
                ca,
                cb,
                pred,
            } => {
                let scratch = self.wide_scratch_hi_reg();
                self.emit_inst_predicate_setup(s, cmp, ca, cb, pred);
                write!(s, "    and.b32       %r{scratch}, ").unwrap();
                amount.emit(s);
                writeln!(s, ", 31;").unwrap();
                write!(s, "    {} {:<8} %r{dst}, ", pred_guard(pred), op.mnemonic()).unwrap();
                src.emit(s);
                writeln!(s, ", %r{scratch};").unwrap();
            }
            Inst::Unary { op, dst, src } => {
                write!(s, "    {:<13} %r{dst}, ", op.mnemonic()).unwrap();
                src.emit(s);
                writeln!(s, ";").unwrap();
            }
            Inst::PredicatedUnary {
                op,
                dst,
                src,
                cmp,
                ca,
                cb,
                pred,
            } => {
                self.emit_inst_predicate_setup(s, cmp, ca, cb, pred);
                write!(s, "    {} {:<8} %r{dst}, ", pred_guard(pred), op.mnemonic()).unwrap();
                src.emit(s);
                writeln!(s, ";").unwrap();
            }
            Inst::SpecialReg { op, dst } => {
                writeln!(s, "    mov.u32       %r{dst}, {};", op.reg_name()).unwrap();
            }
            Inst::PredicatedSpecialReg {
                op,
                dst,
                cmp,
                ca,
                cb,
                pred,
            } => {
                self.emit_inst_predicate_setup(s, cmp, ca, cb, pred);
                writeln!(
                    s,
                    "    {} mov.u32 %r{dst}, {};",
                    pred_guard(pred),
                    op.reg_name()
                )
                .unwrap();
            }
            Inst::Cvt { op, dst, src } => {
                write!(s, "    {:<13} %r{dst}, ", op.mnemonic()).unwrap();
                src.emit(s);
                writeln!(s, ";").unwrap();
            }
            Inst::PredicatedCvt {
                op,
                dst,
                src,
                cmp,
                ca,
                cb,
                pred,
            } => {
                self.emit_inst_predicate_setup(s, cmp, ca, cb, pred);
                write!(s, "    {} {:<8} %r{dst}, ", pred_guard(pred), op.mnemonic()).unwrap();
                src.emit(s);
                writeln!(s, ";").unwrap();
            }
            Inst::NarrowCvt { op, dst, src } => {
                let scratch = self.wide_scratch_hi_reg();
                write!(s, "    {:<13} %r{scratch}, ", op.narrow_mnemonic()).unwrap();
                src.emit(s);
                writeln!(s, ";").unwrap();
                writeln!(s, "    {:<13} %r{dst}, %r{scratch};", op.extend_mnemonic()).unwrap();
            }
            Inst::PredicatedNarrowCvt {
                op,
                dst,
                src,
                cmp,
                ca,
                cb,
                pred,
            } => {
                let scratch = self.wide_scratch_hi_reg();
                self.emit_inst_predicate_setup(s, cmp, ca, cb, pred);
                write!(
                    s,
                    "    {} {:<8} %r{scratch}, ",
                    pred_guard(pred),
                    op.narrow_mnemonic()
                )
                .unwrap();
                src.emit(s);
                writeln!(s, ";").unwrap();
                writeln!(
                    s,
                    "    {} {:<8} %r{dst}, %r{scratch};",
                    pred_guard(pred),
                    op.extend_mnemonic()
                )
                .unwrap();
            }
            Inst::WideCvt { op, dst, src } => {
                write!(s, "    {:<13} %rd6, ", op.source_cvt_mnemonic()).unwrap();
                src.emit(s);
                writeln!(s, ";").unwrap();
                writeln!(s, "    {:<13} %r{dst}, %rd6;", op.mnemonic()).unwrap();
            }
            Inst::PredicatedWideCvt {
                op,
                dst,
                src,
                cmp,
                ca,
                cb,
                pred,
            } => {
                self.emit_inst_predicate_setup(s, cmp, ca, cb, pred);
                write!(
                    s,
                    "    {} {:<8} %rd6, ",
                    pred_guard(pred),
                    op.source_cvt_mnemonic()
                )
                .unwrap();
                src.emit(s);
                writeln!(s, ";").unwrap();
                writeln!(
                    s,
                    "    {} {:<8} %r{dst}, %rd6;",
                    pred_guard(pred),
                    op.mnemonic()
                )
                .unwrap();
            }
            Inst::Szext {
                op,
                dst,
                src,
                width,
            } => {
                write!(s, "    {:<17} %r{dst}, ", op.mnemonic()).unwrap();
                src.emit(s);
                write!(s, ", ").unwrap();
                width.emit(s);
                writeln!(s, ";").unwrap();
            }
            Inst::PredicatedSzext {
                op,
                dst,
                src,
                width,
                cmp,
                ca,
                cb,
                pred,
            } => {
                self.emit_inst_predicate_setup(s, cmp, ca, cb, pred);
                write!(
                    s,
                    "    {} {:<12} %r{dst}, ",
                    pred_guard(pred),
                    op.mnemonic()
                )
                .unwrap();
                src.emit(s);
                write!(s, ", ").unwrap();
                width.emit(s);
                writeln!(s, ";").unwrap();
            }
            Inst::Bfind { op, dst, src } => {
                if op.is_wide() {
                    write!(s, "    {:<13} %rd6, ", op.cvt_mnemonic()).unwrap();
                    src.emit(s);
                    writeln!(s, ";").unwrap();
                    writeln!(s, "    {:<13} %r{dst}, %rd6;", op.mnemonic()).unwrap();
                } else {
                    write!(s, "    {:<13} %r{dst}, ", op.mnemonic()).unwrap();
                    src.emit(s);
                    writeln!(s, ";").unwrap();
                }
            }
            Inst::PredicatedBfind {
                op,
                dst,
                src,
                cmp,
                ca,
                cb,
                pred,
            } => {
                self.emit_inst_predicate_setup(s, cmp, ca, cb, pred);
                if op.is_wide() {
                    write!(s, "    {:<13} %rd6, ", op.cvt_mnemonic()).unwrap();
                    src.emit(s);
                    writeln!(s, ";").unwrap();
                    writeln!(
                        s,
                        "    {} {:<8} %r{dst}, %rd6;",
                        pred_guard(pred),
                        op.mnemonic()
                    )
                    .unwrap();
                } else {
                    write!(s, "    {} {:<8} %r{dst}, ", pred_guard(pred), op.mnemonic()).unwrap();
                    src.emit(s);
                    writeln!(s, ";").unwrap();
                }
            }
            Inst::Fns {
                dst,
                mask,
                base,
                offset,
            } => {
                write!(s, "    fns.b32       %r{dst}, ").unwrap();
                mask.emit(s);
                writeln!(s, ", {base}, {offset};").unwrap();
            }
            Inst::PredicatedFns {
                dst,
                mask,
                base,
                offset,
                cmp,
                ca,
                cb,
                pred,
            } => {
                self.emit_inst_predicate_setup(s, cmp, ca, cb, pred);
                write!(s, "    {} fns.b32  %r{dst}, ", pred_guard(pred)).unwrap();
                mask.emit(s);
                writeln!(s, ", {base}, {offset};").unwrap();
            }
            Inst::RegFns {
                dst,
                mask,
                param,
                slot,
                imm,
            } => {
                let scratch = self.wide_scratch_hi_reg();
                write!(s, "    and.b32       %r{scratch}, ").unwrap();
                param.emit(s);
                writeln!(s, ", 31;").unwrap();
                write!(s, "    fns.b32       %r{dst}, ").unwrap();
                mask.emit(s);
                match slot {
                    FnsParamSlot::Base => {
                        writeln!(s, ", %r{scratch}, {imm};").unwrap();
                    }
                    FnsParamSlot::Offset => {
                        writeln!(s, ", {imm}, %r{scratch};").unwrap();
                    }
                }
            }
            Inst::PredicatedRegFns {
                dst,
                mask,
                param,
                slot,
                imm,
                cmp,
                ca,
                cb,
                pred,
            } => {
                let scratch = self.wide_scratch_hi_reg();
                self.emit_inst_predicate_setup(s, cmp, ca, cb, pred);
                write!(s, "    and.b32       %r{scratch}, ").unwrap();
                param.emit(s);
                writeln!(s, ", 31;").unwrap();
                write!(s, "    {} fns.b32  %r{dst}, ", pred_guard(pred)).unwrap();
                mask.emit(s);
                match slot {
                    FnsParamSlot::Base => {
                        writeln!(s, ", %r{scratch}, {imm};").unwrap();
                    }
                    FnsParamSlot::Offset => {
                        writeln!(s, ", {imm}, %r{scratch};").unwrap();
                    }
                }
            }
            Inst::DivRem {
                op,
                dst,
                src,
                divisor,
            } => {
                write!(s, "    {:<13} %r{dst}, ", op.mnemonic()).unwrap();
                src.emit(s);
                writeln!(s, ", {divisor};").unwrap();
            }
            Inst::RegDivRem {
                op,
                dst,
                src,
                divisor,
            } => {
                let scratch = self.wide_scratch_hi_reg();
                write!(s, "    or.b32        %r{scratch}, ").unwrap();
                divisor.emit(s);
                writeln!(s, ", 1;").unwrap();
                write!(s, "    {:<13} %r{dst}, ", op.mnemonic()).unwrap();
                src.emit(s);
                writeln!(s, ", %r{scratch};").unwrap();
            }
            Inst::PredicatedDivRem {
                op,
                dst,
                src,
                divisor,
                cmp,
                ca,
                cb,
                pred,
            } => {
                self.emit_inst_predicate_setup(s, cmp, ca, cb, pred);
                write!(s, "    {} {:<8} %r{dst}, ", pred_guard(pred), op.mnemonic()).unwrap();
                src.emit(s);
                writeln!(s, ", {divisor};").unwrap();
            }
            Inst::PredicatedRegDivRem {
                op,
                dst,
                src,
                divisor,
                cmp,
                ca,
                cb,
                pred,
            } => {
                let scratch = self.wide_scratch_hi_reg();
                self.emit_inst_predicate_setup(s, cmp, ca, cb, pred);
                write!(s, "    or.b32        %r{scratch}, ").unwrap();
                divisor.emit(s);
                writeln!(s, ", 1;").unwrap();
                write!(s, "    {} {:<8} %r{dst}, ", pred_guard(pred), op.mnemonic()).unwrap();
                src.emit(s);
                writeln!(s, ", %r{scratch};").unwrap();
            }
            Inst::Mad24 { op, dst, a, b, c } => {
                write!(s, "    {:<13} %r{dst}, ", op.mnemonic()).unwrap();
                a.emit(s);
                write!(s, ", ").unwrap();
                b.emit(s);
                write!(s, ", ").unwrap();
                c.emit(s);
                writeln!(s, ";").unwrap();
            }
            Inst::PredicatedMad24 {
                op,
                dst,
                a,
                b,
                c,
                cmp,
                ca,
                cb,
                pred,
            } => {
                self.emit_inst_predicate_setup(s, cmp, ca, cb, pred);
                write!(s, "    {} {:<8} %r{dst}, ", pred_guard(pred), op.mnemonic()).unwrap();
                a.emit(s);
                write!(s, ", ").unwrap();
                b.emit(s);
                write!(s, ", ").unwrap();
                c.emit(s);
                writeln!(s, ";").unwrap();
            }
            Inst::Mul24 { op, dst, a, b } => {
                write!(s, "    {:<13} %r{dst}, ", op.mnemonic()).unwrap();
                a.emit(s);
                write!(s, ", ").unwrap();
                b.emit(s);
                writeln!(s, ";").unwrap();
            }
            Inst::PredicatedMul24 {
                op,
                dst,
                a,
                b,
                cmp,
                ca,
                cb,
                pred,
            } => {
                self.emit_inst_predicate_setup(s, cmp, ca, cb, pred);
                write!(s, "    {} {:<8} %r{dst}, ", pred_guard(pred), op.mnemonic()).unwrap();
                a.emit(s);
                write!(s, ", ").unwrap();
                b.emit(s);
                writeln!(s, ";").unwrap();
            }
            Inst::SubwordWide { op, dst, a, b, c } => {
                write!(s, "    {:<13} %h0, ", op.cvt_mnemonic()).unwrap();
                a.emit(s);
                writeln!(s, ";").unwrap();
                write!(s, "    {:<13} %h1, ", op.cvt_mnemonic()).unwrap();
                b.emit(s);
                writeln!(s, ";").unwrap();
                write!(s, "    {:<13} %r{dst}, %h0, %h1", op.mnemonic()).unwrap();
                if op.is_mad() {
                    write!(s, ", ").unwrap();
                    c.emit(s);
                }
                writeln!(s, ";").unwrap();
            }
            Inst::PredicatedSubwordWide {
                op,
                dst,
                a,
                b,
                c,
                cmp,
                ca,
                cb,
                pred,
            } => {
                self.emit_inst_predicate_setup(s, cmp, ca, cb, pred);
                write!(s, "    {:<13} %h0, ", op.cvt_mnemonic()).unwrap();
                a.emit(s);
                writeln!(s, ";").unwrap();
                write!(s, "    {:<13} %h1, ", op.cvt_mnemonic()).unwrap();
                b.emit(s);
                writeln!(s, ";").unwrap();
                write!(
                    s,
                    "    {} {:<8} %r{dst}, %h0, %h1",
                    pred_guard(pred),
                    op.mnemonic()
                )
                .unwrap();
                if op.is_mad() {
                    write!(s, ", ").unwrap();
                    c.emit(s);
                }
                writeln!(s, ";").unwrap();
            }
            Inst::MulWide {
                op,
                dst,
                a,
                b,
                keep_high,
            } => {
                let scratch_hi = self.wide_scratch_hi_reg();
                write!(s, "    {:<13} %rd6, ", op.mnemonic()).unwrap();
                a.emit(s);
                write!(s, ", ").unwrap();
                b.emit(s);
                writeln!(s, ";").unwrap();
                if keep_high {
                    writeln!(s, "    mov.b64       {{%r{scratch_hi}, %r{dst}}}, %rd6;").unwrap();
                } else {
                    writeln!(s, "    mov.b64       {{%r{dst}, %r{scratch_hi}}}, %rd6;").unwrap();
                }
            }
            Inst::PredicatedMulWide {
                op,
                dst,
                a,
                b,
                keep_high,
                cmp,
                ca,
                cb,
                pred,
            } => {
                let scratch_hi = self.wide_scratch_hi_reg();
                self.emit_inst_predicate_setup(s, cmp, ca, cb, pred);
                write!(s, "    {} {:<8} %rd6, ", pred_guard(pred), op.mnemonic()).unwrap();
                a.emit(s);
                write!(s, ", ").unwrap();
                b.emit(s);
                writeln!(s, ";").unwrap();
                if keep_high {
                    writeln!(
                        s,
                        "    {} mov.b64 {{%r{scratch_hi}, %r{dst}}}, %rd6;",
                        pred_guard(pred)
                    )
                    .unwrap();
                } else {
                    writeln!(
                        s,
                        "    {} mov.b64 {{%r{dst}, %r{scratch_hi}}}, %rd6;",
                        pred_guard(pred)
                    )
                    .unwrap();
                }
            }
            Inst::MadWide {
                op,
                dst,
                a,
                b,
                c,
                keep_high,
            } => {
                let scratch_hi = self.wide_scratch_hi_reg();
                write!(s, "    {:<13} %rd7, ", op.cvt_mnemonic()).unwrap();
                c.emit(s);
                writeln!(s, ";").unwrap();
                write!(s, "    {:<13} %rd6, ", op.mnemonic()).unwrap();
                a.emit(s);
                write!(s, ", ").unwrap();
                b.emit(s);
                writeln!(s, ", %rd7;").unwrap();
                if keep_high {
                    writeln!(s, "    mov.b64       {{%r{scratch_hi}, %r{dst}}}, %rd6;").unwrap();
                } else {
                    writeln!(s, "    mov.b64       {{%r{dst}, %r{scratch_hi}}}, %rd6;").unwrap();
                }
            }
            Inst::PredicatedMadWide {
                op,
                dst,
                a,
                b,
                c,
                keep_high,
                cmp,
                ca,
                cb,
                pred,
            } => {
                let scratch_hi = self.wide_scratch_hi_reg();
                self.emit_inst_predicate_setup(s, cmp, ca, cb, pred);
                write!(s, "    {:<13} %rd7, ", op.cvt_mnemonic()).unwrap();
                c.emit(s);
                writeln!(s, ";").unwrap();
                write!(s, "    {} {:<8} %rd6, ", pred_guard(pred), op.mnemonic()).unwrap();
                a.emit(s);
                write!(s, ", ").unwrap();
                b.emit(s);
                writeln!(s, ", %rd7;").unwrap();
                if keep_high {
                    writeln!(
                        s,
                        "    {} mov.b64 {{%r{scratch_hi}, %r{dst}}}, %rd6;",
                        pred_guard(pred)
                    )
                    .unwrap();
                } else {
                    writeln!(
                        s,
                        "    {} mov.b64 {{%r{dst}, %r{scratch_hi}}}, %rd6;",
                        pred_guard(pred)
                    )
                    .unwrap();
                }
            }
            Inst::WideInt { op, dst, a, b } => {
                let scratch_hi = self.wide_scratch_hi_reg();
                write!(s, "    {:<13} %rd6, ", op.cvt_mnemonic()).unwrap();
                a.emit(s);
                writeln!(s, ";").unwrap();
                write!(s, "    {:<13} %rd7, ", op.cvt_mnemonic()).unwrap();
                b.emit(s);
                writeln!(s, ";").unwrap();
                writeln!(s, "    {:<13} %rd6, %rd6, %rd7;", op.mnemonic()).unwrap();
                writeln!(s, "    mov.b64       {{%r{dst}, %r{scratch_hi}}}, %rd6;").unwrap();
            }
            Inst::PredicatedWideInt {
                op,
                dst,
                a,
                b,
                cmp,
                ca,
                cb,
                pred,
            } => {
                let scratch_hi = self.wide_scratch_hi_reg();
                self.emit_inst_predicate_setup(s, cmp, ca, cb, pred);
                write!(s, "    {:<13} %rd6, ", op.cvt_mnemonic()).unwrap();
                a.emit(s);
                writeln!(s, ";").unwrap();
                write!(s, "    {:<13} %rd7, ", op.cvt_mnemonic()).unwrap();
                b.emit(s);
                writeln!(s, ";").unwrap();
                writeln!(
                    s,
                    "    {} {:<8} %rd6, %rd6, %rd7;",
                    pred_guard(pred),
                    op.mnemonic()
                )
                .unwrap();
                writeln!(
                    s,
                    "    {} mov.b64 {{%r{dst}, %r{scratch_hi}}}, %rd6;",
                    pred_guard(pred)
                )
                .unwrap();
            }
            Inst::WideMad64 { op, dst, a, b, c } => {
                let scratch_hi = self.wide_scratch_hi_reg();
                write!(s, "    {:<13} %rd4, ", op.cvt_mnemonic()).unwrap();
                a.emit(s);
                writeln!(s, ";").unwrap();
                write!(s, "    {:<13} %rd5, ", op.cvt_mnemonic()).unwrap();
                b.emit(s);
                writeln!(s, ";").unwrap();
                write!(s, "    {:<13} %rd6, ", op.cvt_mnemonic()).unwrap();
                c.emit(s);
                writeln!(s, ";").unwrap();
                writeln!(s, "    {:<13} %rd6, %rd4, %rd5, %rd6;", op.mnemonic()).unwrap();
                writeln!(s, "    mov.b64       {{%r{dst}, %r{scratch_hi}}}, %rd6;").unwrap();
            }
            Inst::PredicatedWideMad64 {
                op,
                dst,
                a,
                b,
                c,
                cmp,
                ca,
                cb,
                pred,
            } => {
                let scratch_hi = self.wide_scratch_hi_reg();
                self.emit_inst_predicate_setup(s, cmp, ca, cb, pred);
                write!(s, "    {:<13} %rd4, ", op.cvt_mnemonic()).unwrap();
                a.emit(s);
                writeln!(s, ";").unwrap();
                write!(s, "    {:<13} %rd5, ", op.cvt_mnemonic()).unwrap();
                b.emit(s);
                writeln!(s, ";").unwrap();
                write!(s, "    {:<13} %rd6, ", op.cvt_mnemonic()).unwrap();
                c.emit(s);
                writeln!(s, ";").unwrap();
                writeln!(
                    s,
                    "    {} {:<8} %rd6, %rd4, %rd5, %rd6;",
                    pred_guard(pred),
                    op.mnemonic()
                )
                .unwrap();
                writeln!(
                    s,
                    "    {} mov.b64 {{%r{dst}, %r{scratch_hi}}}, %rd6;",
                    pred_guard(pred)
                )
                .unwrap();
            }
            Inst::WideSetpBin {
                cmp,
                ca,
                cb,
                pred,
                op,
                dst,
                a,
                b,
            } => {
                write!(s, "    {:<13} %rd6, ", cmp.wide_cvt_mnemonic()).unwrap();
                ca.emit(s);
                writeln!(s, ";").unwrap();
                write!(s, "    {:<13} %rd7, ", cmp.wide_cvt_mnemonic()).unwrap();
                cb.emit(s);
                writeln!(s, ";").unwrap();
                writeln!(
                    s,
                    "    {:<13} %p{}, %rd6, %rd7;",
                    cmp.wide_setp_mnemonic(),
                    pred_id(pred)
                )
                .unwrap();
                write!(s, "    {} {:<8} %r{dst}, ", pred_guard(pred), op.mnemonic()).unwrap();
                a.emit(s);
                write!(s, ", ").unwrap();
                b.emit(s);
                writeln!(s, ";").unwrap();
            }
            Inst::WideSetpBoolBin {
                bool_op,
                base_cmp,
                base_a,
                base_b,
                cmp,
                cmp_a,
                cmp_b,
                base_pred,
                guard_pred,
                op,
                dst,
                a,
                b,
            } => {
                write!(s, "    {:<13} %rd6, ", base_cmp.wide_cvt_mnemonic()).unwrap();
                base_a.emit(s);
                writeln!(s, ";").unwrap();
                write!(s, "    {:<13} %rd7, ", base_cmp.wide_cvt_mnemonic()).unwrap();
                base_b.emit(s);
                writeln!(s, ";").unwrap();
                writeln!(
                    s,
                    "    {:<13} %p{base_pred}, %rd6, %rd7;",
                    base_cmp.wide_setp_mnemonic()
                )
                .unwrap();
                write!(s, "    {:<13} %rd6, ", cmp.wide_cvt_mnemonic()).unwrap();
                cmp_a.emit(s);
                writeln!(s, ";").unwrap();
                write!(s, "    {:<13} %rd7, ", cmp.wide_cvt_mnemonic()).unwrap();
                cmp_b.emit(s);
                writeln!(s, ";").unwrap();
                writeln!(
                    s,
                    "    {:<13} %p{}, %rd6, %rd7, %p{base_pred};",
                    cmp.wide_setp_bool_mnemonic(bool_op),
                    pred_id(guard_pred)
                )
                .unwrap();
                write!(
                    s,
                    "    {} {:<8} %r{dst}, ",
                    pred_guard(guard_pred),
                    op.mnemonic()
                )
                .unwrap();
                a.emit(s);
                write!(s, ", ").unwrap();
                b.emit(s);
                writeln!(s, ";").unwrap();
            }
            Inst::WideSet { dst, cmp, a, b } => {
                write!(s, "    {:<13} %rd6, ", cmp.wide_cvt_mnemonic()).unwrap();
                a.emit(s);
                writeln!(s, ";").unwrap();
                write!(s, "    {:<13} %rd7, ", cmp.wide_cvt_mnemonic()).unwrap();
                b.emit(s);
                writeln!(s, ";").unwrap();
                writeln!(
                    s,
                    "    {:<13} %r{dst}, %rd6, %rd7;",
                    cmp.wide_set_mnemonic()
                )
                .unwrap();
            }
            Inst::PredicatedWideSet {
                dst,
                cmp,
                a,
                b,
                guard_cmp,
                guard_ca,
                guard_cb,
                guard_pred,
            } => {
                self.emit_inst_predicate_setup(s, guard_cmp, guard_ca, guard_cb, guard_pred);
                write!(s, "    {:<13} %rd6, ", cmp.wide_cvt_mnemonic()).unwrap();
                a.emit(s);
                writeln!(s, ";").unwrap();
                write!(s, "    {:<13} %rd7, ", cmp.wide_cvt_mnemonic()).unwrap();
                b.emit(s);
                writeln!(s, ";").unwrap();
                writeln!(
                    s,
                    "    {} {:<8} %r{dst}, %rd6, %rd7;",
                    pred_guard(guard_pred),
                    cmp.wide_set_mnemonic()
                )
                .unwrap();
            }
            Inst::WideSelp {
                op,
                cmp,
                ca,
                cb,
                pred,
                dst,
                true_value,
                false_value,
            } => {
                let scratch_hi = self.wide_scratch_hi_reg();
                write!(s, "    {:<13} %rd6, ", cmp.wide_cvt_mnemonic()).unwrap();
                ca.emit(s);
                writeln!(s, ";").unwrap();
                write!(s, "    {:<13} %rd7, ", cmp.wide_cvt_mnemonic()).unwrap();
                cb.emit(s);
                writeln!(s, ";").unwrap();
                writeln!(
                    s,
                    "    {:<13} %p{pred}, %rd6, %rd7;",
                    cmp.wide_setp_mnemonic()
                )
                .unwrap();
                write!(s, "    cvt.u64.u32  %rd6, ").unwrap();
                true_value.emit(s);
                writeln!(s, ";").unwrap();
                write!(s, "    cvt.u64.u32  %rd7, ").unwrap();
                false_value.emit(s);
                writeln!(s, ";").unwrap();
                writeln!(s, "    {:<12} %rd6, %rd6, %rd7, %p{pred};", op.mnemonic()).unwrap();
                writeln!(s, "    mov.b64      {{%r{dst}, %r{scratch_hi}}}, %rd6;").unwrap();
            }
            Inst::WideUnary { op, dst, src } => {
                let scratch_hi = self.wide_scratch_hi_reg();
                write!(s, "    {:<13} %rd6, ", op.cvt_mnemonic()).unwrap();
                src.emit(s);
                writeln!(s, ";").unwrap();
                if op.writes_b64() {
                    writeln!(s, "    {:<13} %rd6, %rd6;", op.mnemonic()).unwrap();
                    writeln!(s, "    mov.b64      {{%r{dst}, %r{scratch_hi}}}, %rd6;").unwrap();
                } else {
                    writeln!(s, "    {:<13} %r{dst}, %rd6;", op.mnemonic()).unwrap();
                }
            }
            Inst::PredicatedWideUnary {
                op,
                dst,
                src,
                cmp,
                ca,
                cb,
                pred,
            } => {
                let scratch_hi = self.wide_scratch_hi_reg();
                self.emit_inst_predicate_setup(s, cmp, ca, cb, pred);
                write!(s, "    {:<13} %rd6, ", op.cvt_mnemonic()).unwrap();
                src.emit(s);
                writeln!(s, ";").unwrap();
                if op.writes_b64() {
                    writeln!(
                        s,
                        "    {} {:<8} %rd6, %rd6;",
                        pred_guard(pred),
                        op.mnemonic()
                    )
                    .unwrap();
                    writeln!(
                        s,
                        "    {} mov.b64 {{%r{dst}, %r{scratch_hi}}}, %rd6;",
                        pred_guard(pred)
                    )
                    .unwrap();
                } else {
                    writeln!(
                        s,
                        "    {} {:<8} %r{dst}, %rd6;",
                        pred_guard(pred),
                        op.mnemonic()
                    )
                    .unwrap();
                }
            }
            Inst::WideShift {
                op,
                dst,
                src,
                amount,
            } => {
                let scratch_hi = self.wide_scratch_hi_reg();
                write!(s, "    {:<13} %rd6, ", op.cvt_mnemonic()).unwrap();
                src.emit(s);
                writeln!(s, ";").unwrap();
                writeln!(s, "    {:<13} %rd6, %rd6, {amount};", op.mnemonic()).unwrap();
                writeln!(s, "    mov.b64       {{%r{dst}, %r{scratch_hi}}}, %rd6;").unwrap();
            }
            Inst::RegWideShift {
                op,
                dst,
                src,
                amount,
            } => {
                let scratch_hi = self.wide_scratch_hi_reg();
                write!(s, "    {:<13} %rd6, ", op.cvt_mnemonic()).unwrap();
                src.emit(s);
                writeln!(s, ";").unwrap();
                write!(s, "    and.b32       %r{scratch_hi}, ").unwrap();
                amount.emit(s);
                writeln!(s, ", 63;").unwrap();
                writeln!(s, "    {:<13} %rd6, %rd6, %r{scratch_hi};", op.mnemonic()).unwrap();
                writeln!(s, "    mov.b64       {{%r{dst}, %r{scratch_hi}}}, %rd6;").unwrap();
            }
            Inst::PredicatedWideShift {
                op,
                dst,
                src,
                amount,
                cmp,
                ca,
                cb,
                pred,
            } => {
                let scratch_hi = self.wide_scratch_hi_reg();
                self.emit_inst_predicate_setup(s, cmp, ca, cb, pred);
                write!(s, "    {:<13} %rd6, ", op.cvt_mnemonic()).unwrap();
                src.emit(s);
                writeln!(s, ";").unwrap();
                writeln!(
                    s,
                    "    {} {:<8} %rd6, %rd6, {amount};",
                    pred_guard(pred),
                    op.mnemonic()
                )
                .unwrap();
                writeln!(
                    s,
                    "    {} mov.b64 {{%r{dst}, %r{scratch_hi}}}, %rd6;",
                    pred_guard(pred)
                )
                .unwrap();
            }
            Inst::PredicatedRegWideShift {
                op,
                dst,
                src,
                amount,
                cmp,
                ca,
                cb,
                pred,
            } => {
                let scratch_hi = self.wide_scratch_hi_reg();
                self.emit_inst_predicate_setup(s, cmp, ca, cb, pred);
                write!(s, "    {:<13} %rd6, ", op.cvt_mnemonic()).unwrap();
                src.emit(s);
                writeln!(s, ";").unwrap();
                write!(s, "    and.b32       %r{scratch_hi}, ").unwrap();
                amount.emit(s);
                writeln!(s, ", 63;").unwrap();
                writeln!(
                    s,
                    "    {} {:<8} %rd6, %rd6, %r{scratch_hi};",
                    pred_guard(pred),
                    op.mnemonic()
                )
                .unwrap();
                writeln!(
                    s,
                    "    {} mov.b64 {{%r{dst}, %r{scratch_hi}}}, %rd6;",
                    pred_guard(pred)
                )
                .unwrap();
            }
            Inst::WideDivRem {
                op,
                dst,
                src,
                divisor,
            } => {
                let scratch_hi = self.wide_scratch_hi_reg();
                write!(s, "    {:<13} %rd6, ", op.cvt_mnemonic()).unwrap();
                src.emit(s);
                writeln!(s, ";").unwrap();
                let divisor = self.emit_wide_divisor(s, op, divisor);
                writeln!(s, "    {:<13} %rd6, %rd6, {divisor};", op.mnemonic()).unwrap();
                writeln!(s, "    mov.b64       {{%r{dst}, %r{scratch_hi}}}, %rd6;").unwrap();
            }
            Inst::PredicatedWideDivRem {
                op,
                dst,
                src,
                divisor,
                cmp,
                ca,
                cb,
                pred,
            } => {
                let scratch_hi = self.wide_scratch_hi_reg();
                self.emit_inst_predicate_setup(s, cmp, ca, cb, pred);
                write!(s, "    {:<13} %rd6, ", op.cvt_mnemonic()).unwrap();
                src.emit(s);
                writeln!(s, ";").unwrap();
                let divisor = self.emit_wide_divisor(s, op, divisor);
                writeln!(
                    s,
                    "    {} {:<8} %rd6, %rd6, {divisor};",
                    pred_guard(pred),
                    op.mnemonic()
                )
                .unwrap();
                writeln!(
                    s,
                    "    {} mov.b64 {{%r{dst}, %r{scratch_hi}}}, %rd6;",
                    pred_guard(pred)
                )
                .unwrap();
            }
            Inst::WideCarry {
                op,
                dst_lo,
                dst_hi,
                a,
                b,
                c,
                d,
            } => {
                let scratch_hi = self.wide_scratch_hi_reg();
                let (first, second) = op.wide_mnemonic_pair();
                write!(s, "    cvt.u64.u32  %rd4, ").unwrap();
                a.emit(s);
                writeln!(s, ";").unwrap();
                write!(s, "    cvt.u64.u32  %rd5, ").unwrap();
                b.emit(s);
                writeln!(s, ";").unwrap();
                write!(s, "    cvt.u64.u32  %rd6, ").unwrap();
                c.emit(s);
                writeln!(s, ";").unwrap();
                write!(s, "    cvt.u64.u32  %rd7, ").unwrap();
                d.emit(s);
                writeln!(s, ";").unwrap();
                writeln!(s, "    {first:<13} %rd4, %rd4, %rd5;").unwrap();
                writeln!(s, "    {second:<13} %rd6, %rd6, %rd7;").unwrap();
                writeln!(s, "    mov.b64       {{%r{dst_lo}, %r{scratch_hi}}}, %rd4;").unwrap();
                writeln!(s, "    mov.b64       {{%r{dst_hi}, %r{scratch_hi}}}, %rd6;").unwrap();
            }
            Inst::PredicatedWideCarry {
                op,
                dst_lo,
                dst_hi,
                a,
                b,
                c,
                d,
                cmp,
                ca,
                cb,
                pred,
            } => {
                let scratch_hi = self.wide_scratch_hi_reg();
                let (first, second) = op.wide_mnemonic_pair();
                self.emit_inst_predicate_setup(s, cmp, ca, cb, pred);
                write!(s, "    cvt.u64.u32  %rd4, ").unwrap();
                a.emit(s);
                writeln!(s, ";").unwrap();
                write!(s, "    cvt.u64.u32  %rd5, ").unwrap();
                b.emit(s);
                writeln!(s, ";").unwrap();
                write!(s, "    cvt.u64.u32  %rd6, ").unwrap();
                c.emit(s);
                writeln!(s, ";").unwrap();
                write!(s, "    cvt.u64.u32  %rd7, ").unwrap();
                d.emit(s);
                writeln!(s, ";").unwrap();
                writeln!(s, "    {} {first:<8} %rd4, %rd4, %rd5;", pred_guard(pred)).unwrap();
                writeln!(s, "    {} {second:<8} %rd6, %rd6, %rd7;", pred_guard(pred)).unwrap();
                writeln!(
                    s,
                    "    {} mov.b64 {{%r{dst_lo}, %r{scratch_hi}}}, %rd4;",
                    pred_guard(pred)
                )
                .unwrap();
                writeln!(
                    s,
                    "    {} mov.b64 {{%r{dst_hi}, %r{scratch_hi}}}, %rd6;",
                    pred_guard(pred)
                )
                .unwrap();
            }
            Inst::WideCarryChain {
                op,
                dst0,
                dst1,
                dst2,
                a,
                b,
                c,
                d,
                e,
                f,
            } => {
                let scratch_hi = self.wide_scratch_hi_reg();
                let (first, second, third) = op.wide_mnemonic_triple();
                write!(s, "    cvt.u64.u32  %rd4, ").unwrap();
                a.emit(s);
                writeln!(s, ";").unwrap();
                write!(s, "    cvt.u64.u32  %rd5, ").unwrap();
                b.emit(s);
                writeln!(s, ";").unwrap();
                write!(s, "    cvt.u64.u32  %rd6, ").unwrap();
                c.emit(s);
                writeln!(s, ";").unwrap();
                write!(s, "    cvt.u64.u32  %rd7, ").unwrap();
                d.emit(s);
                writeln!(s, ";").unwrap();
                write!(s, "    cvt.u64.u32  %rd8, ").unwrap();
                e.emit(s);
                writeln!(s, ";").unwrap();
                write!(s, "    cvt.u64.u32  %rd9, ").unwrap();
                f.emit(s);
                writeln!(s, ";").unwrap();
                writeln!(s, "    {first:<13} %rd4, %rd4, %rd5;").unwrap();
                writeln!(s, "    {second:<13} %rd6, %rd6, %rd7;").unwrap();
                writeln!(s, "    {third:<13} %rd8, %rd8, %rd9;").unwrap();
                writeln!(s, "    mov.b64       {{%r{dst0}, %r{scratch_hi}}}, %rd4;").unwrap();
                writeln!(s, "    mov.b64       {{%r{dst1}, %r{scratch_hi}}}, %rd6;").unwrap();
                writeln!(s, "    mov.b64       {{%r{dst2}, %r{scratch_hi}}}, %rd8;").unwrap();
            }
            Inst::PredicatedWideCarryChain {
                op,
                dst0,
                dst1,
                dst2,
                a,
                b,
                c,
                d,
                e,
                f,
                cmp,
                ca,
                cb,
                pred,
            } => {
                let scratch_hi = self.wide_scratch_hi_reg();
                let (first, second, third) = op.wide_mnemonic_triple();
                self.emit_inst_predicate_setup(s, cmp, ca, cb, pred);
                write!(s, "    cvt.u64.u32  %rd4, ").unwrap();
                a.emit(s);
                writeln!(s, ";").unwrap();
                write!(s, "    cvt.u64.u32  %rd5, ").unwrap();
                b.emit(s);
                writeln!(s, ";").unwrap();
                write!(s, "    cvt.u64.u32  %rd6, ").unwrap();
                c.emit(s);
                writeln!(s, ";").unwrap();
                write!(s, "    cvt.u64.u32  %rd7, ").unwrap();
                d.emit(s);
                writeln!(s, ";").unwrap();
                write!(s, "    cvt.u64.u32  %rd8, ").unwrap();
                e.emit(s);
                writeln!(s, ";").unwrap();
                write!(s, "    cvt.u64.u32  %rd9, ").unwrap();
                f.emit(s);
                writeln!(s, ";").unwrap();
                writeln!(s, "    {} {first:<8} %rd4, %rd4, %rd5;", pred_guard(pred)).unwrap();
                writeln!(s, "    {} {second:<8} %rd6, %rd6, %rd7;", pred_guard(pred)).unwrap();
                writeln!(s, "    {} {third:<8} %rd8, %rd8, %rd9;", pred_guard(pred)).unwrap();
                writeln!(
                    s,
                    "    {} mov.b64 {{%r{dst0}, %r{scratch_hi}}}, %rd4;",
                    pred_guard(pred)
                )
                .unwrap();
                writeln!(
                    s,
                    "    {} mov.b64 {{%r{dst1}, %r{scratch_hi}}}, %rd6;",
                    pred_guard(pred)
                )
                .unwrap();
                writeln!(
                    s,
                    "    {} mov.b64 {{%r{dst2}, %r{scratch_hi}}}, %rd8;",
                    pred_guard(pred)
                )
                .unwrap();
            }
            Inst::AddCarry {
                op,
                dst_lo,
                dst_hi,
                a,
                b,
                c,
                d,
            } => {
                let (first, second) = op.mnemonic_pair();
                write!(s, "    {first:<13} %r{dst_lo}, ").unwrap();
                a.emit(s);
                write!(s, ", ").unwrap();
                b.emit(s);
                writeln!(s, ";").unwrap();
                write!(s, "    {second:<13} %r{dst_hi}, ").unwrap();
                c.emit(s);
                write!(s, ", ").unwrap();
                d.emit(s);
                writeln!(s, ";").unwrap();
            }
            Inst::PredicatedAddCarry {
                op,
                dst_lo,
                dst_hi,
                a,
                b,
                c,
                d,
                cmp,
                ca,
                cb,
                pred,
            } => {
                let (first, second) = op.mnemonic_pair();
                self.emit_inst_predicate_setup(s, cmp, ca, cb, pred);
                write!(s, "    {} {first:<8} %r{dst_lo}, ", pred_guard(pred)).unwrap();
                a.emit(s);
                write!(s, ", ").unwrap();
                b.emit(s);
                writeln!(s, ";").unwrap();
                write!(s, "    {} {second:<8} %r{dst_hi}, ", pred_guard(pred)).unwrap();
                c.emit(s);
                write!(s, ", ").unwrap();
                d.emit(s);
                writeln!(s, ";").unwrap();
            }
            Inst::CarryChain {
                op,
                dst0,
                dst1,
                dst2,
                a,
                b,
                c,
                d,
                e,
                f,
            } => {
                let (first, second, third) = op.mnemonic_triple();
                write!(s, "    {first:<13} %r{dst0}, ").unwrap();
                a.emit(s);
                write!(s, ", ").unwrap();
                b.emit(s);
                writeln!(s, ";").unwrap();
                write!(s, "    {second:<13} %r{dst1}, ").unwrap();
                c.emit(s);
                write!(s, ", ").unwrap();
                d.emit(s);
                writeln!(s, ";").unwrap();
                write!(s, "    {third:<13} %r{dst2}, ").unwrap();
                e.emit(s);
                write!(s, ", ").unwrap();
                f.emit(s);
                writeln!(s, ";").unwrap();
            }
            Inst::PredicatedCarryChain {
                op,
                dst0,
                dst1,
                dst2,
                a,
                b,
                c,
                d,
                e,
                f,
                cmp,
                ca,
                cb,
                pred,
            } => {
                let (first, second, third) = op.mnemonic_triple();
                self.emit_inst_predicate_setup(s, cmp, ca, cb, pred);
                write!(s, "    {} {first:<8} %r{dst0}, ", pred_guard(pred)).unwrap();
                a.emit(s);
                write!(s, ", ").unwrap();
                b.emit(s);
                writeln!(s, ";").unwrap();
                write!(s, "    {} {second:<8} %r{dst1}, ", pred_guard(pred)).unwrap();
                c.emit(s);
                write!(s, ", ").unwrap();
                d.emit(s);
                writeln!(s, ";").unwrap();
                write!(s, "    {} {third:<8} %r{dst2}, ", pred_guard(pred)).unwrap();
                e.emit(s);
                write!(s, ", ").unwrap();
                f.emit(s);
                writeln!(s, ";").unwrap();
            }
            Inst::Sad { op, dst, a, b, c } => {
                write!(s, "    {:<13} %r{dst}, ", op.mnemonic()).unwrap();
                a.emit(s);
                write!(s, ", ").unwrap();
                b.emit(s);
                write!(s, ", ").unwrap();
                c.emit(s);
                writeln!(s, ";").unwrap();
            }
            Inst::PredicatedSad {
                op,
                dst,
                a,
                b,
                c,
                cmp,
                ca,
                cb,
                pred,
            } => {
                self.emit_inst_predicate_setup(s, cmp, ca, cb, pred);
                write!(s, "    {} {:<8} %r{dst}, ", pred_guard(pred), op.mnemonic()).unwrap();
                a.emit(s);
                write!(s, ", ").unwrap();
                b.emit(s);
                write!(s, ", ").unwrap();
                c.emit(s);
                writeln!(s, ";").unwrap();
            }
            Inst::Slct { op, dst, a, b, c } => {
                if op.dst_is_f32() {
                    self.emit_raw_f32_operand(s, 0, a, false);
                    self.emit_raw_f32_operand(s, 1, b, false);
                    if op.selector_is_f32() {
                        self.emit_raw_f32_operand(s, 2, c, true);
                        writeln!(s, "    {:<13} %f3, %f0, %f1, %f2;", op.mnemonic()).unwrap();
                    } else {
                        write!(s, "    {:<13} %f3, %f0, %f1, ", op.mnemonic()).unwrap();
                        c.emit(s);
                        writeln!(s, ";").unwrap();
                    }
                    writeln!(s, "    cvt.rzi.s32.f32 %r{dst}, %f3;").unwrap();
                } else if op.dst_is_f64() {
                    self.emit_raw_f64_operand(s, 0, a, true);
                    self.emit_raw_f64_operand(s, 1, b, true);
                    if op.selector_is_f32() {
                        self.emit_raw_f32_operand(s, 0, c, true);
                        writeln!(s, "    {:<13} %fd3, %fd0, %fd1, %f0;", op.mnemonic()).unwrap();
                    } else {
                        write!(s, "    {:<13} %fd3, %fd0, %fd1, ", op.mnemonic()).unwrap();
                        c.emit(s);
                        writeln!(s, ";").unwrap();
                    }
                    writeln!(s, "    cvt.rzi.s32.f64 %r{dst}, %fd3;").unwrap();
                } else if op.dst_is_wide() {
                    let signed = op.wide_input_is_signed();
                    let scratch = self.wide_scratch_hi_reg();
                    self.emit_raw_wide_operand(s, 6, a, signed);
                    self.emit_raw_wide_operand(s, 7, b, signed);
                    if op.selector_is_f32() {
                        self.emit_raw_f32_operand(s, 0, c, true);
                        writeln!(s, "    {:<13} %rd5, %rd6, %rd7, %f0;", op.mnemonic()).unwrap();
                    } else {
                        write!(s, "    {:<13} %rd5, %rd6, %rd7, ", op.mnemonic()).unwrap();
                        c.emit(s);
                        writeln!(s, ";").unwrap();
                    }
                    writeln!(s, "    mov.b64       {{%r{dst}, %r{scratch}}}, %rd5;").unwrap();
                } else {
                    if op.selector_is_f32() {
                        self.emit_raw_f32_operand(s, 0, c, true);
                    }
                    write!(s, "    {:<13} %r{dst}, ", op.mnemonic()).unwrap();
                    a.emit(s);
                    write!(s, ", ").unwrap();
                    b.emit(s);
                    write!(s, ", ").unwrap();
                    if op.selector_is_f32() {
                        writeln!(s, "%f0;").unwrap();
                    } else {
                        c.emit(s);
                        writeln!(s, ";").unwrap();
                    }
                }
            }
            Inst::PredicatedSlct {
                op,
                dst,
                a,
                b,
                c,
                cmp,
                ca,
                cb,
                pred,
            } => {
                self.emit_inst_predicate_setup(s, cmp, ca, cb, pred);
                if op.dst_is_f32() {
                    self.emit_raw_f32_operand(s, 0, a, false);
                    self.emit_raw_f32_operand(s, 1, b, false);
                    writeln!(s, "    mov.f32       %f3, %f0;").unwrap();
                    if op.selector_is_f32() {
                        self.emit_raw_f32_operand(s, 2, c, true);
                        writeln!(
                            s,
                            "    {} {:<8} %f3, %f0, %f1, %f2;",
                            pred_guard(pred),
                            op.mnemonic()
                        )
                        .unwrap();
                    } else {
                        write!(
                            s,
                            "    {} {:<8} %f3, %f0, %f1, ",
                            pred_guard(pred),
                            op.mnemonic()
                        )
                        .unwrap();
                        c.emit(s);
                        writeln!(s, ";").unwrap();
                    }
                    writeln!(s, "    cvt.rzi.s32.f32 %r{dst}, %f3;").unwrap();
                } else if op.dst_is_f64() {
                    self.emit_raw_f64_operand(s, 0, a, true);
                    self.emit_raw_f64_operand(s, 1, b, true);
                    writeln!(s, "    mov.f64       %fd3, %fd0;").unwrap();
                    if op.selector_is_f32() {
                        self.emit_raw_f32_operand(s, 0, c, true);
                        writeln!(
                            s,
                            "    {} {:<8} %fd3, %fd0, %fd1, %f0;",
                            pred_guard(pred),
                            op.mnemonic()
                        )
                        .unwrap();
                    } else {
                        write!(
                            s,
                            "    {} {:<8} %fd3, %fd0, %fd1, ",
                            pred_guard(pred),
                            op.mnemonic()
                        )
                        .unwrap();
                        c.emit(s);
                        writeln!(s, ";").unwrap();
                    }
                    writeln!(s, "    cvt.rzi.s32.f64 %r{dst}, %fd3;").unwrap();
                } else if op.dst_is_wide() {
                    let signed = op.wide_input_is_signed();
                    let scratch = self.wide_scratch_hi_reg();
                    self.emit_raw_wide_operand(s, 6, a, signed);
                    self.emit_raw_wide_operand(s, 7, b, signed);
                    writeln!(s, "    mov.b64       %rd5, %rd6;").unwrap();
                    if op.selector_is_f32() {
                        self.emit_raw_f32_operand(s, 0, c, true);
                        writeln!(
                            s,
                            "    {} {:<8} %rd5, %rd6, %rd7, %f0;",
                            pred_guard(pred),
                            op.mnemonic()
                        )
                        .unwrap();
                    } else {
                        write!(
                            s,
                            "    {} {:<8} %rd5, %rd6, %rd7, ",
                            pred_guard(pred),
                            op.mnemonic()
                        )
                        .unwrap();
                        c.emit(s);
                        writeln!(s, ";").unwrap();
                    }
                    writeln!(s, "    mov.b64       {{%r{dst}, %r{scratch}}}, %rd5;").unwrap();
                } else {
                    if op.selector_is_f32() {
                        self.emit_raw_f32_operand(s, 0, c, true);
                    }
                    write!(s, "    {} {:<8} %r{dst}, ", pred_guard(pred), op.mnemonic()).unwrap();
                    a.emit(s);
                    write!(s, ", ").unwrap();
                    b.emit(s);
                    write!(s, ", ").unwrap();
                    if op.selector_is_f32() {
                        writeln!(s, "%f0;").unwrap();
                    } else {
                        c.emit(s);
                        writeln!(s, ";").unwrap();
                    }
                }
            }
            Inst::Dp4a { op, dst, a, b, c } => {
                write!(s, "    {:<13} %r{dst}, ", op.mnemonic()).unwrap();
                a.emit(s);
                write!(s, ", ").unwrap();
                b.emit(s);
                write!(s, ", ").unwrap();
                c.emit(s);
                writeln!(s, ";").unwrap();
            }
            Inst::PredicatedDp4a {
                op,
                dst,
                a,
                b,
                c,
                cmp,
                ca,
                cb,
                pred,
            } => {
                self.emit_inst_predicate_setup(s, cmp, ca, cb, pred);
                write!(s, "    {} {:<8} %r{dst}, ", pred_guard(pred), op.mnemonic()).unwrap();
                a.emit(s);
                write!(s, ", ").unwrap();
                b.emit(s);
                write!(s, ", ").unwrap();
                c.emit(s);
                writeln!(s, ";").unwrap();
            }
            Inst::Dp2a { op, dst, a, b, c } => {
                write!(s, "    {:<17} %r{dst}, ", op.mnemonic()).unwrap();
                a.emit(s);
                write!(s, ", ").unwrap();
                b.emit(s);
                write!(s, ", ").unwrap();
                c.emit(s);
                writeln!(s, ";").unwrap();
            }
            Inst::PredicatedDp2a {
                op,
                dst,
                a,
                b,
                c,
                cmp,
                ca,
                cb,
                pred,
            } => {
                self.emit_inst_predicate_setup(s, cmp, ca, cb, pred);
                write!(
                    s,
                    "    {} {:<12} %r{dst}, ",
                    pred_guard(pred),
                    op.mnemonic()
                )
                .unwrap();
                a.emit(s);
                write!(s, ", ").unwrap();
                b.emit(s);
                write!(s, ", ").unwrap();
                c.emit(s);
                writeln!(s, ";").unwrap();
            }
            Inst::Video { op, dst, a, b, c } => {
                write!(s, "    {:<24} %r{dst}, ", op.mnemonic()).unwrap();
                a.emit(s);
                write!(s, ", ").unwrap();
                b.emit(s);
                write!(s, ", ").unwrap();
                c.emit(s);
                writeln!(s, ";").unwrap();
            }
            Inst::PredicatedVideo {
                op,
                dst,
                a,
                b,
                c,
                cmp,
                ca,
                cb,
                pred,
            } => {
                self.emit_inst_predicate_setup(s, cmp, ca, cb, pred);
                write!(
                    s,
                    "    {} {:<19} %r{dst}, ",
                    pred_guard(pred),
                    op.mnemonic()
                )
                .unwrap();
                a.emit(s);
                write!(s, ", ").unwrap();
                b.emit(s);
                write!(s, ", ").unwrap();
                c.emit(s);
                writeln!(s, ";").unwrap();
            }
            Inst::Mad {
                signed,
                dst,
                a,
                b,
                c,
            } => {
                let mnemonic = if signed { "mad.lo.s32" } else { "mad.lo.u32" };
                write!(s, "    {mnemonic:<13} %r{dst}, ").unwrap();
                a.emit(s);
                write!(s, ", ").unwrap();
                b.emit(s);
                write!(s, ", ").unwrap();
                c.emit(s);
                writeln!(s, ";").unwrap();
            }
            Inst::PredicatedMad {
                signed,
                dst,
                a,
                b,
                c,
                cmp,
                ca,
                cb,
                pred,
            } => {
                let mnemonic = if signed { "mad.lo.s32" } else { "mad.lo.u32" };
                self.emit_inst_predicate_setup(s, cmp, ca, cb, pred);
                write!(s, "    {} {mnemonic:<8} %r{dst}, ", pred_guard(pred)).unwrap();
                a.emit(s);
                write!(s, ", ").unwrap();
                b.emit(s);
                write!(s, ", ").unwrap();
                c.emit(s);
                writeln!(s, ";").unwrap();
            }
            Inst::MadCarry {
                op,
                dst0,
                dst1,
                dst2,
                a,
                b,
                c,
                d,
                e,
                f,
                g,
                h,
                i,
            } => {
                let (first, second, third) = op.mnemonic_triple();
                write!(s, "    {first:<13} %r{dst0}, ").unwrap();
                a.emit(s);
                write!(s, ", ").unwrap();
                b.emit(s);
                write!(s, ", ").unwrap();
                c.emit(s);
                writeln!(s, ";").unwrap();
                write!(s, "    {second:<13} %r{dst1}, ").unwrap();
                d.emit(s);
                write!(s, ", ").unwrap();
                e.emit(s);
                write!(s, ", ").unwrap();
                f.emit(s);
                writeln!(s, ";").unwrap();
                write!(s, "    {third:<13} %r{dst2}, ").unwrap();
                g.emit(s);
                write!(s, ", ").unwrap();
                h.emit(s);
                write!(s, ", ").unwrap();
                i.emit(s);
                writeln!(s, ";").unwrap();
            }
            Inst::PredicatedMadCarry {
                op,
                dst0,
                dst1,
                dst2,
                a,
                b,
                c,
                d,
                e,
                f,
                g,
                h,
                i,
                cmp,
                ca,
                cb,
                pred,
            } => {
                let (first, second, third) = op.mnemonic_triple();
                self.emit_inst_predicate_setup(s, cmp, ca, cb, pred);
                write!(s, "    {} {first:<8} %r{dst0}, ", pred_guard(pred)).unwrap();
                a.emit(s);
                write!(s, ", ").unwrap();
                b.emit(s);
                write!(s, ", ").unwrap();
                c.emit(s);
                writeln!(s, ";").unwrap();
                write!(s, "    {} {second:<8} %r{dst1}, ", pred_guard(pred)).unwrap();
                d.emit(s);
                write!(s, ", ").unwrap();
                e.emit(s);
                write!(s, ", ").unwrap();
                f.emit(s);
                writeln!(s, ";").unwrap();
                write!(s, "    {} {third:<8} %r{dst2}, ", pred_guard(pred)).unwrap();
                g.emit(s);
                write!(s, ", ").unwrap();
                h.emit(s);
                write!(s, ", ").unwrap();
                i.emit(s);
                writeln!(s, ";").unwrap();
            }
            Inst::MadHi { op, dst, a, b, c } => {
                write!(s, "    {:<13} %r{dst}, ", op.mnemonic()).unwrap();
                a.emit(s);
                write!(s, ", ").unwrap();
                b.emit(s);
                write!(s, ", ").unwrap();
                c.emit(s);
                writeln!(s, ";").unwrap();
            }
            Inst::PredicatedMadHi {
                op,
                dst,
                a,
                b,
                c,
                cmp,
                ca,
                cb,
                pred,
            } => {
                self.emit_inst_predicate_setup(s, cmp, ca, cb, pred);
                write!(s, "    {} {:<8} %r{dst}, ", pred_guard(pred), op.mnemonic()).unwrap();
                a.emit(s);
                write!(s, ", ").unwrap();
                b.emit(s);
                write!(s, ", ").unwrap();
                c.emit(s);
                writeln!(s, ";").unwrap();
            }
            Inst::Lop3 { dst, a, b, c, imm } => {
                write!(s, "    lop3.b32      %r{dst}, ").unwrap();
                a.emit(s);
                write!(s, ", ").unwrap();
                b.emit(s);
                write!(s, ", ").unwrap();
                c.emit(s);
                writeln!(s, ", 0x{imm:02x};").unwrap();
            }
            Inst::PredicatedLop3 {
                dst,
                a,
                b,
                c,
                imm,
                cmp,
                ca,
                cb,
                pred,
            } => {
                self.emit_inst_predicate_setup(s, cmp, ca, cb, pred);
                write!(s, "    {} lop3.b32 %r{dst}, ", pred_guard(pred)).unwrap();
                a.emit(s);
                write!(s, ", ").unwrap();
                b.emit(s);
                write!(s, ", ").unwrap();
                c.emit(s);
                writeln!(s, ", 0x{imm:02x};").unwrap();
            }
            Inst::Prmt {
                mode,
                dst,
                a,
                b,
                ctrl,
            } => {
                let mnemonic = format!("prmt.b32{}", mode.map_or("", PrmtMode::suffix));
                let ctrl_text = match ctrl {
                    Operand::Reg(_) => {
                        let scratch = self.wide_scratch_hi_reg();
                        let mask = mode.map_or(0xFFFF, PrmtMode::ctrl_mask);
                        write!(s, "    and.b32       %r{scratch}, ").unwrap();
                        ctrl.emit(s);
                        writeln!(s, ", {mask};").unwrap();
                        format!("%r{scratch}")
                    }
                    Operand::Imm(v) => format!("0x{v:x}"),
                };
                write!(s, "    {mnemonic:<13} %r{dst}, ").unwrap();
                a.emit(s);
                write!(s, ", ").unwrap();
                b.emit(s);
                writeln!(s, ", {ctrl_text};").unwrap();
            }
            Inst::PredicatedPrmt {
                mode,
                dst,
                a,
                b,
                ctrl,
                cmp,
                ca,
                cb,
                pred,
            } => {
                let mnemonic = format!("prmt.b32{}", mode.map_or("", PrmtMode::suffix));
                let ctrl_text = match ctrl {
                    Operand::Reg(_) => {
                        let scratch = self.wide_scratch_hi_reg();
                        let mask = mode.map_or(0xFFFF, PrmtMode::ctrl_mask);
                        write!(s, "    and.b32       %r{scratch}, ").unwrap();
                        ctrl.emit(s);
                        writeln!(s, ", {mask};").unwrap();
                        format!("%r{scratch}")
                    }
                    Operand::Imm(v) => format!("0x{v:x}"),
                };
                self.emit_inst_predicate_setup(s, cmp, ca, cb, pred);
                write!(s, "    {} {mnemonic:<8} %r{dst}, ", pred_guard(pred)).unwrap();
                a.emit(s);
                write!(s, ", ").unwrap();
                b.emit(s);
                writeln!(s, ", {ctrl_text};").unwrap();
            }
            Inst::Funnel {
                dir,
                mode,
                dst,
                a,
                b,
                amount,
            } => {
                write!(s, "    {:<17} %r{dst}, ", dir.mnemonic(mode)).unwrap();
                a.emit(s);
                write!(s, ", ").unwrap();
                b.emit(s);
                writeln!(s, ", {amount};").unwrap();
            }
            Inst::RegFunnel {
                dir,
                mode,
                dst,
                a,
                b,
                amount,
            } => {
                write!(s, "    {:<17} %r{dst}, ", dir.mnemonic(mode)).unwrap();
                a.emit(s);
                write!(s, ", ").unwrap();
                b.emit(s);
                write!(s, ", ").unwrap();
                amount.emit(s);
                writeln!(s, ";").unwrap();
            }
            Inst::PredicatedFunnel {
                dir,
                mode,
                dst,
                a,
                b,
                amount,
                cmp,
                ca,
                cb,
                pred,
            } => {
                self.emit_inst_predicate_setup(s, cmp, ca, cb, pred);
                write!(
                    s,
                    "    {} {:<12} %r{dst}, ",
                    pred_guard(pred),
                    dir.mnemonic(mode)
                )
                .unwrap();
                a.emit(s);
                write!(s, ", ").unwrap();
                b.emit(s);
                write!(s, ", ").unwrap();
                amount.emit(s);
                writeln!(s, ";").unwrap();
            }
            Inst::Bfe {
                op,
                dst,
                src,
                pos,
                len,
            } => {
                write!(s, "    {:<13} %r{dst}, ", op.mnemonic()).unwrap();
                src.emit(s);
                write!(s, ", ").unwrap();
                pos.emit(s);
                write!(s, ", ").unwrap();
                len.emit(s);
                writeln!(s, ";").unwrap();
            }
            Inst::PredicatedBfe {
                op,
                dst,
                src,
                pos,
                len,
                cmp,
                ca,
                cb,
                pred,
            } => {
                self.emit_inst_predicate_setup(s, cmp, ca, cb, pred);
                write!(s, "    {} {:<8} %r{dst}, ", pred_guard(pred), op.mnemonic()).unwrap();
                src.emit(s);
                write!(s, ", ").unwrap();
                pos.emit(s);
                write!(s, ", ").unwrap();
                len.emit(s);
                writeln!(s, ";").unwrap();
            }
            Inst::WideBfe {
                op,
                dst,
                src,
                pos,
                len,
            } => {
                let scratch_hi = self.wide_scratch_hi_reg();
                write!(s, "    {:<13} %rd6, ", op.cvt_mnemonic()).unwrap();
                src.emit(s);
                writeln!(s, ";").unwrap();
                writeln!(s, "    {:<13} %rd6, %rd6, {pos}, {len};", op.mnemonic()).unwrap();
                writeln!(s, "    mov.b64       {{%r{dst}, %r{scratch_hi}}}, %rd6;").unwrap();
            }
            Inst::RegWideBfe {
                op,
                dst,
                src,
                param,
                slot,
                imm,
            } => {
                let scratch_hi = self.wide_scratch_hi_reg();
                write!(s, "    and.b32       %r{scratch_hi}, ").unwrap();
                param.emit(s);
                writeln!(s, ", 63;").unwrap();
                write!(s, "    {:<13} %rd6, ", op.cvt_mnemonic()).unwrap();
                src.emit(s);
                writeln!(s, ";").unwrap();
                match slot {
                    BitfieldParamSlot::Pos => {
                        writeln!(
                            s,
                            "    {:<13} %rd6, %rd6, %r{scratch_hi}, {imm};",
                            op.mnemonic()
                        )
                        .unwrap();
                    }
                    BitfieldParamSlot::Len => {
                        writeln!(
                            s,
                            "    {:<13} %rd6, %rd6, {imm}, %r{scratch_hi};",
                            op.mnemonic()
                        )
                        .unwrap();
                    }
                }
                writeln!(s, "    mov.b64       {{%r{dst}, %r{scratch_hi}}}, %rd6;").unwrap();
            }
            Inst::PredicatedWideBfe {
                op,
                dst,
                src,
                pos,
                len,
                cmp,
                ca,
                cb,
                pred,
            } => {
                let scratch_hi = self.wide_scratch_hi_reg();
                self.emit_inst_predicate_setup(s, cmp, ca, cb, pred);
                write!(s, "    {:<13} %rd6, ", op.cvt_mnemonic()).unwrap();
                src.emit(s);
                writeln!(s, ";").unwrap();
                writeln!(
                    s,
                    "    {} {:<8} %rd6, %rd6, {pos}, {len};",
                    pred_guard(pred),
                    op.mnemonic()
                )
                .unwrap();
                writeln!(
                    s,
                    "    {} mov.b64 {{%r{dst}, %r{scratch_hi}}}, %rd6;",
                    pred_guard(pred)
                )
                .unwrap();
            }
            Inst::PredicatedRegWideBfe {
                op,
                dst,
                src,
                param,
                slot,
                imm,
                cmp,
                ca,
                cb,
                pred,
            } => {
                let scratch_hi = self.wide_scratch_hi_reg();
                self.emit_inst_predicate_setup(s, cmp, ca, cb, pred);
                write!(s, "    and.b32       %r{scratch_hi}, ").unwrap();
                param.emit(s);
                writeln!(s, ", 63;").unwrap();
                write!(s, "    {:<13} %rd6, ", op.cvt_mnemonic()).unwrap();
                src.emit(s);
                writeln!(s, ";").unwrap();
                match slot {
                    BitfieldParamSlot::Pos => {
                        writeln!(
                            s,
                            "    {} {:<8} %rd6, %rd6, %r{scratch_hi}, {imm};",
                            pred_guard(pred),
                            op.mnemonic()
                        )
                        .unwrap();
                    }
                    BitfieldParamSlot::Len => {
                        writeln!(
                            s,
                            "    {} {:<8} %rd6, %rd6, {imm}, %r{scratch_hi};",
                            pred_guard(pred),
                            op.mnemonic()
                        )
                        .unwrap();
                    }
                }
                writeln!(
                    s,
                    "    {} mov.b64 {{%r{dst}, %r{scratch_hi}}}, %rd6;",
                    pred_guard(pred)
                )
                .unwrap();
            }
            Inst::Bfi {
                dst,
                src,
                base,
                pos,
                len,
            } => {
                write!(s, "    bfi.b32       %r{dst}, ").unwrap();
                src.emit(s);
                write!(s, ", ").unwrap();
                base.emit(s);
                write!(s, ", ").unwrap();
                pos.emit(s);
                write!(s, ", ").unwrap();
                len.emit(s);
                writeln!(s, ";").unwrap();
            }
            Inst::PredicatedBfi {
                dst,
                src,
                base,
                pos,
                len,
                cmp,
                ca,
                cb,
                pred,
            } => {
                self.emit_inst_predicate_setup(s, cmp, ca, cb, pred);
                write!(s, "    {} bfi.b32  %r{dst}, ", pred_guard(pred)).unwrap();
                src.emit(s);
                write!(s, ", ").unwrap();
                base.emit(s);
                write!(s, ", ").unwrap();
                pos.emit(s);
                write!(s, ", ").unwrap();
                len.emit(s);
                writeln!(s, ";").unwrap();
            }
            Inst::WideBfi {
                dst,
                src,
                base,
                pos,
                len,
            } => {
                let scratch_hi = self.wide_scratch_hi_reg();
                write!(s, "    cvt.u64.u32   %rd6, ").unwrap();
                src.emit(s);
                writeln!(s, ";").unwrap();
                write!(s, "    cvt.u64.u32   %rd7, ").unwrap();
                base.emit(s);
                writeln!(s, ";").unwrap();
                writeln!(s, "    bfi.b64       %rd6, %rd6, %rd7, {pos}, {len};").unwrap();
                writeln!(s, "    mov.b64       {{%r{dst}, %r{scratch_hi}}}, %rd6;").unwrap();
            }
            Inst::RegWideBfi {
                dst,
                src,
                base,
                param,
                slot,
                imm,
            } => {
                let scratch_hi = self.wide_scratch_hi_reg();
                write!(s, "    and.b32       %r{scratch_hi}, ").unwrap();
                param.emit(s);
                writeln!(s, ", 63;").unwrap();
                write!(s, "    cvt.u64.u32   %rd6, ").unwrap();
                src.emit(s);
                writeln!(s, ";").unwrap();
                write!(s, "    cvt.u64.u32   %rd7, ").unwrap();
                base.emit(s);
                writeln!(s, ";").unwrap();
                match slot {
                    BitfieldParamSlot::Pos => {
                        writeln!(
                            s,
                            "    bfi.b64       %rd6, %rd6, %rd7, %r{scratch_hi}, {imm};"
                        )
                        .unwrap();
                    }
                    BitfieldParamSlot::Len => {
                        writeln!(
                            s,
                            "    bfi.b64       %rd6, %rd6, %rd7, {imm}, %r{scratch_hi};"
                        )
                        .unwrap();
                    }
                }
                writeln!(s, "    mov.b64       {{%r{dst}, %r{scratch_hi}}}, %rd6;").unwrap();
            }
            Inst::PredicatedWideBfi {
                dst,
                src,
                base,
                pos,
                len,
                cmp,
                ca,
                cb,
                pred,
            } => {
                let scratch_hi = self.wide_scratch_hi_reg();
                self.emit_inst_predicate_setup(s, cmp, ca, cb, pred);
                write!(s, "    cvt.u64.u32   %rd6, ").unwrap();
                src.emit(s);
                writeln!(s, ";").unwrap();
                write!(s, "    cvt.u64.u32   %rd7, ").unwrap();
                base.emit(s);
                writeln!(s, ";").unwrap();
                writeln!(
                    s,
                    "    {} bfi.b64  %rd6, %rd6, %rd7, {pos}, {len};",
                    pred_guard(pred)
                )
                .unwrap();
                writeln!(
                    s,
                    "    {} mov.b64 {{%r{dst}, %r{scratch_hi}}}, %rd6;",
                    pred_guard(pred)
                )
                .unwrap();
            }
            Inst::PredicatedRegWideBfi {
                dst,
                src,
                base,
                param,
                slot,
                imm,
                cmp,
                ca,
                cb,
                pred,
            } => {
                let scratch_hi = self.wide_scratch_hi_reg();
                self.emit_inst_predicate_setup(s, cmp, ca, cb, pred);
                write!(s, "    and.b32       %r{scratch_hi}, ").unwrap();
                param.emit(s);
                writeln!(s, ", 63;").unwrap();
                write!(s, "    cvt.u64.u32   %rd6, ").unwrap();
                src.emit(s);
                writeln!(s, ";").unwrap();
                write!(s, "    cvt.u64.u32   %rd7, ").unwrap();
                base.emit(s);
                writeln!(s, ";").unwrap();
                match slot {
                    BitfieldParamSlot::Pos => {
                        writeln!(
                            s,
                            "    {} bfi.b64  %rd6, %rd6, %rd7, %r{scratch_hi}, {imm};",
                            pred_guard(pred)
                        )
                        .unwrap();
                    }
                    BitfieldParamSlot::Len => {
                        writeln!(
                            s,
                            "    {} bfi.b64  %rd6, %rd6, %rd7, {imm}, %r{scratch_hi};",
                            pred_guard(pred)
                        )
                        .unwrap();
                    }
                }
                writeln!(
                    s,
                    "    {} mov.b64 {{%r{dst}, %r{scratch_hi}}}, %rd6;",
                    pred_guard(pred)
                )
                .unwrap();
            }
            Inst::Bmsk {
                mode,
                dst,
                pos,
                len,
            } => {
                write!(s, "    {:<15} %r{dst}, ", mode.mnemonic()).unwrap();
                pos.emit(s);
                write!(s, ", ").unwrap();
                len.emit(s);
                writeln!(s, ";").unwrap();
            }
            Inst::PredicatedBmsk {
                mode,
                dst,
                pos,
                len,
                cmp,
                ca,
                cb,
                pred,
            } => {
                self.emit_inst_predicate_setup(s, cmp, ca, cb, pred);
                write!(
                    s,
                    "    {} {:<10} %r{dst}, ",
                    pred_guard(pred),
                    mode.mnemonic()
                )
                .unwrap();
                pos.emit(s);
                write!(s, ", ").unwrap();
                len.emit(s);
                writeln!(s, ";").unwrap();
            }
        }
    }

    fn emit_terminator(&self, s: &mut String, t: &Term) {
        match *t {
            Term::Branch(tgt) => {
                writeln!(s, "    bra             block_{tgt};").unwrap();
            }
            Term::CondBranch {
                cmp,
                a,
                b,
                pred,
                t: tt,
                f: ff,
            } => {
                write!(s, "    {:<13} %p{pred}, ", cmp.mnemonic()).unwrap();
                a.emit(s);
                write!(s, ", ").unwrap();
                b.emit(s);
                writeln!(s, ";").unwrap();
                writeln!(s, "    @%p{pred} bra   block_{tt};").unwrap();
                writeln!(s, "    bra             block_{ff};").unwrap();
            }
            Term::Loop {
                ctr,
                pred,
                back,
                fwd,
            } => {
                writeln!(s, "    setp.eq.u32   %p{pred}, %r{ctr}, 0;").unwrap();
                writeln!(s, "    @%p{pred} bra   loop_done_{ctr};").unwrap();
                writeln!(s, "    sub.u32         %r{ctr}, %r{ctr}, 1;").unwrap();
                writeln!(s, "    bra             block_{back};").unwrap();
                writeln!(s, "loop_done_{ctr}:").unwrap();
                writeln!(s, "    bra             block_{fwd};").unwrap();
            }
            Term::Return => {
                writeln!(s, "    bra             exit;").unwrap();
            }
        }
    }
}

fn pick_binop(
    u: &mut Unstructured,
    emit_minmax: bool,
    emit_sub: bool,
    emit_mul_lo: bool,
    emit_signed_lo_alu: bool,
    emit_sat_arith: bool,
    emit_mulhi: bool,
    emit_signed_mulhi: bool,
    emit_bitwise_binops: bool,
    emit_or: bool,
    emit_xor: bool,
) -> Result<BinOp> {
    let mut ops = vec![BinOp::Add];
    if emit_signed_lo_alu {
        ops.push(BinOp::AddS);
        if emit_sat_arith {
            ops.push(BinOp::AddSatS);
        }
    }
    if emit_mul_lo {
        ops.push(BinOp::Mul);
        if emit_signed_lo_alu {
            ops.push(BinOp::MulS);
        }
    }
    if emit_sub {
        ops.push(BinOp::Sub);
        if emit_signed_lo_alu {
            ops.push(BinOp::SubS);
            if emit_sat_arith {
                ops.push(BinOp::SubSatS);
            }
        }
    }
    if emit_bitwise_binops {
        ops.push(BinOp::And);
        if emit_or {
            ops.push(BinOp::Or);
        }
        if emit_xor {
            ops.push(BinOp::Xor);
        }
    }
    if emit_mulhi {
        ops.push(BinOp::MulHi);
        if emit_signed_mulhi {
            ops.push(BinOp::MulHiS);
        }
    }
    if emit_minmax {
        ops.extend([BinOp::Min, BinOp::Max, BinOp::MinS, BinOp::MaxS]);
    }
    Ok(*u.choose(&ops)?)
}

fn pick_packed_add(u: &mut Unstructured, emit_signed_packed_add: bool) -> Result<PackedAddOp> {
    let unsigned_ops = [PackedAddOp::U16x2];
    let all_ops = [PackedAddOp::U16x2, PackedAddOp::S16x2];
    let ops: &[PackedAddOp] = if emit_signed_packed_add {
        &all_ops
    } else {
        &unsigned_ops
    };
    Ok(*u.choose(ops)?)
}

fn pick_packed_minmax(
    u: &mut Unstructured,
    emit_signed_packed_minmax: bool,
) -> Result<PackedMinMaxOp> {
    let unsigned_ops = [PackedMinMaxOp::MinU16x2, PackedMinMaxOp::MaxU16x2];
    let all_ops = [
        PackedMinMaxOp::MinU16x2,
        PackedMinMaxOp::MaxU16x2,
        PackedMinMaxOp::MinS16x2,
        PackedMinMaxOp::MaxS16x2,
    ];
    let ops: &[PackedMinMaxOp] = if emit_signed_packed_minmax {
        &all_ops
    } else {
        &unsigned_ops
    };
    Ok(*u.choose(ops)?)
}

fn pick_scalar_16(
    u: &mut Unstructured,
    emit_signed_scalar_16bit: bool,
    emit_scalar_16bit_min: bool,
    emit_scalar_16bit_signed_unary: bool,
    emit_scalar_16bit_bitwise: bool,
    emit_scalar_16bit_shifts: bool,
) -> Result<Scalar16Op> {
    let mut ops = vec![
        Scalar16Op::AddU16,
        Scalar16Op::SubU16,
        Scalar16Op::MulLoU16,
        Scalar16Op::MulHiU16,
    ];
    if emit_scalar_16bit_min {
        ops.extend_from_slice(&[Scalar16Op::MinU16, Scalar16Op::MaxU16]);
    }
    if emit_scalar_16bit_bitwise {
        ops.extend_from_slice(&[
            Scalar16Op::AndB16,
            Scalar16Op::OrB16,
            Scalar16Op::XorB16,
            Scalar16Op::NotB16,
        ]);
    }
    if emit_scalar_16bit_shifts {
        ops.extend_from_slice(&[Scalar16Op::ShlB16, Scalar16Op::ShrU16]);
    }
    if emit_signed_scalar_16bit {
        ops.extend_from_slice(&[
            Scalar16Op::AddS16,
            Scalar16Op::SubS16,
            Scalar16Op::MulLoS16,
            Scalar16Op::MulHiS16,
        ]);
        if emit_scalar_16bit_min {
            ops.extend_from_slice(&[Scalar16Op::MinS16, Scalar16Op::MaxS16]);
        }
        if emit_scalar_16bit_signed_unary {
            ops.extend_from_slice(&[Scalar16Op::AbsS16, Scalar16Op::NegS16]);
        }
        if emit_scalar_16bit_shifts {
            ops.push(Scalar16Op::ShrS16);
        }
    }
    Ok(*u.choose(&ops)?)
}

fn pick_cmp(u: &mut Unstructured, emit_signed_cmp: bool) -> Result<CmpOp> {
    let ops_with_signed = [
        CmpOp::Eq,
        CmpOp::Ne,
        CmpOp::Lt,
        CmpOp::Le,
        CmpOp::Gt,
        CmpOp::Ge,
        CmpOp::LtS,
        CmpOp::LeS,
        CmpOp::GtS,
        CmpOp::GeS,
    ];
    let ops_without_signed = [
        CmpOp::Eq,
        CmpOp::Ne,
        CmpOp::Lt,
        CmpOp::Le,
        CmpOp::Gt,
        CmpOp::Ge,
    ];
    let ops: &[CmpOp] = if emit_signed_cmp {
        &ops_with_signed
    } else {
        &ops_without_signed
    };
    Ok(*u.choose(&ops)?)
}

fn pick_float_cmp(u: &mut Unstructured) -> Result<FloatCmpOp> {
    let ops = [
        FloatCmpOp::Eq,
        FloatCmpOp::Ne,
        FloatCmpOp::Lt,
        FloatCmpOp::Le,
        FloatCmpOp::Gt,
        FloatCmpOp::Ge,
        FloatCmpOp::Equ,
        FloatCmpOp::Neu,
        FloatCmpOp::Ltu,
        FloatCmpOp::Leu,
        FloatCmpOp::Gtu,
        FloatCmpOp::Geu,
        FloatCmpOp::Num,
        FloatCmpOp::Nan,
    ];
    Ok(*u.choose(&ops)?)
}

fn pick_f32_cmp(u: &mut Unstructured) -> Result<F32CmpOp> {
    Ok(F32CmpOp {
        cmp: pick_float_cmp(u)?,
        ftz: u.arbitrary()?,
    })
}

fn pick_narrow_cvt(u: &mut Unstructured, emit_signed_narrow_cvt: bool) -> Result<NarrowCvtOp> {
    let unsigned_ops = [NarrowCvtOp::U32ToU8, NarrowCvtOp::U32ToU16];
    let all_ops = [
        NarrowCvtOp::U32ToU8,
        NarrowCvtOp::U32ToU16,
        NarrowCvtOp::S32ToS8,
        NarrowCvtOp::S32ToS16,
    ];
    let ops: &[NarrowCvtOp] = if emit_signed_narrow_cvt {
        &all_ops
    } else {
        &unsigned_ops
    };
    Ok(*u.choose(ops)?)
}

fn pick_wide_cvt(u: &mut Unstructured, emit_signed_wide_cvt: bool) -> Result<WideCvtOp> {
    let unsigned_ops = [WideCvtOp::U64ToU32];
    let all_ops = [
        WideCvtOp::U64ToU32,
        WideCvtOp::S64ToS32,
        WideCvtOp::S64ToU32,
        WideCvtOp::U64ToS32,
    ];
    let ops: &[WideCvtOp] = if emit_signed_wide_cvt {
        &all_ops
    } else {
        &unsigned_ops
    };
    Ok(*u.choose(ops)?)
}

fn pick_predicate_bool_op(u: &mut Unstructured) -> Result<PredicateBoolOp> {
    let ops = [
        PredicateBoolOp::And,
        PredicateBoolOp::Or,
        PredicateBoolOp::Xor,
    ];
    Ok(*u.choose(&ops)?)
}

fn pick_predicate_logic_op(u: &mut Unstructured) -> Result<PredicateLogicOp> {
    let ops = [
        PredicateLogicOp::And,
        PredicateLogicOp::Or,
        PredicateLogicOp::Xor,
        PredicateLogicOp::Not,
    ];
    Ok(*u.choose(&ops)?)
}

fn pick_shift(
    u: &mut Unstructured,
    emit_shl: bool,
    emit_shr: bool,
    emit_signed_shr: bool,
) -> Result<ShiftOp> {
    let ops_all = [ShiftOp::Shl, ShiftOp::Shr, ShiftOp::ShrS];
    let ops_shl_shr = [ShiftOp::Shl, ShiftOp::Shr];
    let ops_shl_signed_shr = [ShiftOp::Shl, ShiftOp::ShrS];
    let ops_shr_signed_shr = [ShiftOp::Shr, ShiftOp::ShrS];
    let ops_shl_only = [ShiftOp::Shl];
    let ops_shr_only = [ShiftOp::Shr];
    let ops_signed_shr_only = [ShiftOp::ShrS];
    let ops: &[ShiftOp] = match (emit_shl, emit_shr, emit_signed_shr) {
        (true, true, true) => &ops_all,
        (true, true, false) => &ops_shl_shr,
        (true, false, true) => &ops_shl_signed_shr,
        (true, false, false) => &ops_shl_only,
        (false, true, true) => &ops_shr_signed_shr,
        (false, true, false) => &ops_shr_only,
        (false, false, true) => &ops_signed_shr_only,
        (false, false, false) => {
            unreachable!("shift generation requested with all shifts disabled")
        }
    };
    Ok(*u.choose(&ops)?)
}

fn pick_unary(
    u: &mut Unstructured,
    emit_not: bool,
    emit_clz: bool,
    emit_brev: bool,
    emit_neg: bool,
    emit_cnot: bool,
    emit_popc: bool,
    emit_abs: bool,
) -> Result<UnaryOp> {
    let ops_all = [
        UnaryOp::Not,
        UnaryOp::Cnot,
        UnaryOp::Popc,
        UnaryOp::Clz,
        UnaryOp::Brev,
        UnaryOp::AbsS,
        UnaryOp::NegS,
    ];
    let ops = ops_all
        .into_iter()
        .filter(|op| match op {
            UnaryOp::Not => emit_not,
            UnaryOp::NegS => emit_neg,
            UnaryOp::Cnot => emit_cnot,
            UnaryOp::AbsS => emit_abs,
            UnaryOp::Clz => emit_clz,
            UnaryOp::Brev => emit_brev,
            UnaryOp::Popc => emit_popc,
        })
        .collect::<Vec<_>>();
    Ok(*u.choose(&ops)?)
}

fn pick_selp(u: &mut Unstructured, emit_typed_selp: bool) -> Result<SelpOp> {
    const BASE_OPS: &[SelpOp] = &[SelpOp::B32];
    const TYPED_OPS: &[SelpOp] = &[SelpOp::U32, SelpOp::S32];

    let mut ops = Vec::with_capacity(3);
    ops.extend_from_slice(BASE_OPS);
    if emit_typed_selp {
        ops.extend_from_slice(TYPED_OPS);
    }
    Ok(*u.choose(&ops)?)
}

fn pick_selp64(u: &mut Unstructured) -> Result<Selp64Op> {
    let ops = [Selp64Op::B64, Selp64Op::U64, Selp64Op::S64];
    Ok(*u.choose(&ops)?)
}

fn pick_special_reg(u: &mut Unstructured) -> Result<SpecialRegOp> {
    let ops = [
        SpecialRegOp::TidX,
        SpecialRegOp::TidY,
        SpecialRegOp::TidZ,
        SpecialRegOp::NtidX,
        SpecialRegOp::NtidY,
        SpecialRegOp::NtidZ,
        SpecialRegOp::CtaidX,
        SpecialRegOp::CtaidY,
        SpecialRegOp::CtaidZ,
        SpecialRegOp::NctaidX,
        SpecialRegOp::NctaidY,
        SpecialRegOp::NctaidZ,
        SpecialRegOp::LaneId,
        SpecialRegOp::NWarpId,
        SpecialRegOp::LaneMaskEq,
        SpecialRegOp::LaneMaskLt,
        SpecialRegOp::LaneMaskLe,
        SpecialRegOp::LaneMaskGt,
        SpecialRegOp::LaneMaskGe,
    ];
    Ok(*u.choose(&ops)?)
}

fn pick_prmt_mode(u: &mut Unstructured, emit_prmt_modes: bool) -> Result<Option<PrmtMode>> {
    if !emit_prmt_modes || !u.arbitrary::<bool>()? {
        return Ok(None);
    }
    let ops = [
        PrmtMode::F4e,
        PrmtMode::B4e,
        PrmtMode::Rc8,
        PrmtMode::Ecl,
        PrmtMode::Ecr,
        PrmtMode::Rc16,
    ];
    Ok(Some(*u.choose(&ops)?))
}

fn pick_cvt(u: &mut Unstructured) -> Result<CvtOp> {
    let ops = [
        CvtOp::U8ToU32,
        CvtOp::U16ToU32,
        CvtOp::U8ToS32,
        CvtOp::U16ToS32,
        CvtOp::S8ToU32,
        CvtOp::S16ToU32,
        CvtOp::S8ToS32,
        CvtOp::S16ToS32,
    ];
    Ok(*u.choose(&ops)?)
}

fn pick_szext(u: &mut Unstructured, emit_signed: bool) -> Result<SzextOp> {
    let unsigned_ops = [SzextOp::WrapU32, SzextOp::ClampU32];
    let all_ops = [
        SzextOp::WrapU32,
        SzextOp::ClampU32,
        SzextOp::WrapS32,
        SzextOp::ClampS32,
    ];
    let ops: &[SzextOp] = if emit_signed { &all_ops } else { &unsigned_ops };
    Ok(*u.choose(&ops)?)
}

fn pick_fns_offset(u: &mut Unstructured) -> Result<i32> {
    u.int_in_range(-31..=31)
}

fn pick_bfind(
    u: &mut Unstructured,
    emit_signed_bfind: bool,
    emit_wide_bfind: bool,
    emit_signed_wide_bfind: bool,
) -> Result<BfindOp> {
    let u32_ops = [BfindOp::PositionU32, BfindOp::ShiftAmountU32];
    let all_32_ops = [
        BfindOp::PositionU32,
        BfindOp::ShiftAmountU32,
        BfindOp::PositionS32,
        BfindOp::ShiftAmountS32,
    ];
    let u32_u64_ops = [
        BfindOp::PositionU32,
        BfindOp::ShiftAmountU32,
        BfindOp::PositionU64,
        BfindOp::ShiftAmountU64,
    ];
    let all_32_u64_ops = [
        BfindOp::PositionU32,
        BfindOp::ShiftAmountU32,
        BfindOp::PositionS32,
        BfindOp::ShiftAmountS32,
        BfindOp::PositionU64,
        BfindOp::ShiftAmountU64,
    ];
    let u32_all_64_ops = [
        BfindOp::PositionU32,
        BfindOp::ShiftAmountU32,
        BfindOp::PositionU64,
        BfindOp::ShiftAmountU64,
        BfindOp::PositionS64,
        BfindOp::ShiftAmountS64,
    ];
    let all_ops = [
        BfindOp::PositionU32,
        BfindOp::ShiftAmountU32,
        BfindOp::PositionS32,
        BfindOp::ShiftAmountS32,
        BfindOp::PositionU64,
        BfindOp::ShiftAmountU64,
        BfindOp::PositionS64,
        BfindOp::ShiftAmountS64,
    ];
    let ops: &[BfindOp] = match (emit_signed_bfind, emit_wide_bfind, emit_signed_wide_bfind) {
        (false, false, _) => &u32_ops,
        (true, false, _) => &all_32_ops,
        (false, true, false) => &u32_u64_ops,
        (true, true, false) => &all_32_u64_ops,
        (false, true, true) => &u32_all_64_ops,
        (true, true, true) => &all_ops,
    };
    Ok(*u.choose(&ops)?)
}

fn pick_bfe(u: &mut Unstructured) -> Result<BfeOp> {
    let ops = [BfeOp::U32, BfeOp::S32];
    Ok(*u.choose(&ops)?)
}

fn pick_bmsk_mode(u: &mut Unstructured, emit_wrap: bool) -> Result<BmskMode> {
    if emit_wrap {
        let ops = [BmskMode::Clamp, BmskMode::Wrap];
        Ok(*u.choose(&ops)?)
    } else {
        Ok(BmskMode::Clamp)
    }
}

fn pick_wide_bfe(u: &mut Unstructured, emit_signed_wide_bfe: bool) -> Result<WideBfeOp> {
    let unsigned_ops = [WideBfeOp::U64];
    let all_ops = [WideBfeOp::U64, WideBfeOp::S64];
    let ops: &[WideBfeOp] = if emit_signed_wide_bfe {
        &all_ops
    } else {
        &unsigned_ops
    };
    Ok(*u.choose(ops)?)
}

fn pick_mad24(u: &mut Unstructured) -> Result<Mad24Op> {
    let ops = [
        Mad24Op::LoU32,
        Mad24Op::HiU32,
        Mad24Op::LoS32,
        Mad24Op::HiS32,
    ];
    Ok(*u.choose(&ops)?)
}

fn pick_mul24(u: &mut Unstructured) -> Result<Mul24Op> {
    let ops = [
        Mul24Op::LoU32,
        Mul24Op::HiU32,
        Mul24Op::LoS32,
        Mul24Op::HiS32,
    ];
    Ok(*u.choose(&ops)?)
}

fn pick_mul_wide(u: &mut Unstructured) -> Result<MulWideOp> {
    let ops = [MulWideOp::U32, MulWideOp::S32];
    Ok(*u.choose(&ops)?)
}

fn pick_funnel_mode(u: &mut Unstructured, emit_funnel_clamp: bool) -> Result<FunnelMode> {
    if emit_funnel_clamp && u.arbitrary::<bool>()? {
        Ok(FunnelMode::Clamp)
    } else {
        Ok(FunnelMode::Wrap)
    }
}

fn pick_mad_wide(u: &mut Unstructured, emit_signed_mad_wide: bool) -> Result<MadWideOp> {
    let unsigned_ops = [MadWideOp::U32];
    let all_ops = [MadWideOp::U32, MadWideOp::S32];
    let ops: &[MadWideOp] = if emit_signed_mad_wide {
        &all_ops
    } else {
        &unsigned_ops
    };
    Ok(*u.choose(ops)?)
}

fn pick_wide_int(
    u: &mut Unstructured,
    emit_wide_minmax: bool,
    emit_wide_mulhi: bool,
) -> Result<WideIntOp> {
    let mut ops = vec![
        WideIntOp::AddU64,
        WideIntOp::SubU64,
        WideIntOp::MulLoU64,
        WideIntOp::AddS64,
        WideIntOp::SubS64,
        WideIntOp::MulLoS64,
        WideIntOp::AndB64,
        WideIntOp::OrB64,
        WideIntOp::XorB64,
    ];
    if emit_wide_minmax {
        ops.extend([
            WideIntOp::MinU64,
            WideIntOp::MaxU64,
            WideIntOp::MinS64,
            WideIntOp::MaxS64,
        ]);
    }
    if emit_wide_mulhi {
        ops.extend([WideIntOp::MulHiU64, WideIntOp::MulHiS64]);
    }
    Ok(*u.choose(&ops)?)
}

fn pick_wide_mad64(u: &mut Unstructured, emit_signed_wide_mad64: bool) -> Result<WideMad64Op> {
    let unsigned_ops = [WideMad64Op::LoU64, WideMad64Op::HiU64];
    let all_ops = [
        WideMad64Op::LoU64,
        WideMad64Op::HiU64,
        WideMad64Op::LoS64,
        WideMad64Op::HiS64,
    ];
    let ops: &[WideMad64Op] = if emit_signed_wide_mad64 {
        &all_ops
    } else {
        &unsigned_ops
    };
    Ok(*u.choose(ops)?)
}

fn pick_wide_shift(u: &mut Unstructured) -> Result<WideShiftOp> {
    let ops = [
        WideShiftOp::ShlB64,
        WideShiftOp::ShrU64,
        WideShiftOp::ShrS64,
    ];
    Ok(*u.choose(&ops)?)
}

fn pick_wide_unary(u: &mut Unstructured, emit_signed_wide_unary: bool) -> Result<WideUnaryOp> {
    const BASE_OPS: &[WideUnaryOp] = &[
        WideUnaryOp::NotB64,
        WideUnaryOp::CnotB64,
        WideUnaryOp::PopcB64,
        WideUnaryOp::ClzB64,
        WideUnaryOp::BrevB64,
    ];
    const SIGNED_OPS: &[WideUnaryOp] = &[WideUnaryOp::NegS64, WideUnaryOp::AbsS64];

    let mut ops = Vec::with_capacity(7);
    ops.extend_from_slice(BASE_OPS);
    if emit_signed_wide_unary {
        ops.extend_from_slice(SIGNED_OPS);
    }
    Ok(*u.choose(&ops)?)
}

fn pick_wide_divrem(u: &mut Unstructured, emit_signed_wide_divrem: bool) -> Result<WideDivRemOp> {
    let unsigned_ops = [WideDivRemOp::DivU64, WideDivRemOp::RemU64];
    let all_ops = [
        WideDivRemOp::DivU64,
        WideDivRemOp::RemU64,
        WideDivRemOp::DivS64,
        WideDivRemOp::RemS64,
    ];
    let ops: &[WideDivRemOp] = if emit_signed_wide_divrem {
        &all_ops
    } else {
        &unsigned_ops
    };
    Ok(*u.choose(ops)?)
}

fn pick_add_carry(u: &mut Unstructured, emit_addc: bool, emit_subc: bool) -> Result<AddCarryOp> {
    let ops: &[AddCarryOp] = match (emit_addc, emit_subc) {
        (true, true) => &[AddCarryOp::Add, AddCarryOp::Sub],
        (true, false) => &[AddCarryOp::Add],
        (false, true) => &[AddCarryOp::Sub],
        (false, false) => unreachable!(),
    };
    Ok(*u.choose(ops)?)
}

fn pick_mad_carry(u: &mut Unstructured, emit_signed_mad_carry: bool) -> Result<MadCarryOp> {
    let unsigned_ops = [MadCarryOp::LoU32, MadCarryOp::HiU32];
    let all_ops = [
        MadCarryOp::LoU32,
        MadCarryOp::HiU32,
        MadCarryOp::LoS32,
        MadCarryOp::HiS32,
    ];
    let ops: &[MadCarryOp] = if emit_signed_mad_carry {
        &all_ops
    } else {
        &unsigned_ops
    };
    Ok(*u.choose(ops)?)
}

fn pick_subword_wide(
    u: &mut Unstructured,
    emit_signed_subword_wide: bool,
) -> Result<SubwordWideOp> {
    let unsigned_ops = [SubwordWideOp::MulU16, SubwordWideOp::MadU16];
    let all_ops = [
        SubwordWideOp::MulU16,
        SubwordWideOp::MulS16,
        SubwordWideOp::MadU16,
        SubwordWideOp::MadS16,
    ];
    let ops: &[SubwordWideOp] = if emit_signed_subword_wide {
        &all_ops
    } else {
        &unsigned_ops
    };
    Ok(*u.choose(ops)?)
}

fn pick_dp4a(u: &mut Unstructured) -> Result<Dp4aOp> {
    let ops = [
        Dp4aOp::U32U32,
        Dp4aOp::U32S32,
        Dp4aOp::S32U32,
        Dp4aOp::S32S32,
    ];
    Ok(*u.choose(&ops)?)
}

fn pick_dp2a(u: &mut Unstructured) -> Result<Dp2aOp> {
    let ops = [
        Dp2aOp::LoU32U32,
        Dp2aOp::HiU32U32,
        Dp2aOp::LoU32S32,
        Dp2aOp::HiU32S32,
        Dp2aOp::LoS32U32,
        Dp2aOp::HiS32U32,
        Dp2aOp::LoS32S32,
        Dp2aOp::HiS32S32,
    ];
    Ok(*u.choose(&ops)?)
}

fn pick_sad(u: &mut Unstructured) -> Result<SadOp> {
    let ops = [SadOp::U32, SadOp::S32];
    Ok(*u.choose(&ops)?)
}

fn pick_slct(
    u: &mut Unstructured,
    emit_s32_slct: bool,
    emit_f32_slct: bool,
    emit_wide_slct: bool,
    emit_f64_slct: bool,
) -> Result<SlctOp> {
    const BASE_OPS: &[SlctOp] = &[SlctOp::U32S32, SlctOp::B32S32];
    const S32_OPS: &[SlctOp] = &[SlctOp::S32S32];
    const F32_OPS: &[SlctOp] = &[
        SlctOp::U32F32,
        SlctOp::B32F32,
        SlctOp::F32S32,
        SlctOp::F32F32,
    ];
    const S32_F32_OPS: &[SlctOp] = &[SlctOp::S32F32];
    const WIDE_OPS: &[SlctOp] = &[SlctOp::U64S32, SlctOp::S64S32, SlctOp::B64S32];
    const WIDE_F32_OPS: &[SlctOp] = &[SlctOp::U64F32, SlctOp::S64F32, SlctOp::B64F32];
    const F64_OPS: &[SlctOp] = &[SlctOp::F64S32];
    const F64_F32_OPS: &[SlctOp] = &[SlctOp::F64F32];

    let mut ops = Vec::with_capacity(16);
    ops.extend_from_slice(BASE_OPS);
    if emit_s32_slct {
        ops.extend_from_slice(S32_OPS);
    }
    if emit_f32_slct {
        ops.extend_from_slice(F32_OPS);
        if emit_s32_slct {
            ops.extend_from_slice(S32_F32_OPS);
        }
    }
    if emit_wide_slct {
        ops.extend_from_slice(WIDE_OPS);
        if emit_f32_slct {
            ops.extend_from_slice(WIDE_F32_OPS);
        }
    }
    if emit_f64_slct {
        ops.extend_from_slice(F64_OPS);
        if emit_f32_slct {
            ops.extend_from_slice(F64_F32_OPS);
        }
    }

    Ok(*u.choose(&ops)?)
}

fn pick_video(
    u: &mut Unstructured,
    emit_vsub4: bool,
    emit_signed_video: bool,
    emit_video_sat: bool,
) -> Result<VideoOp> {
    const OPS_WITH_VSUB4: &[VideoKind] = &[
        VideoKind::Add2,
        VideoKind::Sub2,
        VideoKind::Avrg2,
        VideoKind::AbsDiff2,
        VideoKind::Min2,
        VideoKind::Max2,
        VideoKind::Add4,
        VideoKind::Sub4,
        VideoKind::Avrg4,
        VideoKind::AbsDiff4,
        VideoKind::Min4,
        VideoKind::Max4,
    ];
    const OPS_WITHOUT_VSUB4: &[VideoKind] = &[
        VideoKind::Add2,
        VideoKind::Sub2,
        VideoKind::Avrg2,
        VideoKind::AbsDiff2,
        VideoKind::Min2,
        VideoKind::Max2,
        VideoKind::Add4,
        VideoKind::Avrg4,
        VideoKind::AbsDiff4,
        VideoKind::Min4,
        VideoKind::Max4,
    ];
    const U32_TYPES: &[(VideoType, VideoType, VideoType)] =
        &[(VideoType::U32, VideoType::U32, VideoType::U32)];
    const ALL_TYPES: &[(VideoType, VideoType, VideoType)] = &[
        (VideoType::U32, VideoType::U32, VideoType::U32),
        (VideoType::U32, VideoType::U32, VideoType::S32),
        (VideoType::U32, VideoType::S32, VideoType::U32),
        (VideoType::U32, VideoType::S32, VideoType::S32),
        (VideoType::S32, VideoType::U32, VideoType::U32),
        (VideoType::S32, VideoType::U32, VideoType::S32),
        (VideoType::S32, VideoType::S32, VideoType::U32),
        (VideoType::S32, VideoType::S32, VideoType::S32),
    ];
    const BASE_MODES: &[VideoMode] = &[VideoMode::Plain, VideoMode::Add];
    const SAT_MODES: &[VideoMode] = &[VideoMode::Plain, VideoMode::Add, VideoMode::Sat];

    let kinds = if emit_vsub4 {
        OPS_WITH_VSUB4
    } else {
        OPS_WITHOUT_VSUB4
    };
    let (dst_type, a_type, b_type) = *u.choose(if emit_signed_video {
        ALL_TYPES
    } else {
        U32_TYPES
    })?;
    Ok(VideoOp {
        kind: *u.choose(kinds)?,
        dst_type,
        a_type,
        b_type,
        mode: *u.choose(if emit_video_sat {
            SAT_MODES
        } else {
            BASE_MODES
        })?,
    })
}

fn pick_divrem(u: &mut Unstructured, emit_signed_divrem: bool) -> Result<DivRemOp> {
    let ops_all = [
        DivRemOp::DivU,
        DivRemOp::RemU,
        DivRemOp::DivS,
        DivRemOp::RemS,
    ];
    let ops_unsigned = [DivRemOp::DivU, DivRemOp::RemU];
    let ops: &[DivRemOp] = if emit_signed_divrem {
        &ops_all
    } else {
        &ops_unsigned
    };
    Ok(*u.choose(&ops)?)
}

fn pick_unsigned_divrem(u: &mut Unstructured) -> Result<DivRemOp> {
    let ops = [DivRemOp::DivU, DivRemOp::RemU];
    Ok(*u.choose(&ops)?)
}

fn pick_mad_hi(u: &mut Unstructured, emit_signed_mad_hi: bool) -> Result<MadHiOp> {
    let ops_all = [MadHiOp::U32, MadHiOp::S32];
    let ops_unsigned = [MadHiOp::U32];
    let ops = if emit_signed_mad_hi {
        &ops_all[..]
    } else {
        &ops_unsigned[..]
    };
    Ok(*u.choose(ops)?)
}

/// Pick a u32 immediate with a weighted distribution that favors small values
/// (for arithmetic legibility) while still hitting the corner cases the
/// constant-folder cares about: 0, INT_MIN, INT_MAX, 0xFFFFFFFF, powers of
/// two. `max_small` caps the uniform-small bucket.
fn sanitize_imm32(v: u32, max_small: u32, emit_i32_boundary_immediates: bool) -> u32 {
    const SIGNED_BOUNDARY_LO: u32 = 0x7FFF_FF00;
    const SIGNED_BOUNDARY_HI: u32 = 0x8000_00FF;
    const SAFE_BELOW_SIGNED_BOUNDARY: u32 = SIGNED_BOUNDARY_LO - 1;

    if !emit_i32_boundary_immediates && (SIGNED_BOUNDARY_LO..=SIGNED_BOUNDARY_HI).contains(&v) {
        max_small.min(SAFE_BELOW_SIGNED_BOUNDARY)
    } else {
        v
    }
}

fn pick_imm32(
    u: &mut Unstructured,
    max_small: u32,
    emit_i32_boundary_immediates: bool,
) -> Result<u32> {
    let pick: u8 = u.arbitrary()?;
    if pick < 154 {
        // 60% small uniform
        Ok(sanitize_imm32(
            u.int_in_range(0..=max_small)?,
            max_small,
            emit_i32_boundary_immediates,
        ))
    } else if pick < 205 {
        // 20% power of two (1, 2, 4, ..., 0x80000000)
        let shift: u8 = u.int_in_range(0..=31)?;
        Ok(sanitize_imm32(
            1u32 << shift,
            max_small,
            emit_i32_boundary_immediates,
        ))
    } else if pick < 230 {
        // 10% specials: corner cases for constant folding & sign-extension
        let specials_with_boundaries = [
            0u32,
            1,
            0xFFFF_FFFF, // -1 / UINT_MAX
            0x8000_0000, // INT_MIN
            0x7FFF_FFFF, // INT_MAX
            0xAAAA_AAAA, // alternating bits
            0x5555_5555,
            0x0000_FFFF,
            0xFFFF_0000,
            0xFF00_FF00,
            0x0F0F_0F0F,
        ];
        let specials_without_boundaries = [
            0u32,
            1,
            0xFFFF_FFFF, // -1 / UINT_MAX
            0xAAAA_AAAA, // alternating bits
            0x5555_5555,
            0x0000_FFFF,
            0xFFFF_0000,
            0xFF00_FF00,
            0x0F0F_0F0F,
        ];
        if emit_i32_boundary_immediates {
            Ok(*u.choose(&specials_with_boundaries)?)
        } else {
            Ok(*u.choose(&specials_without_boundaries)?)
        }
    } else {
        // 10% arbitrary
        Ok(sanitize_imm32(
            u.arbitrary()?,
            max_small,
            emit_i32_boundary_immediates,
        ))
    }
}

fn pick_nonzero_imm32(
    u: &mut Unstructured,
    max_small: u32,
    emit_i32_boundary_immediates: bool,
) -> Result<u32> {
    Ok(pick_imm32(u, max_small, emit_i32_boundary_immediates)?.max(1))
}

fn pick_signed_divisor_imm32(u: &mut Unstructured, max_small: u32) -> Result<u32> {
    let upper = max_small.max(2);
    u.int_in_range(2..=upper)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    const BIN_MNEMONICS: &[&str] = &[
        "add.u32",
        "add.s32",
        "add.sat.s32",
        "sub.u32",
        "sub.s32",
        "sub.sat.s32",
        "mul.lo.u32",
        "mul.lo.s32",
        "mul.hi.u32",
        "mul.hi.s32",
        "and.b32",
        "or.b32",
        "xor.b32",
        "min.u32",
        "max.u32",
        "min.s32",
        "max.s32",
    ];
    const SIGNED_LO_BIN_MNEMONICS: &[&str] = &[
        "add.s32",
        "add.sat.s32",
        "sub.s32",
        "sub.sat.s32",
        "mul.lo.s32",
    ];
    const SAT_ARITH_MNEMONICS: &[&str] = &["add.sat.s32", "sub.sat.s32"];
    const PACKED_ADD_MNEMONICS: &[&str] = &["add.u16x2", "add.s16x2"];
    const SIGNED_PACKED_ADD_MNEMONICS: &[&str] = &["add.s16x2"];
    const PACKED_MINMAX_MNEMONICS: &[&str] = &["min.u16x2", "max.u16x2", "min.s16x2", "max.s16x2"];
    const SIGNED_PACKED_MINMAX_MNEMONICS: &[&str] = &["min.s16x2", "max.s16x2"];
    const SCALAR_16BIT_MNEMONICS: &[&str] = &[
        "add.u16",
        "sub.u16",
        "min.u16",
        "max.u16",
        "mul.lo.u16",
        "mul.hi.u16",
        "add.s16",
        "sub.s16",
        "min.s16",
        "max.s16",
        "mul.lo.s16",
        "mul.hi.s16",
        "abs.s16",
        "neg.s16",
        "and.b16",
        "or.b16",
        "xor.b16",
        "not.b16",
        "shl.b16",
        "shr.u16",
        "shr.s16",
    ];
    const SCALAR_16BIT_BITWISE_MNEMONICS: &[&str] = &["and.b16", "or.b16", "xor.b16", "not.b16"];
    const SCALAR_16BIT_SHIFT_MNEMONICS: &[&str] = &["shl.b16", "shr.u16", "shr.s16"];
    const SCALAR_16BIT_COMPARE_MNEMONICS: &[&str] = &[
        "setp.eq.u16",
        "setp.ne.u16",
        "setp.lt.u16",
        "setp.le.u16",
        "setp.gt.u16",
        "setp.ge.u16",
        "setp.lt.s16",
        "setp.le.s16",
        "setp.gt.s16",
        "setp.ge.s16",
        "set.eq.u32.u16",
        "set.ne.u32.u16",
        "set.lt.u32.u16",
        "set.le.u32.u16",
        "set.gt.u32.u16",
        "set.ge.u32.u16",
        "set.lt.u32.s16",
        "set.le.u32.s16",
        "set.gt.u32.s16",
        "set.ge.u32.s16",
    ];
    const SCALAR_16BIT_SELP_MNEMONICS: &[&str] = &["selp.u16", "selp.s16"];
    const SCALAR_16BIT_POST_KNOWN_MNEMONICS: &[&str] = &[
        "add.u16",
        "sub.u16",
        "mul.lo.u16",
        "mul.hi.u16",
        "add.s16",
        "sub.s16",
        "mul.lo.s16",
        "mul.hi.s16",
        "and.b16",
        "or.b16",
        "xor.b16",
        "not.b16",
        "shl.b16",
        "shr.u16",
        "shr.s16",
        "setp.eq.u16",
        "setp.ne.u16",
        "setp.lt.u16",
        "setp.le.u16",
        "setp.gt.u16",
        "setp.ge.u16",
        "set.eq.u32.u16",
        "set.ne.u32.u16",
        "set.lt.u32.u16",
        "set.le.u32.u16",
        "set.gt.u32.u16",
        "set.ge.u32.u16",
        "selp.u16",
    ];
    const SIGNED_SCALAR_16BIT_MNEMONICS: &[&str] = &[
        "add.s16",
        "sub.s16",
        "min.s16",
        "max.s16",
        "mul.lo.s16",
        "mul.hi.s16",
        "abs.s16",
        "neg.s16",
        "shr.s16",
        "setp.lt.s16",
        "setp.le.s16",
        "setp.gt.s16",
        "setp.ge.s16",
        "set.lt.u32.s16",
        "set.le.u32.s16",
        "set.gt.u32.s16",
        "set.ge.u32.s16",
        "selp.s16",
    ];
    const MAD_LO_MNEMONICS: &[&str] = &["mad.lo.u32", "mad.lo.s32"];
    const MAD_HI_MNEMONICS: &[&str] = &["mad.hi.u32", "mad.hi.s32"];
    const POST_KNOWN_BIN_MNEMONICS: &[&str] = &["add.u32", "sub.u32", "and.b32"];
    const PRED_LOGIC_MNEMONICS: &[&str] = &["and.pred", "or.pred", "xor.pred", "not.pred"];
    const SHIFT_MNEMONICS: &[&str] = &["shl.b32", "shr.u32", "shr.s32"];
    const UNARY_MNEMONICS: &[&str] = &[
        "not.b32", "cnot.b32", "popc.b32", "clz.b32", "brev.b32", "abs.s32", "neg.s32",
    ];
    const POST_KNOWN_UNARY_MNEMONICS: &[&str] = &["popc.b32", "clz.b32"];
    const GLOBAL_LOAD_MNEMONICS: &[&str] = &[
        "ld.global.u8",
        "ld.global.s8",
        "ld.global.u16",
        "ld.global.s16",
        "ld.global.u32",
        "ld.global.u64",
        "ld.global.s64",
    ];
    const UNIFORM_GLOBAL_LOAD_MNEMONICS: &[&str] = &[
        "ldu.global.u8",
        "ldu.global.s8",
        "ldu.global.u16",
        "ldu.global.s16",
        "ldu.global.u32",
        "ldu.global.u64",
        "ldu.global.s64",
        "ldu.global.b8",
        "ldu.global.b16",
        "ldu.global.b32",
        "ldu.global.b64",
    ];
    const UNIFORM_GLOBAL_VECTOR_LOAD_MNEMONICS: &[&str] = &[
        "ldu.global.v2.u32",
        "ldu.global.v4.u32",
        "ldu.global.v2.u64",
        "ldu.global.v2.b32",
        "ldu.global.v4.b32",
        "ldu.global.v2.b64",
    ];
    const GLOBAL_LOAD_CACHE_PREFIXES: &[&str] = &[
        "ld.global.ca.",
        "ld.global.cg.",
        "ld.global.cs.",
        "ld.global.lu.",
        "ld.global.cv.",
        "ld.global.nc.",
    ];
    const GLOBAL_STORE_MNEMONICS: &[&str] = &[
        "st.global.u8",
        "st.global.u16",
        "st.global.u32",
        "st.global.u64",
    ];
    const GLOBAL_STORE_CACHE_PREFIXES: &[&str] = &[
        "st.global.wb.",
        "st.global.cg.",
        "st.global.cs.",
        "st.global.wt.",
    ];
    const CONST_LOAD_MNEMONICS: &[&str] = &[
        "ld.const.u8",
        "ld.const.s8",
        "ld.const.u16",
        "ld.const.s16",
        "ld.const.u32",
        "ld.const.u64",
        "ld.const.s64",
    ];
    const LOCAL_MEM_LOAD_MNEMONICS: &[&str] = &[
        "ld.local.u8",
        "ld.local.s8",
        "ld.local.u16",
        "ld.local.s16",
        "ld.local.u32",
        "ld.local.u64",
        "ld.local.s64",
    ];
    const LOCAL_MEM_STORE_MNEMONICS: &[&str] = &[
        "st.local.u8",
        "st.local.u16",
        "st.local.u32",
        "st.local.u64",
    ];
    const SHARED_MEM_LOAD_MNEMONICS: &[&str] = &[
        "ld.shared.u8",
        "ld.shared.s8",
        "ld.shared.u16",
        "ld.shared.s16",
        "ld.shared.u32",
        "ld.shared.u64",
        "ld.shared.s64",
    ];
    const SHARED_MEM_STORE_MNEMONICS: &[&str] = &[
        "st.shared.u8",
        "st.shared.u16",
        "st.shared.u32",
        "st.shared.u64",
    ];
    const VOLATILE_MEMORY_MNEMONICS: &[&str] = &[
        "ld.volatile.global.u8",
        "ld.volatile.global.s8",
        "ld.volatile.global.u16",
        "ld.volatile.global.s16",
        "ld.volatile.global.u32",
        "ld.volatile.global.u64",
        "ld.volatile.global.s64",
        "st.volatile.global.u8",
        "st.volatile.global.u16",
        "st.volatile.global.u32",
        "st.volatile.global.u64",
        "ld.volatile.shared.u8",
        "ld.volatile.shared.s8",
        "ld.volatile.shared.u16",
        "ld.volatile.shared.s16",
        "ld.volatile.shared.u32",
        "ld.volatile.shared.u64",
        "ld.volatile.shared.s64",
        "st.volatile.shared.u8",
        "st.volatile.shared.u16",
        "st.volatile.shared.u32",
        "st.volatile.shared.u64",
        "ld.volatile.global.v2.u32",
        "ld.volatile.global.v4.u32",
        "ld.volatile.global.v2.u64",
        "st.volatile.global.v2.u32",
        "st.volatile.global.v4.u32",
        "st.volatile.global.v2.u64",
        "ld.volatile.shared.v2.u32",
        "ld.volatile.shared.v4.u32",
        "ld.volatile.shared.v2.u64",
        "st.volatile.shared.v2.u32",
        "st.volatile.shared.v4.u32",
        "st.volatile.shared.v2.u64",
    ];
    const WIDE_MEMORY_MNEMONICS: &[&str] = &[
        "ld.global.u64",
        "ld.global.s64",
        "ld.global.b64",
        "st.global.u64",
        "st.global.b64",
        "ld.const.u64",
        "ld.const.s64",
        "ld.const.b64",
        "ld.local.u64",
        "ld.local.s64",
        "ld.local.b64",
        "st.local.u64",
        "st.local.b64",
        "ld.shared.u64",
        "ld.shared.s64",
        "ld.shared.b64",
        "st.shared.u64",
        "st.shared.b64",
        "ld.global.v2.u64",
        "ld.global.v2.b64",
        "st.global.v2.u64",
        "st.global.v2.b64",
        "ld.const.v2.u64",
        "ld.const.v2.b64",
        "ld.local.v2.u64",
        "ld.local.v2.b64",
        "st.local.v2.u64",
        "st.local.v2.b64",
        "ld.shared.v2.u64",
        "ld.shared.v2.b64",
        "st.shared.v2.u64",
        "st.shared.v2.b64",
        "ldu.global.b64",
        "ldu.global.v2.u64",
        "ldu.global.v2.b64",
    ];
    const VECTOR_MEMORY_MNEMONICS: &[&str] = &[
        "ld.global.v2.u32",
        "ld.global.v4.u32",
        "ld.global.v2.u64",
        "ldu.global.v2.u32",
        "ldu.global.v4.u32",
        "ldu.global.v2.u64",
        "st.global.v2.u32",
        "st.global.v4.u32",
        "st.global.v2.u64",
        "ld.const.v2.u32",
        "ld.const.v4.u32",
        "ld.const.v2.u64",
        "ld.local.v2.u32",
        "ld.local.v4.u32",
        "ld.local.v2.u64",
        "st.local.v2.u32",
        "st.local.v4.u32",
        "st.local.v2.u64",
        "ld.shared.v2.u32",
        "ld.shared.v4.u32",
        "ld.shared.v2.u64",
        "st.shared.v2.u32",
        "st.shared.v4.u32",
        "st.shared.v2.u64",
    ];
    const BIT_MEMORY_MNEMONICS: &[&str] = &[
        "ld.global.b8",
        "ld.global.b16",
        "ld.global.b32",
        "ld.global.b64",
        "st.global.b8",
        "st.global.b16",
        "st.global.b32",
        "st.global.b64",
        "ld.const.b8",
        "ld.const.b16",
        "ld.const.b32",
        "ld.const.b64",
        "ld.local.b8",
        "ld.local.b16",
        "ld.local.b32",
        "ld.local.b64",
        "st.local.b8",
        "st.local.b16",
        "st.local.b32",
        "st.local.b64",
        "ld.shared.b8",
        "ld.shared.b16",
        "ld.shared.b32",
        "ld.shared.b64",
        "st.shared.b8",
        "st.shared.b16",
        "st.shared.b32",
        "st.shared.b64",
        "ld.global.v2.b32",
        "ld.global.v4.b32",
        "ld.global.v2.b64",
        "ldu.global.b8",
        "ldu.global.b16",
        "ldu.global.b32",
        "ldu.global.b64",
        "ldu.global.v2.b32",
        "ldu.global.v4.b32",
        "ldu.global.v2.b64",
        "st.global.v2.b32",
        "st.global.v4.b32",
        "st.global.v2.b64",
        "ld.const.v2.b32",
        "ld.const.v4.b32",
        "ld.const.v2.b64",
        "ld.local.v2.b32",
        "ld.local.v4.b32",
        "ld.local.v2.b64",
        "st.local.v2.b32",
        "st.local.v4.b32",
        "st.local.v2.b64",
        "ld.shared.v2.b32",
        "ld.shared.v4.b32",
        "ld.shared.v2.b64",
        "st.shared.v2.b32",
        "st.shared.v4.b32",
        "st.shared.v2.b64",
    ];
    const F32_ARITH_MNEMONICS: &[&str] = &[
        "add.rn.f32",
        "sub.rn.f32",
        "mul.rn.f32",
        "div.rn.f32",
        "div.approx.ftz.f32",
        "fma.rn.f32",
        "add.rn.sat.f32",
        "sub.rn.sat.f32",
        "mul.rn.sat.f32",
        "fma.rn.sat.f32",
        "copysign.f32",
        "min.f32",
        "max.f32",
        "min.ftz.f32",
        "max.ftz.f32",
    ];
    const F32_ROUNDING_MNEMONICS: &[&str] = &[
        "add.rz.f32",
        "add.rm.f32",
        "add.rp.f32",
        "add.rn.ftz.f32",
        "add.rz.ftz.f32",
        "add.rm.ftz.f32",
        "add.rp.ftz.f32",
        "sub.rz.f32",
        "sub.rm.f32",
        "sub.rp.f32",
        "sub.rn.ftz.f32",
        "sub.rz.ftz.f32",
        "sub.rm.ftz.f32",
        "sub.rp.ftz.f32",
        "mul.rz.f32",
        "mul.rm.f32",
        "mul.rp.f32",
        "mul.rn.ftz.f32",
        "mul.rz.ftz.f32",
        "mul.rm.ftz.f32",
        "mul.rp.ftz.f32",
        "div.rz.f32",
        "div.rm.f32",
        "div.rp.f32",
        "div.rn.ftz.f32",
        "div.rz.ftz.f32",
        "div.rm.ftz.f32",
        "div.rp.ftz.f32",
        "fma.rz.f32",
        "fma.rm.f32",
        "fma.rp.f32",
        "fma.rn.ftz.f32",
        "fma.rz.ftz.f32",
        "fma.rm.ftz.f32",
        "fma.rp.ftz.f32",
    ];
    const F32_UNARY_MNEMONICS: &[&str] = &["abs.f32", "neg.f32", "abs.ftz.f32", "neg.ftz.f32"];
    const F32_CVT_MNEMONICS: &[&str] = &[
        "cvt.rn.f32.u32",
        "cvt.rz.f32.u32",
        "cvt.rm.f32.u32",
        "cvt.rp.f32.u32",
        "cvt.rn.ftz.f32.u32",
        "cvt.rz.ftz.f32.u32",
        "cvt.rm.ftz.f32.u32",
        "cvt.rp.ftz.f32.u32",
        "cvt.rn.f32.s32",
        "cvt.rz.f32.s32",
        "cvt.rm.f32.s32",
        "cvt.rp.f32.s32",
        "cvt.rn.ftz.f32.s32",
        "cvt.rz.ftz.f32.s32",
        "cvt.rm.ftz.f32.s32",
        "cvt.rp.ftz.f32.s32",
        "cvt.rn.f32.u64",
        "cvt.rz.f32.u64",
        "cvt.rm.f32.u64",
        "cvt.rp.f32.u64",
        "cvt.rn.ftz.f32.u64",
        "cvt.rz.ftz.f32.u64",
        "cvt.rm.ftz.f32.u64",
        "cvt.rp.ftz.f32.u64",
        "cvt.rn.f32.s64",
        "cvt.rz.f32.s64",
        "cvt.rm.f32.s64",
        "cvt.rp.f32.s64",
        "cvt.rn.ftz.f32.s64",
        "cvt.rz.ftz.f32.s64",
        "cvt.rm.ftz.f32.s64",
        "cvt.rp.ftz.f32.s64",
        "cvt.rzi.s32.f32",
        "cvt.rni.s32.f32",
        "cvt.rmi.s32.f32",
        "cvt.rpi.s32.f32",
        "cvt.rzi.ftz.s32.f32",
        "cvt.rni.ftz.s32.f32",
        "cvt.rmi.ftz.s32.f32",
        "cvt.rpi.ftz.s32.f32",
        "cvt.rzi.u32.f32",
        "cvt.rni.u32.f32",
        "cvt.rmi.u32.f32",
        "cvt.rpi.u32.f32",
        "cvt.rzi.ftz.u32.f32",
        "cvt.rni.ftz.u32.f32",
        "cvt.rmi.ftz.u32.f32",
        "cvt.rpi.ftz.u32.f32",
        "cvt.rzi.sat.s32.f32",
        "cvt.rni.sat.s32.f32",
        "cvt.rmi.sat.s32.f32",
        "cvt.rpi.sat.s32.f32",
        "cvt.rzi.ftz.sat.s32.f32",
        "cvt.rni.ftz.sat.s32.f32",
        "cvt.rmi.ftz.sat.s32.f32",
        "cvt.rpi.ftz.sat.s32.f32",
        "cvt.rzi.sat.u32.f32",
        "cvt.rni.sat.u32.f32",
        "cvt.rmi.sat.u32.f32",
        "cvt.rpi.sat.u32.f32",
        "cvt.rzi.ftz.sat.u32.f32",
        "cvt.rni.ftz.sat.u32.f32",
        "cvt.rmi.ftz.sat.u32.f32",
        "cvt.rpi.ftz.sat.u32.f32",
        "cvt.rzi.s64.f32",
        "cvt.rni.s64.f32",
        "cvt.rmi.s64.f32",
        "cvt.rpi.s64.f32",
        "cvt.rzi.ftz.s64.f32",
        "cvt.rni.ftz.s64.f32",
        "cvt.rmi.ftz.s64.f32",
        "cvt.rpi.ftz.s64.f32",
        "cvt.rzi.u64.f32",
        "cvt.rni.u64.f32",
        "cvt.rmi.u64.f32",
        "cvt.rpi.u64.f32",
        "cvt.rzi.ftz.u64.f32",
        "cvt.rni.ftz.u64.f32",
        "cvt.rmi.ftz.u64.f32",
        "cvt.rpi.ftz.u64.f32",
        "cvt.rzi.sat.s64.f32",
        "cvt.rni.sat.s64.f32",
        "cvt.rmi.sat.s64.f32",
        "cvt.rpi.sat.s64.f32",
        "cvt.rzi.ftz.sat.s64.f32",
        "cvt.rni.ftz.sat.s64.f32",
        "cvt.rmi.ftz.sat.s64.f32",
        "cvt.rpi.ftz.sat.s64.f32",
        "cvt.rzi.sat.u64.f32",
        "cvt.rni.sat.u64.f32",
        "cvt.rmi.sat.u64.f32",
        "cvt.rpi.sat.u64.f32",
        "cvt.rzi.ftz.sat.u64.f32",
        "cvt.rni.ftz.sat.u64.f32",
        "cvt.rmi.ftz.sat.u64.f32",
        "cvt.rpi.ftz.sat.u64.f32",
        "cvt.rn.f32.f64",
        "cvt.rz.f32.f64",
        "cvt.rm.f32.f64",
        "cvt.rp.f32.f64",
        "cvt.rn.ftz.f32.f64",
        "cvt.rz.ftz.f32.f64",
        "cvt.rm.ftz.f32.f64",
        "cvt.rp.ftz.f32.f64",
    ];
    const F32_CVT_DISABLE_MNEMONICS: &[&str] = &[
        "cvt.rz.f32.u32",
        "cvt.rm.f32.u32",
        "cvt.rp.f32.u32",
        "cvt.rn.ftz.f32.u32",
        "cvt.rz.ftz.f32.u32",
        "cvt.rm.ftz.f32.u32",
        "cvt.rp.ftz.f32.u32",
        "cvt.rn.f32.s32",
        "cvt.rz.f32.s32",
        "cvt.rm.f32.s32",
        "cvt.rp.f32.s32",
        "cvt.rn.ftz.f32.s32",
        "cvt.rz.ftz.f32.s32",
        "cvt.rm.ftz.f32.s32",
        "cvt.rp.ftz.f32.s32",
        "cvt.rn.f32.u64",
        "cvt.rz.f32.u64",
        "cvt.rm.f32.u64",
        "cvt.rp.f32.u64",
        "cvt.rn.ftz.f32.u64",
        "cvt.rz.ftz.f32.u64",
        "cvt.rm.ftz.f32.u64",
        "cvt.rp.ftz.f32.u64",
        "cvt.rn.f32.s64",
        "cvt.rz.f32.s64",
        "cvt.rm.f32.s64",
        "cvt.rp.f32.s64",
        "cvt.rn.ftz.f32.s64",
        "cvt.rz.ftz.f32.s64",
        "cvt.rm.ftz.f32.s64",
        "cvt.rp.ftz.f32.s64",
        "cvt.rni.s32.f32",
        "cvt.rmi.s32.f32",
        "cvt.rpi.s32.f32",
        "cvt.rzi.ftz.s32.f32",
        "cvt.rni.ftz.s32.f32",
        "cvt.rmi.ftz.s32.f32",
        "cvt.rpi.ftz.s32.f32",
        "cvt.rzi.u32.f32",
        "cvt.rni.u32.f32",
        "cvt.rmi.u32.f32",
        "cvt.rpi.u32.f32",
        "cvt.rzi.ftz.u32.f32",
        "cvt.rni.ftz.u32.f32",
        "cvt.rmi.ftz.u32.f32",
        "cvt.rpi.ftz.u32.f32",
        "cvt.rzi.sat.s32.f32",
        "cvt.rni.sat.s32.f32",
        "cvt.rmi.sat.s32.f32",
        "cvt.rpi.sat.s32.f32",
        "cvt.rzi.ftz.sat.s32.f32",
        "cvt.rni.ftz.sat.s32.f32",
        "cvt.rmi.ftz.sat.s32.f32",
        "cvt.rpi.ftz.sat.s32.f32",
        "cvt.rzi.sat.u32.f32",
        "cvt.rni.sat.u32.f32",
        "cvt.rmi.sat.u32.f32",
        "cvt.rpi.sat.u32.f32",
        "cvt.rzi.ftz.sat.u32.f32",
        "cvt.rni.ftz.sat.u32.f32",
        "cvt.rmi.ftz.sat.u32.f32",
        "cvt.rpi.ftz.sat.u32.f32",
        "cvt.rzi.s64.f32",
        "cvt.rni.s64.f32",
        "cvt.rmi.s64.f32",
        "cvt.rpi.s64.f32",
        "cvt.rzi.ftz.s64.f32",
        "cvt.rni.ftz.s64.f32",
        "cvt.rmi.ftz.s64.f32",
        "cvt.rpi.ftz.s64.f32",
        "cvt.rzi.u64.f32",
        "cvt.rni.u64.f32",
        "cvt.rmi.u64.f32",
        "cvt.rpi.u64.f32",
        "cvt.rzi.ftz.u64.f32",
        "cvt.rni.ftz.u64.f32",
        "cvt.rmi.ftz.u64.f32",
        "cvt.rpi.ftz.u64.f32",
        "cvt.rzi.sat.s64.f32",
        "cvt.rni.sat.s64.f32",
        "cvt.rmi.sat.s64.f32",
        "cvt.rpi.sat.s64.f32",
        "cvt.rzi.ftz.sat.s64.f32",
        "cvt.rni.ftz.sat.s64.f32",
        "cvt.rmi.ftz.sat.s64.f32",
        "cvt.rpi.ftz.sat.s64.f32",
        "cvt.rzi.sat.u64.f32",
        "cvt.rni.sat.u64.f32",
        "cvt.rmi.sat.u64.f32",
        "cvt.rpi.sat.u64.f32",
        "cvt.rzi.ftz.sat.u64.f32",
        "cvt.rni.ftz.sat.u64.f32",
        "cvt.rmi.ftz.sat.u64.f32",
        "cvt.rpi.ftz.sat.u64.f32",
        "cvt.rn.f32.f64",
        "cvt.rz.f32.f64",
        "cvt.rm.f32.f64",
        "cvt.rp.f32.f64",
        "cvt.rn.ftz.f32.f64",
        "cvt.rz.ftz.f32.f64",
        "cvt.rm.ftz.f32.f64",
        "cvt.rp.ftz.f32.f64",
    ];
    const F32_SPECIAL_MATH_MNEMONICS: &[&str] = &[
        "sqrt.rn.f32",
        "sqrt.rz.f32",
        "sqrt.rm.f32",
        "sqrt.rp.f32",
        "sqrt.rn.ftz.f32",
        "sqrt.rz.ftz.f32",
        "sqrt.rm.ftz.f32",
        "sqrt.rp.ftz.f32",
        "rcp.rn.f32",
        "rcp.rz.f32",
        "rcp.rm.f32",
        "rcp.rp.f32",
        "rcp.rn.ftz.f32",
        "rcp.rz.ftz.f32",
        "rcp.rm.ftz.f32",
        "rcp.rp.ftz.f32",
        "rcp.approx.ftz.f32",
        "rsqrt.approx.ftz.f32",
        "ex2.approx.ftz.f32",
        "lg2.approx.ftz.f32",
        "sin.approx.ftz.f32",
        "cos.approx.ftz.f32",
    ];
    const F32_COMPARE_MNEMONICS: &[&str] = &[
        "set.eq.u32.f32",
        "set.ne.u32.f32",
        "set.lt.u32.f32",
        "set.le.u32.f32",
        "set.gt.u32.f32",
        "set.ge.u32.f32",
        "set.equ.u32.f32",
        "set.neu.u32.f32",
        "set.ltu.u32.f32",
        "set.leu.u32.f32",
        "set.gtu.u32.f32",
        "set.geu.u32.f32",
        "set.num.u32.f32",
        "set.nan.u32.f32",
        "set.eq.ftz.u32.f32",
        "set.ne.ftz.u32.f32",
        "set.lt.ftz.u32.f32",
        "set.le.ftz.u32.f32",
        "set.gt.ftz.u32.f32",
        "set.ge.ftz.u32.f32",
        "set.equ.ftz.u32.f32",
        "set.neu.ftz.u32.f32",
        "set.ltu.ftz.u32.f32",
        "set.leu.ftz.u32.f32",
        "set.gtu.ftz.u32.f32",
        "set.geu.ftz.u32.f32",
        "set.num.ftz.u32.f32",
        "set.nan.ftz.u32.f32",
    ];
    const F32_SETP_MNEMONICS: &[&str] = &[
        "setp.eq.f32",
        "setp.ne.f32",
        "setp.lt.f32",
        "setp.le.f32",
        "setp.gt.f32",
        "setp.ge.f32",
        "setp.equ.f32",
        "setp.neu.f32",
        "setp.ltu.f32",
        "setp.leu.f32",
        "setp.gtu.f32",
        "setp.geu.f32",
        "setp.num.f32",
        "setp.nan.f32",
        "setp.eq.ftz.f32",
        "setp.ne.ftz.f32",
        "setp.lt.ftz.f32",
        "setp.le.ftz.f32",
        "setp.gt.ftz.f32",
        "setp.ge.ftz.f32",
        "setp.equ.ftz.f32",
        "setp.neu.ftz.f32",
        "setp.ltu.ftz.f32",
        "setp.leu.ftz.f32",
        "setp.gtu.ftz.f32",
        "setp.geu.ftz.f32",
        "setp.num.ftz.f32",
        "setp.nan.ftz.f32",
    ];
    const F32_SETP_BOOL_MNEMONICS: &[&str] = &[
        "setp.eq.and.f32",
        "setp.eq.or.f32",
        "setp.eq.xor.f32",
        "setp.ne.and.f32",
        "setp.ne.or.f32",
        "setp.ne.xor.f32",
        "setp.lt.and.f32",
        "setp.lt.or.f32",
        "setp.lt.xor.f32",
        "setp.le.and.f32",
        "setp.le.or.f32",
        "setp.le.xor.f32",
        "setp.gt.and.f32",
        "setp.gt.or.f32",
        "setp.gt.xor.f32",
        "setp.ge.and.f32",
        "setp.ge.or.f32",
        "setp.ge.xor.f32",
        "setp.equ.and.f32",
        "setp.equ.or.f32",
        "setp.equ.xor.f32",
        "setp.neu.and.f32",
        "setp.neu.or.f32",
        "setp.neu.xor.f32",
        "setp.ltu.and.f32",
        "setp.ltu.or.f32",
        "setp.ltu.xor.f32",
        "setp.leu.and.f32",
        "setp.leu.or.f32",
        "setp.leu.xor.f32",
        "setp.gtu.and.f32",
        "setp.gtu.or.f32",
        "setp.gtu.xor.f32",
        "setp.geu.and.f32",
        "setp.geu.or.f32",
        "setp.geu.xor.f32",
        "setp.num.and.f32",
        "setp.num.or.f32",
        "setp.num.xor.f32",
        "setp.nan.and.f32",
        "setp.nan.or.f32",
        "setp.nan.xor.f32",
        "setp.eq.ftz.and.f32",
        "setp.eq.ftz.or.f32",
        "setp.eq.ftz.xor.f32",
        "setp.ne.ftz.and.f32",
        "setp.ne.ftz.or.f32",
        "setp.ne.ftz.xor.f32",
        "setp.lt.ftz.and.f32",
        "setp.lt.ftz.or.f32",
        "setp.lt.ftz.xor.f32",
        "setp.le.ftz.and.f32",
        "setp.le.ftz.or.f32",
        "setp.le.ftz.xor.f32",
        "setp.gt.ftz.and.f32",
        "setp.gt.ftz.or.f32",
        "setp.gt.ftz.xor.f32",
        "setp.ge.ftz.and.f32",
        "setp.ge.ftz.or.f32",
        "setp.ge.ftz.xor.f32",
        "setp.equ.ftz.and.f32",
        "setp.equ.ftz.or.f32",
        "setp.equ.ftz.xor.f32",
        "setp.neu.ftz.and.f32",
        "setp.neu.ftz.or.f32",
        "setp.neu.ftz.xor.f32",
        "setp.ltu.ftz.and.f32",
        "setp.ltu.ftz.or.f32",
        "setp.ltu.ftz.xor.f32",
        "setp.leu.ftz.and.f32",
        "setp.leu.ftz.or.f32",
        "setp.leu.ftz.xor.f32",
        "setp.gtu.ftz.and.f32",
        "setp.gtu.ftz.or.f32",
        "setp.gtu.ftz.xor.f32",
        "setp.geu.ftz.and.f32",
        "setp.geu.ftz.or.f32",
        "setp.geu.ftz.xor.f32",
        "setp.num.ftz.and.f32",
        "setp.num.ftz.or.f32",
        "setp.num.ftz.xor.f32",
        "setp.nan.ftz.and.f32",
        "setp.nan.ftz.or.f32",
        "setp.nan.ftz.xor.f32",
    ];
    const F32_TESTP_MNEMONICS: &[&str] = &[
        "testp.finite.f32",
        "testp.infinite.f32",
        "testp.number.f32",
        "testp.notanumber.f32",
        "testp.normal.f32",
        "testp.subnormal.f32",
    ];
    const F32_SELP_MNEMONICS: &[&str] = &["selp.f32"];
    const F64_ARITH_MNEMONICS: &[&str] = &[
        "add.rn.f64",
        "sub.rn.f64",
        "mul.rn.f64",
        "div.rn.f64",
        "fma.rn.f64",
        "copysign.f64",
        "min.f64",
        "max.f64",
    ];
    const F64_ROUNDING_MNEMONICS: &[&str] = &[
        "add.rz.f64",
        "add.rm.f64",
        "add.rp.f64",
        "sub.rz.f64",
        "sub.rm.f64",
        "sub.rp.f64",
        "mul.rz.f64",
        "mul.rm.f64",
        "mul.rp.f64",
        "div.rz.f64",
        "div.rm.f64",
        "div.rp.f64",
        "fma.rz.f64",
        "fma.rm.f64",
        "fma.rp.f64",
    ];
    const F64_UNARY_MNEMONICS: &[&str] = &["abs.f64", "neg.f64"];
    const F64_CVT_MNEMONICS: &[&str] = &[
        "cvt.rn.f64.u32",
        "cvt.rz.f64.u32",
        "cvt.rm.f64.u32",
        "cvt.rp.f64.u32",
        "cvt.rn.f64.s32",
        "cvt.rz.f64.s32",
        "cvt.rm.f64.s32",
        "cvt.rp.f64.s32",
        "cvt.rn.f64.u64",
        "cvt.rz.f64.u64",
        "cvt.rm.f64.u64",
        "cvt.rp.f64.u64",
        "cvt.rn.f64.s64",
        "cvt.rz.f64.s64",
        "cvt.rm.f64.s64",
        "cvt.rp.f64.s64",
        "cvt.rzi.s32.f64",
        "cvt.rni.s32.f64",
        "cvt.rmi.s32.f64",
        "cvt.rpi.s32.f64",
        "cvt.rzi.u32.f64",
        "cvt.rni.u32.f64",
        "cvt.rmi.u32.f64",
        "cvt.rpi.u32.f64",
        "cvt.rzi.sat.s32.f64",
        "cvt.rni.sat.s32.f64",
        "cvt.rmi.sat.s32.f64",
        "cvt.rpi.sat.s32.f64",
        "cvt.rzi.sat.u32.f64",
        "cvt.rni.sat.u32.f64",
        "cvt.rmi.sat.u32.f64",
        "cvt.rpi.sat.u32.f64",
        "cvt.rzi.s64.f64",
        "cvt.rni.s64.f64",
        "cvt.rmi.s64.f64",
        "cvt.rpi.s64.f64",
        "cvt.rzi.u64.f64",
        "cvt.rni.u64.f64",
        "cvt.rmi.u64.f64",
        "cvt.rpi.u64.f64",
        "cvt.rzi.sat.s64.f64",
        "cvt.rni.sat.s64.f64",
        "cvt.rmi.sat.s64.f64",
        "cvt.rpi.sat.s64.f64",
        "cvt.rzi.sat.u64.f64",
        "cvt.rni.sat.u64.f64",
        "cvt.rmi.sat.u64.f64",
        "cvt.rpi.sat.u64.f64",
        "cvt.f64.f32",
    ];
    const F64_CVT_DISABLE_MNEMONICS: &[&str] = &[
        "cvt.rz.f64.u32",
        "cvt.rm.f64.u32",
        "cvt.rp.f64.u32",
        "cvt.rn.f64.s32",
        "cvt.rz.f64.s32",
        "cvt.rm.f64.s32",
        "cvt.rp.f64.s32",
        "cvt.rn.f64.u64",
        "cvt.rz.f64.u64",
        "cvt.rm.f64.u64",
        "cvt.rp.f64.u64",
        "cvt.rn.f64.s64",
        "cvt.rz.f64.s64",
        "cvt.rm.f64.s64",
        "cvt.rp.f64.s64",
        "cvt.rni.s32.f64",
        "cvt.rmi.s32.f64",
        "cvt.rpi.s32.f64",
        "cvt.rzi.u32.f64",
        "cvt.rni.u32.f64",
        "cvt.rmi.u32.f64",
        "cvt.rpi.u32.f64",
        "cvt.rzi.sat.s32.f64",
        "cvt.rni.sat.s32.f64",
        "cvt.rmi.sat.s32.f64",
        "cvt.rpi.sat.s32.f64",
        "cvt.rzi.sat.u32.f64",
        "cvt.rni.sat.u32.f64",
        "cvt.rmi.sat.u32.f64",
        "cvt.rpi.sat.u32.f64",
        "cvt.rzi.s64.f64",
        "cvt.rni.s64.f64",
        "cvt.rmi.s64.f64",
        "cvt.rpi.s64.f64",
        "cvt.rzi.u64.f64",
        "cvt.rni.u64.f64",
        "cvt.rmi.u64.f64",
        "cvt.rpi.u64.f64",
        "cvt.rzi.sat.s64.f64",
        "cvt.rni.sat.s64.f64",
        "cvt.rmi.sat.s64.f64",
        "cvt.rpi.sat.s64.f64",
        "cvt.rzi.sat.u64.f64",
        "cvt.rni.sat.u64.f64",
        "cvt.rmi.sat.u64.f64",
        "cvt.rpi.sat.u64.f64",
        "cvt.f64.f32",
    ];
    const F64_SPECIAL_MATH_MNEMONICS: &[&str] = &[
        "sqrt.rn.f64",
        "sqrt.rz.f64",
        "sqrt.rm.f64",
        "sqrt.rp.f64",
        "rcp.rn.f64",
        "rcp.rz.f64",
        "rcp.rm.f64",
        "rcp.rp.f64",
    ];
    const F64_COMPARE_MNEMONICS: &[&str] = &[
        "set.eq.u32.f64",
        "set.ne.u32.f64",
        "set.lt.u32.f64",
        "set.le.u32.f64",
        "set.gt.u32.f64",
        "set.ge.u32.f64",
        "set.equ.u32.f64",
        "set.neu.u32.f64",
        "set.ltu.u32.f64",
        "set.leu.u32.f64",
        "set.gtu.u32.f64",
        "set.geu.u32.f64",
        "set.num.u32.f64",
        "set.nan.u32.f64",
    ];
    const F64_SETP_MNEMONICS: &[&str] = &[
        "setp.eq.f64",
        "setp.ne.f64",
        "setp.lt.f64",
        "setp.le.f64",
        "setp.gt.f64",
        "setp.ge.f64",
        "setp.equ.f64",
        "setp.neu.f64",
        "setp.ltu.f64",
        "setp.leu.f64",
        "setp.gtu.f64",
        "setp.geu.f64",
        "setp.num.f64",
        "setp.nan.f64",
    ];
    const F64_SETP_BOOL_MNEMONICS: &[&str] = &[
        "setp.eq.and.f64",
        "setp.eq.or.f64",
        "setp.eq.xor.f64",
        "setp.ne.and.f64",
        "setp.ne.or.f64",
        "setp.ne.xor.f64",
        "setp.lt.and.f64",
        "setp.lt.or.f64",
        "setp.lt.xor.f64",
        "setp.le.and.f64",
        "setp.le.or.f64",
        "setp.le.xor.f64",
        "setp.gt.and.f64",
        "setp.gt.or.f64",
        "setp.gt.xor.f64",
        "setp.ge.and.f64",
        "setp.ge.or.f64",
        "setp.ge.xor.f64",
        "setp.equ.and.f64",
        "setp.equ.or.f64",
        "setp.equ.xor.f64",
        "setp.neu.and.f64",
        "setp.neu.or.f64",
        "setp.neu.xor.f64",
        "setp.ltu.and.f64",
        "setp.ltu.or.f64",
        "setp.ltu.xor.f64",
        "setp.leu.and.f64",
        "setp.leu.or.f64",
        "setp.leu.xor.f64",
        "setp.gtu.and.f64",
        "setp.gtu.or.f64",
        "setp.gtu.xor.f64",
        "setp.geu.and.f64",
        "setp.geu.or.f64",
        "setp.geu.xor.f64",
        "setp.num.and.f64",
        "setp.num.or.f64",
        "setp.num.xor.f64",
        "setp.nan.and.f64",
        "setp.nan.or.f64",
        "setp.nan.xor.f64",
    ];
    const F64_TESTP_MNEMONICS: &[&str] = &[
        "testp.finite.f64",
        "testp.infinite.f64",
        "testp.number.f64",
        "testp.notanumber.f64",
        "testp.normal.f64",
        "testp.subnormal.f64",
    ];
    const F64_SELP_MNEMONICS: &[&str] = &["selp.f64"];
    const SPECIAL_REG_NAMES: &[&str] = &[
        "%tid.x",
        "%tid.y",
        "%tid.z",
        "%ntid.x",
        "%ntid.y",
        "%ntid.z",
        "%ctaid.x",
        "%ctaid.y",
        "%ctaid.z",
        "%nctaid.x",
        "%nctaid.y",
        "%nctaid.z",
        "%laneid",
        "%nwarpid",
        "%lanemask_eq",
        "%lanemask_lt",
        "%lanemask_le",
        "%lanemask_gt",
        "%lanemask_ge",
    ];
    const CVT_MNEMONICS: &[&str] = &[
        "cvt.u32.u8",
        "cvt.u32.u16",
        "cvt.s32.u8",
        "cvt.s32.u16",
        "cvt.u32.s8",
        "cvt.u32.s16",
        "cvt.s32.s8",
        "cvt.s32.s16",
    ];
    const NARROW_CVT_MNEMONICS: &[&str] =
        &["cvt.u8.u32", "cvt.u16.u32", "cvt.s8.s32", "cvt.s16.s32"];
    const SIGNED_NARROW_CVT_MNEMONICS: &[&str] = &["cvt.s8.s32", "cvt.s16.s32"];
    const WIDE_CVT_MNEMONICS: &[&str] =
        &["cvt.u32.u64", "cvt.s32.s64", "cvt.u32.s64", "cvt.s32.u64"];
    const SIGNED_WIDE_CVT_MNEMONICS: &[&str] = &["cvt.s32.s64", "cvt.u32.s64", "cvt.s32.u64"];
    const SZEXT_MNEMONICS: &[&str] = &[
        "szext.wrap.u32",
        "szext.clamp.u32",
        "szext.wrap.s32",
        "szext.clamp.s32",
    ];
    const SIGNED_SZEXT_MNEMONICS: &[&str] = &["szext.wrap.s32", "szext.clamp.s32"];
    const FNS_MNEMONICS: &[&str] = &["fns.b32"];
    const PRMT_MODE_MNEMONICS: &[&str] = &[
        "prmt.b32.f4e",
        "prmt.b32.b4e",
        "prmt.b32.rc8",
        "prmt.b32.ecl",
        "prmt.b32.ecr",
        "prmt.b32.rc16",
    ];
    const BFIND_MNEMONICS: &[&str] = &[
        "bfind.u32",
        "bfind.shiftamt.u32",
        "bfind.s32",
        "bfind.shiftamt.s32",
        "bfind.u64",
        "bfind.shiftamt.u64",
        "bfind.s64",
        "bfind.shiftamt.s64",
    ];
    const SIGNED_BFIND_MNEMONICS: &[&str] = &["bfind.s32", "bfind.shiftamt.s32"];
    const WIDE_BFIND_MNEMONICS: &[&str] = &[
        "bfind.u64",
        "bfind.shiftamt.u64",
        "bfind.s64",
        "bfind.shiftamt.s64",
    ];
    const SIGNED_WIDE_BFIND_MNEMONICS: &[&str] = &["bfind.s64", "bfind.shiftamt.s64"];
    const BFE_MNEMONICS: &[&str] = &["bfe.u32", "bfe.s32"];
    const BMSK_MNEMONICS: &[&str] = &["bmsk.clamp.b32", "bmsk.wrap.b32"];
    const BITFIELD_MNEMONICS: &[&str] = &[
        "bfe.u32",
        "bfe.s32",
        "bfi.b32",
        "bmsk.clamp.b32",
        "bmsk.wrap.b32",
    ];
    const WIDE_BFE_MNEMONICS: &[&str] = &["bfe.u64", "bfe.s64"];
    const SIGNED_WIDE_BFE_MNEMONICS: &[&str] = &["bfe.s64"];
    const WIDE_BFI_MNEMONICS: &[&str] = &["bfi.b64"];
    const WIDE_BITFIELD_MNEMONICS: &[&str] = &["bfe.u64", "bfe.s64", "bfi.b64"];
    const DIVREM_MNEMONICS: &[&str] = &["div.u32", "rem.u32", "div.s32", "rem.s32"];
    const MAD24_MNEMONICS: &[&str] = &[
        "mad24.lo.u32",
        "mad24.hi.u32",
        "mad24.lo.s32",
        "mad24.hi.s32",
    ];
    const MUL24_MNEMONICS: &[&str] = &[
        "mul24.lo.u32",
        "mul24.hi.u32",
        "mul24.lo.s32",
        "mul24.hi.s32",
    ];
    const MUL_WIDE_MNEMONICS: &[&str] = &["mul.wide.u32", "mul.wide.s32"];
    const MAD_WIDE_MNEMONICS: &[&str] = &["mad.wide.u32", "mad.wide.s32"];
    const SIGNED_MAD_WIDE_MNEMONICS: &[&str] = &["mad.wide.s32"];
    const SUBWORD_WIDE_MNEMONICS: &[&str] = &[
        "mul.wide.u16",
        "mul.wide.s16",
        "mad.wide.u16",
        "mad.wide.s16",
    ];
    const SIGNED_SUBWORD_WIDE_MNEMONICS: &[&str] = &["mul.wide.s16", "mad.wide.s16"];
    const WIDE_INT_MNEMONICS: &[&str] = &[
        "add.u64",
        "sub.u64",
        "mul.lo.u64",
        "mul.hi.u64",
        "min.u64",
        "max.u64",
        "add.s64",
        "sub.s64",
        "mul.lo.s64",
        "mul.hi.s64",
        "min.s64",
        "max.s64",
        "and.b64",
        "or.b64",
        "xor.b64",
    ];
    const WIDE_MINMAX_MNEMONICS: &[&str] = &["min.u64", "max.u64", "min.s64", "max.s64"];
    const WIDE_MULHI_MNEMONICS: &[&str] = &["mul.hi.u64", "mul.hi.s64"];
    const WIDE_MAD64_MNEMONICS: &[&str] = &["mad.lo.u64", "mad.hi.u64", "mad.lo.s64", "mad.hi.s64"];
    const SIGNED_WIDE_MAD64_MNEMONICS: &[&str] = &["mad.lo.s64", "mad.hi.s64"];
    const WIDE_SET_MNEMONICS: &[&str] = &[
        "set.eq.u32.u64",
        "set.ne.u32.u64",
        "set.lt.u32.u64",
        "set.le.u32.u64",
        "set.gt.u32.u64",
        "set.ge.u32.u64",
        "set.lt.u32.s64",
        "set.le.u32.s64",
        "set.gt.u32.s64",
        "set.ge.u32.s64",
    ];
    const WIDE_SETP_MNEMONICS: &[&str] = &[
        "setp.eq.u64",
        "setp.ne.u64",
        "setp.lt.u64",
        "setp.le.u64",
        "setp.gt.u64",
        "setp.ge.u64",
        "setp.lt.s64",
        "setp.le.s64",
        "setp.gt.s64",
        "setp.ge.s64",
    ];
    const WIDE_SETP_BOOL_MNEMONICS: &[&str] = &[
        "setp.eq.and.u64",
        "setp.eq.or.u64",
        "setp.eq.xor.u64",
        "setp.ne.and.u64",
        "setp.ne.or.u64",
        "setp.ne.xor.u64",
        "setp.lt.and.u64",
        "setp.lt.or.u64",
        "setp.lt.xor.u64",
        "setp.le.and.u64",
        "setp.le.or.u64",
        "setp.le.xor.u64",
        "setp.gt.and.u64",
        "setp.gt.or.u64",
        "setp.gt.xor.u64",
        "setp.ge.and.u64",
        "setp.ge.or.u64",
        "setp.ge.xor.u64",
        "setp.lt.and.s64",
        "setp.lt.or.s64",
        "setp.lt.xor.s64",
        "setp.le.and.s64",
        "setp.le.or.s64",
        "setp.le.xor.s64",
        "setp.gt.and.s64",
        "setp.gt.or.s64",
        "setp.gt.xor.s64",
        "setp.ge.and.s64",
        "setp.ge.or.s64",
        "setp.ge.xor.s64",
    ];
    const WIDE_SELP_MNEMONICS: &[&str] = &["selp.b64", "selp.u64", "selp.s64"];
    const WIDE_UNARY_MNEMONICS: &[&str] = &[
        "not.b64", "cnot.b64", "popc.b64", "clz.b64", "brev.b64", "neg.s64", "abs.s64",
    ];
    const SIGNED_WIDE_UNARY_MNEMONICS: &[&str] = &["neg.s64", "abs.s64"];
    const WIDE_SHIFT_MNEMONICS: &[&str] = &["shl.b64", "shr.u64", "shr.s64"];
    const WIDE_DIVREM_MNEMONICS: &[&str] = &["div.u64", "rem.u64", "div.s64", "rem.s64"];
    const SIGNED_WIDE_DIVREM_MNEMONICS: &[&str] = &["div.s64", "rem.s64"];
    const CARRY_MNEMONICS: &[&str] = &["add.cc.u32", "addc.u32", "sub.cc.u32", "subc.u32"];
    const CARRY_CHAIN_CC_MNEMONICS: &[&str] = &["addc.cc.u32", "subc.cc.u32"];
    const WIDE_ADDC_MNEMONICS: &[&str] = &["add.cc.u64", "addc.u64"];
    const WIDE_SUBC_MNEMONICS: &[&str] = &["sub.cc.u64", "subc.u64"];
    const WIDE_CARRY_MNEMONICS: &[&str] = &["add.cc.u64", "addc.u64", "sub.cc.u64", "subc.u64"];
    const WIDE_CARRY_CHAIN_CC_MNEMONICS: &[&str] = &["addc.cc.u64", "subc.cc.u64"];
    const MAD_CARRY_MNEMONICS: &[&str] = &[
        "mad.lo.cc.u32",
        "mad.hi.cc.u32",
        "madc.lo.cc.u32",
        "madc.hi.cc.u32",
        "madc.lo.u32",
        "madc.hi.u32",
        "mad.lo.cc.s32",
        "mad.hi.cc.s32",
        "madc.lo.cc.s32",
        "madc.hi.cc.s32",
        "madc.lo.s32",
        "madc.hi.s32",
    ];
    const SIGNED_MAD_CARRY_MNEMONICS: &[&str] = &[
        "mad.lo.cc.s32",
        "mad.hi.cc.s32",
        "madc.lo.cc.s32",
        "madc.hi.cc.s32",
        "madc.lo.s32",
        "madc.hi.s32",
    ];
    const UNSIGNED_SETP_MNEMONICS: &[&str] = &[
        "setp.eq.u32",
        "setp.ne.u32",
        "setp.lt.u32",
        "setp.le.u32",
        "setp.gt.u32",
        "setp.ge.u32",
    ];
    const SIGNED_SETP_MNEMONICS: &[&str] =
        &["setp.lt.s32", "setp.le.s32", "setp.gt.s32", "setp.ge.s32"];
    const SET_MNEMONICS: &[&str] = &[
        "set.eq.u32.u32",
        "set.ne.u32.u32",
        "set.lt.u32.u32",
        "set.le.u32.u32",
        "set.gt.u32.u32",
        "set.ge.u32.u32",
        "set.lt.u32.s32",
        "set.le.u32.s32",
        "set.gt.u32.s32",
        "set.ge.u32.s32",
    ];
    const SELP_MNEMONICS: &[&str] = &["selp.b32", "selp.u32", "selp.s32"];
    const TYPED_SELP_MNEMONICS: &[&str] = &["selp.u32", "selp.s32"];
    const FUNNEL_CLAMP_MNEMONICS: &[&str] = &["shf.l.clamp.b32", "shf.r.clamp.b32"];
    const FUNNEL_MNEMONICS: &[&str] = &[
        "shf.l.wrap.b32",
        "shf.r.wrap.b32",
        "shf.l.clamp.b32",
        "shf.r.clamp.b32",
    ];
    const SAD_MNEMONICS: &[&str] = &["sad.u32", "sad.s32"];
    const SLCT_MNEMONICS: &[&str] = &["slct.u32.s32", "slct.s32.s32", "slct.b32.s32"];
    const F32_SLCT_MNEMONICS: &[&str] = &[
        "slct.u32.f32",
        "slct.s32.f32",
        "slct.b32.f32",
        "slct.f32.s32",
        "slct.f32.f32",
        "slct.u64.f32",
        "slct.s64.f32",
        "slct.b64.f32",
        "slct.f64.f32",
    ];
    const LEGACY_F32_SLCT_MNEMONICS: &[&str] = &[
        "slct.u32.f32",
        "slct.s32.f32",
        "slct.b32.f32",
        "slct.f32.s32",
        "slct.f32.f32",
    ];
    const WIDE_SLCT_MNEMONICS: &[&str] = &[
        "slct.u64.s32",
        "slct.s64.s32",
        "slct.b64.s32",
        "slct.u64.f32",
        "slct.s64.f32",
        "slct.b64.f32",
    ];
    const F64_SLCT_MNEMONICS: &[&str] = &["slct.f64.s32", "slct.f64.f32"];
    const POST_KNOWN_SLCT_MNEMONICS: &[&str] = &["slct.u32.s32", "slct.b32.s32"];
    const DP4A_MNEMONICS: &[&str] = &[
        "dp4a.u32.u32",
        "dp4a.u32.s32",
        "dp4a.s32.u32",
        "dp4a.s32.s32",
    ];
    const DP2A_MNEMONICS: &[&str] = &[
        "dp2a.lo.u32.u32",
        "dp2a.hi.u32.u32",
        "dp2a.lo.u32.s32",
        "dp2a.hi.u32.s32",
        "dp2a.lo.s32.u32",
        "dp2a.hi.s32.u32",
        "dp2a.lo.s32.s32",
        "dp2a.hi.s32.s32",
    ];
    const VIDEO_MNEMONICS: &[&str] = &[
        "vadd2.u32.u32.u32",
        "vsub2.u32.u32.u32",
        "vavrg2.u32.u32.u32",
        "vavrg2.u32.u32.u32.add",
        "vabsdiff2.u32.u32.u32.add",
        "vmin2.u32.u32.u32",
        "vmin2.u32.u32.u32.add",
        "vmax2.u32.u32.u32",
        "vmax2.u32.u32.u32.add",
        "vadd4.u32.u32.u32",
        "vsub4.u32.u32.u32",
        "vavrg4.u32.u32.u32",
        "vavrg4.u32.u32.u32.add",
        "vabsdiff4.u32.u32.u32.add",
        "vmin4.u32.u32.u32",
        "vmin4.u32.u32.u32.add",
        "vmax4.u32.u32.u32",
        "vmax4.u32.u32.u32.add",
    ];
    const POST_KNOWN_VIDEO_MNEMONICS: &[&str] = &[
        "vadd2.u32.u32.u32",
        "vsub2.u32.u32.u32",
        "vavrg2.u32.u32.u32",
        "vavrg2.u32.u32.u32.add",
        "vabsdiff2.u32.u32.u32.add",
        "vmin2.u32.u32.u32",
        "vmin2.u32.u32.u32.add",
        "vmax2.u32.u32.u32",
        "vmax2.u32.u32.u32.add",
        "vadd4.u32.u32.u32",
        "vavrg4.u32.u32.u32",
        "vavrg4.u32.u32.u32.add",
        "vabsdiff4.u32.u32.u32.add",
        "vmin4.u32.u32.u32",
        "vmin4.u32.u32.u32.add",
        "vmax4.u32.u32.u32",
        "vmax4.u32.u32.u32.add",
    ];

    fn default_profile_mnemonics() -> Vec<&'static str> {
        let mut mnemonics = Vec::new();
        for group in [
            BIN_MNEMONICS,
            PACKED_ADD_MNEMONICS,
            PACKED_MINMAX_MNEMONICS,
            SCALAR_16BIT_MNEMONICS,
            SCALAR_16BIT_COMPARE_MNEMONICS,
            SCALAR_16BIT_SELP_MNEMONICS,
            GLOBAL_LOAD_MNEMONICS,
            GLOBAL_STORE_MNEMONICS,
            CONST_LOAD_MNEMONICS,
            LOCAL_MEM_LOAD_MNEMONICS,
            LOCAL_MEM_STORE_MNEMONICS,
            SHARED_MEM_LOAD_MNEMONICS,
            SHARED_MEM_STORE_MNEMONICS,
            VECTOR_MEMORY_MNEMONICS,
            F32_ARITH_MNEMONICS,
            F32_ROUNDING_MNEMONICS,
            F32_UNARY_MNEMONICS,
            F32_CVT_MNEMONICS,
            F32_SPECIAL_MATH_MNEMONICS,
            F32_COMPARE_MNEMONICS,
            F32_SETP_MNEMONICS,
            F32_SETP_BOOL_MNEMONICS,
            F32_TESTP_MNEMONICS,
            F32_SELP_MNEMONICS,
            F64_ARITH_MNEMONICS,
            F64_ROUNDING_MNEMONICS,
            F64_UNARY_MNEMONICS,
            F64_CVT_MNEMONICS,
            F64_SPECIAL_MATH_MNEMONICS,
            F64_COMPARE_MNEMONICS,
            F64_SETP_MNEMONICS,
            F64_SETP_BOOL_MNEMONICS,
            F64_TESTP_MNEMONICS,
            F64_SELP_MNEMONICS,
            SHIFT_MNEMONICS,
            UNARY_MNEMONICS,
            CVT_MNEMONICS,
            NARROW_CVT_MNEMONICS,
            WIDE_CVT_MNEMONICS,
            SZEXT_MNEMONICS,
            FNS_MNEMONICS,
            PRMT_MODE_MNEMONICS,
            BFIND_MNEMONICS,
            BFE_MNEMONICS,
            BMSK_MNEMONICS,
            WIDE_BITFIELD_MNEMONICS,
            DIVREM_MNEMONICS,
            MUL_WIDE_MNEMONICS,
            MAD_WIDE_MNEMONICS,
            WIDE_INT_MNEMONICS,
            WIDE_MAD64_MNEMONICS,
            WIDE_SET_MNEMONICS,
            WIDE_SETP_MNEMONICS,
            WIDE_SETP_BOOL_MNEMONICS,
            WIDE_SELP_MNEMONICS,
            WIDE_UNARY_MNEMONICS,
            WIDE_SHIFT_MNEMONICS,
            WIDE_DIVREM_MNEMONICS,
            CARRY_MNEMONICS,
            WIDE_CARRY_MNEMONICS,
            UNSIGNED_SETP_MNEMONICS,
            SIGNED_SETP_MNEMONICS,
            SET_MNEMONICS,
            SELP_MNEMONICS,
            FUNNEL_MNEMONICS,
            SAD_MNEMONICS,
            SLCT_MNEMONICS,
            DP4A_MNEMONICS,
            DP2A_MNEMONICS,
            VIDEO_MNEMONICS,
        ] {
            mnemonics.extend_from_slice(group);
        }
        mnemonics.extend_from_slice(&["lop3.b32", "prmt.b32", "bfi.b32"]);
        mnemonics
    }

    fn post_known_bug_suppression_mnemonics() -> Vec<&'static str> {
        let mut mnemonics = Vec::new();
        for group in [
            POST_KNOWN_BIN_MNEMONICS,
            SCALAR_16BIT_POST_KNOWN_MNEMONICS,
            GLOBAL_LOAD_MNEMONICS,
            UNIFORM_GLOBAL_LOAD_MNEMONICS,
            GLOBAL_STORE_MNEMONICS,
            CONST_LOAD_MNEMONICS,
            LOCAL_MEM_LOAD_MNEMONICS,
            LOCAL_MEM_STORE_MNEMONICS,
            SHARED_MEM_LOAD_MNEMONICS,
            SHARED_MEM_STORE_MNEMONICS,
            VECTOR_MEMORY_MNEMONICS,
            F32_UNARY_MNEMONICS,
            F32_CVT_MNEMONICS,
            F32_SPECIAL_MATH_MNEMONICS,
            F32_SETP_MNEMONICS,
            F32_SETP_BOOL_MNEMONICS,
            F32_TESTP_MNEMONICS,
            F32_SELP_MNEMONICS,
            F64_ARITH_MNEMONICS,
            F64_ROUNDING_MNEMONICS,
            F64_UNARY_MNEMONICS,
            F64_CVT_MNEMONICS,
            F64_SPECIAL_MATH_MNEMONICS,
            F64_SETP_MNEMONICS,
            F64_SETP_BOOL_MNEMONICS,
            F64_TESTP_MNEMONICS,
            F64_SELP_MNEMONICS,
            POST_KNOWN_UNARY_MNEMONICS,
            CVT_MNEMONICS,
            NARROW_CVT_MNEMONICS,
            WIDE_CVT_MNEMONICS,
            SZEXT_MNEMONICS,
            FNS_MNEMONICS,
            BFE_MNEMONICS,
            BMSK_MNEMONICS,
            WIDE_BITFIELD_MNEMONICS,
            DIVREM_MNEMONICS,
            MAD_HI_MNEMONICS,
            MAD24_MNEMONICS,
            MUL24_MNEMONICS,
            MUL_WIDE_MNEMONICS,
            MAD_WIDE_MNEMONICS,
            WIDE_INT_MNEMONICS,
            WIDE_MAD64_MNEMONICS,
            WIDE_SHIFT_MNEMONICS,
            WIDE_CARRY_MNEMONICS,
            WIDE_DIVREM_MNEMONICS,
            UNSIGNED_SETP_MNEMONICS,
            SAD_MNEMONICS,
            POST_KNOWN_SLCT_MNEMONICS,
            DP4A_MNEMONICS,
            DP2A_MNEMONICS,
            POST_KNOWN_VIDEO_MNEMONICS,
        ] {
            mnemonics.extend_from_slice(group);
        }
        mnemonics
    }

    fn has_mnemonic(ptx: &str, mnemonic: &str) -> bool {
        ptx.lines()
            .filter_map(|line| line.trim_start().split_whitespace().next())
            .any(|token| token == mnemonic)
    }

    fn mnemonic_set(ptx: &str) -> HashSet<&str> {
        ptx.lines()
            .filter_map(|line| line.trim_start().split_whitespace().next())
            .collect()
    }

    fn body_mnemonic(line: &str) -> Option<&str> {
        let mut tokens = line.trim_start().split_whitespace();
        let first = tokens.next()?;
        if first.starts_with('@') {
            tokens.next()
        } else {
            Some(first)
        }
    }

    fn is_video_mnemonic(mnemonic: &str) -> bool {
        matches!(
            mnemonic.split('.').next(),
            Some(
                "vadd2"
                    | "vsub2"
                    | "vavrg2"
                    | "vabsdiff2"
                    | "vmin2"
                    | "vmax2"
                    | "vadd4"
                    | "vsub4"
                    | "vavrg4"
                    | "vabsdiff4"
                    | "vmin4"
                    | "vmax4"
            )
        )
    }

    fn is_signed_video_mnemonic(mnemonic: &str) -> bool {
        is_video_mnemonic(mnemonic) && mnemonic.split('.').any(|part| part == "s32")
    }

    fn is_video_sat_mnemonic(mnemonic: &str) -> bool {
        is_video_mnemonic(mnemonic) && mnemonic.ends_with(".sat")
    }

    fn is_vsub4_mnemonic(mnemonic: &str) -> bool {
        mnemonic.starts_with("vsub4.")
    }

    fn has_video_mnemonic(ptx: &str) -> bool {
        ptx.lines().filter_map(body_mnemonic).any(is_video_mnemonic)
    }

    fn has_signed_video_mnemonic(ptx: &str) -> bool {
        ptx.lines()
            .filter_map(body_mnemonic)
            .any(is_signed_video_mnemonic)
    }

    fn has_video_sat_mnemonic(ptx: &str) -> bool {
        ptx.lines()
            .filter_map(body_mnemonic)
            .any(is_video_sat_mnemonic)
    }

    fn has_vsub4_mnemonic(ptx: &str) -> bool {
        ptx.lines().filter_map(body_mnemonic).any(is_vsub4_mnemonic)
    }

    fn scalar_global_load_width(type_suffix: &str) -> Option<u32> {
        match type_suffix {
            "u8" | "s8" | "b8" => Some(1),
            "u16" | "s16" | "b16" => Some(2),
            "u32" | "b32" => Some(4),
            "u64" | "s64" | "b64" => Some(8),
            _ => None,
        }
    }

    fn scalar_global_store_width(type_suffix: &str) -> Option<u32> {
        match type_suffix {
            "u8" | "b8" => Some(1),
            "u16" | "b16" => Some(2),
            "u32" | "b32" => Some(4),
            "u64" | "b64" => Some(8),
            _ => None,
        }
    }

    fn const_load_width(mnemonic: &str) -> Option<u32> {
        mnemonic
            .strip_prefix("ld.const.")
            .and_then(scalar_global_load_width)
    }

    fn local_memory_width(mnemonic: &str) -> Option<u32> {
        let type_suffix = mnemonic
            .strip_prefix("ld.local.")
            .or_else(|| mnemonic.strip_prefix("st.local."))?;
        scalar_global_load_width(type_suffix).or_else(|| scalar_global_store_width(type_suffix))
    }

    fn shared_memory_width(mnemonic: &str) -> Option<u32> {
        let type_suffix = mnemonic
            .strip_prefix("ld.shared.")
            .or_else(|| mnemonic.strip_prefix("st.shared."))
            .or_else(|| mnemonic.strip_prefix("ld.volatile.shared."))
            .or_else(|| mnemonic.strip_prefix("st.volatile.shared."))?;
        scalar_global_load_width(type_suffix).or_else(|| scalar_global_store_width(type_suffix))
    }

    fn vector_memory_width_suffix(type_suffix: &str) -> Option<u32> {
        match type_suffix {
            "v2.u32" | "v2.b32" => Some(8),
            "v4.u32" | "v2.u64" | "v4.b32" | "v2.b64" => Some(16),
            _ => None,
        }
    }

    fn global_load_width(mnemonic: &str) -> Option<u32> {
        if let Some(type_suffix) = mnemonic.strip_prefix("ld.global.") {
            if let Some(width) = scalar_global_load_width(type_suffix) {
                return Some(width);
            }
        }
        if let Some(type_suffix) = mnemonic.strip_prefix("ldu.global.") {
            return scalar_global_load_width(type_suffix);
        }
        if let Some(type_suffix) = mnemonic.strip_prefix("ld.volatile.global.") {
            return scalar_global_load_width(type_suffix);
        }
        GLOBAL_LOAD_CACHE_PREFIXES.iter().find_map(|prefix| {
            mnemonic
                .strip_prefix(prefix)
                .and_then(scalar_global_load_width)
        })
    }

    fn global_store_width(mnemonic: &str) -> Option<u32> {
        if let Some(type_suffix) = mnemonic.strip_prefix("st.global.") {
            if let Some(width) = scalar_global_store_width(type_suffix) {
                return Some(width);
            }
        }
        if let Some(type_suffix) = mnemonic.strip_prefix("st.volatile.global.") {
            return scalar_global_store_width(type_suffix);
        }
        GLOBAL_STORE_CACHE_PREFIXES.iter().find_map(|prefix| {
            mnemonic
                .strip_prefix(prefix)
                .and_then(scalar_global_store_width)
        })
    }

    fn const_vector_width(mnemonic: &str) -> Option<u32> {
        mnemonic
            .strip_prefix("ld.const.")
            .and_then(vector_memory_width_suffix)
    }

    fn local_vector_width(mnemonic: &str) -> Option<u32> {
        mnemonic
            .strip_prefix("ld.local.")
            .or_else(|| mnemonic.strip_prefix("st.local."))
            .and_then(vector_memory_width_suffix)
    }

    fn global_vector_width(mnemonic: &str) -> Option<u32> {
        if let Some(type_suffix) = mnemonic
            .strip_prefix("ld.global.")
            .or_else(|| mnemonic.strip_prefix("ldu.global."))
            .or_else(|| mnemonic.strip_prefix("st.global."))
            .or_else(|| mnemonic.strip_prefix("ld.volatile.global."))
            .or_else(|| mnemonic.strip_prefix("st.volatile.global."))
        {
            if let Some(width) = vector_memory_width_suffix(type_suffix) {
                return Some(width);
            }
        }
        GLOBAL_LOAD_CACHE_PREFIXES
            .iter()
            .chain(GLOBAL_STORE_CACHE_PREFIXES.iter())
            .find_map(|prefix| {
                mnemonic
                    .strip_prefix(prefix)
                    .and_then(vector_memory_width_suffix)
            })
    }

    fn shared_vector_width(mnemonic: &str) -> Option<u32> {
        let type_suffix = mnemonic
            .strip_prefix("ld.shared.")
            .or_else(|| mnemonic.strip_prefix("st.shared."))
            .or_else(|| mnemonic.strip_prefix("ld.volatile.shared."))
            .or_else(|| mnemonic.strip_prefix("st.volatile.shared."))?;
        vector_memory_width_suffix(type_suffix)
    }

    fn is_global_load_mnemonic(mnemonic: &str) -> bool {
        global_load_width(mnemonic).is_some()
    }

    fn is_global_store_mnemonic(mnemonic: &str) -> bool {
        global_store_width(mnemonic).is_some()
    }

    fn is_vector_memory_mnemonic(mnemonic: &str) -> bool {
        VECTOR_MEMORY_MNEMONICS.contains(&mnemonic)
            || global_vector_width(mnemonic).is_some()
            || shared_vector_width(mnemonic).is_some()
    }

    fn is_global_memory_cache_mnemonic(mnemonic: &str) -> bool {
        GLOBAL_LOAD_CACHE_PREFIXES
            .iter()
            .chain(GLOBAL_STORE_CACHE_PREFIXES.iter())
            .any(|prefix| mnemonic.starts_with(prefix))
    }

    fn is_volatile_memory_mnemonic(mnemonic: &str) -> bool {
        mnemonic.starts_with("ld.volatile.") || mnemonic.starts_with("st.volatile.")
    }

    fn is_bit_memory_mnemonic(mnemonic: &str) -> bool {
        let is_memory = global_load_width(mnemonic).is_some()
            || global_store_width(mnemonic).is_some()
            || const_load_width(mnemonic).is_some()
            || local_memory_width(mnemonic).is_some()
            || shared_memory_width(mnemonic).is_some()
            || global_vector_width(mnemonic).is_some()
            || shared_vector_width(mnemonic).is_some()
            || const_vector_width(mnemonic).is_some()
            || local_vector_width(mnemonic).is_some();
        is_memory
            && (mnemonic.contains(".b8")
                || mnemonic.contains(".b16")
                || mnemonic.contains(".b32")
                || mnemonic.contains(".b64"))
    }

    fn body_global_load(line: &str) -> Option<(&str, u32)> {
        let line = line.trim_start();
        if !line.contains("[%rd6 + ") {
            return None;
        }
        let mnemonic = body_mnemonic(line)?;
        if !is_global_load_mnemonic(mnemonic) {
            return None;
        }
        let offset = line
            .split("[%rd6 + ")
            .nth(1)?
            .trim_end_matches("];")
            .parse()
            .ok()?;
        Some((mnemonic, offset))
    }

    fn has_body_global_load(ptx: &str, mnemonic: &str) -> bool {
        ptx.lines()
            .filter_map(body_global_load)
            .any(|(op, _)| op == mnemonic)
    }

    fn body_global_store_roundtrip_access(line: &str) -> Option<(&str, u32)> {
        let line = line.trim_start();
        if !line.contains("[%rd8 + ") {
            return None;
        }
        let mnemonic = body_mnemonic(line)?;
        if !is_global_load_mnemonic(mnemonic) && !is_global_store_mnemonic(mnemonic) {
            return None;
        }
        let offset = line
            .split("[%rd8 + ")
            .nth(1)?
            .split(']')
            .next()?
            .parse()
            .ok()?;
        Some((mnemonic, offset))
    }

    fn has_body_global_store_roundtrip_access(ptx: &str, mnemonic: &str) -> bool {
        ptx.lines()
            .filter_map(body_global_store_roundtrip_access)
            .any(|(op, _)| op == mnemonic)
    }

    fn body_const_load(line: &str) -> Option<(&str, u32)> {
        let line = line.trim_start();
        if !line.contains("[%rd6 + ") {
            return None;
        }
        let mnemonic = body_mnemonic(line)?;
        if const_load_width(mnemonic).is_none() {
            return None;
        }
        let offset = line
            .split("[%rd6 + ")
            .nth(1)?
            .trim_end_matches("];")
            .parse()
            .ok()?;
        Some((mnemonic, offset))
    }

    fn has_body_const_load(ptx: &str, mnemonic: &str) -> bool {
        ptx.lines()
            .filter_map(body_const_load)
            .any(|(op, _)| op == mnemonic)
    }

    fn body_local_mem_access(line: &str) -> Option<(&str, u32)> {
        let line = line.trim_start();
        if !line.contains("[%rd6 + ") {
            return None;
        }
        let mnemonic = body_mnemonic(line)?;
        if local_memory_width(mnemonic).is_none() {
            return None;
        }
        let offset = line
            .split("[%rd6 + ")
            .nth(1)?
            .split(']')
            .next()?
            .parse()
            .ok()?;
        Some((mnemonic, offset))
    }

    fn has_body_local_mem_access(ptx: &str, mnemonic: &str) -> bool {
        ptx.lines()
            .filter_map(body_local_mem_access)
            .any(|(op, _)| op == mnemonic)
    }

    fn body_shared_mem_access(line: &str) -> Option<(&str, u32)> {
        let line = line.trim_start();
        if !line.contains("[%rd6 + ") {
            return None;
        }
        let mnemonic = body_mnemonic(line)?;
        if shared_memory_width(mnemonic).is_none() {
            return None;
        }
        let offset = line
            .split("[%rd6 + ")
            .nth(1)?
            .split(']')
            .next()?
            .parse()
            .ok()?;
        Some((mnemonic, offset))
    }

    fn has_body_shared_mem_access(ptx: &str, mnemonic: &str) -> bool {
        ptx.lines()
            .filter_map(body_shared_mem_access)
            .any(|(op, _)| op == mnemonic)
    }

    fn body_vector_memory_access(line: &str) -> Option<(&str, &str, u32)> {
        let line = line.trim_start();
        let mut tokens = line.split_whitespace();
        let first = tokens.next()?;
        let mnemonic = if first.starts_with('@') {
            tokens.next()?
        } else {
            first
        };
        if !is_vector_memory_mnemonic(mnemonic) {
            return None;
        }
        let address = if line.contains("[%rd8 + ") {
            "%rd8"
        } else if line.contains("[%rd6 + ") {
            "%rd6"
        } else {
            return None;
        };
        let offset = line
            .split(&format!("[{address} + "))
            .nth(1)?
            .split(']')
            .next()?
            .parse()
            .ok()?;
        Some((mnemonic, address, offset))
    }

    fn has_special_reg(ptx: &str, reg_name: &str) -> bool {
        ptx.lines().any(|line| {
            let line = line.trim_start();
            line.starts_with("mov.u32       %r") && line.ends_with(&format!("{reg_name};"))
        })
    }

    fn has_predicated_special_reg(ptx: &str) -> bool {
        ptx.lines().any(|line| {
            let line = line.trim_start();
            line.starts_with('@')
                && line.contains(" mov.u32 ")
                && SPECIAL_REG_NAMES
                    .iter()
                    .any(|reg_name| line.ends_with(&format!("{reg_name};")))
        })
    }

    fn parse_u32_reg(token: &str) -> Option<u32> {
        token.trim().strip_prefix("%r")?.parse().ok()
    }

    fn has_wide_high_result(ptx: &str) -> bool {
        ptx.lines().any(|line| {
            let line = line.trim_start();
            let mut tokens = line.split_whitespace();
            let first = tokens.next();
            let op = if first.is_some_and(|token| token.starts_with('@')) {
                tokens.next()
            } else {
                first
            };
            if op != Some("mov.b64") {
                return false;
            }

            let Some(start) = line.find('{') else {
                return false;
            };
            let Some(end) = line[start + 1..].find('}') else {
                return false;
            };
            let regs = &line[start + 1..start + 1 + end];
            let mut parts = regs.split(',');
            let Some(lo) = parts.next().and_then(parse_u32_reg) else {
                return false;
            };
            let Some(hi) = parts.next().and_then(parse_u32_reg) else {
                return false;
            };
            lo > hi
        })
    }

    fn predicated_mnemonic(line: &str) -> Option<&str> {
        let mut tokens = line.trim_start().split_whitespace();
        let pred = tokens.next()?;
        let op = tokens.next()?;
        (pred.starts_with("@%p") || pred.starts_with("@!%p")).then_some(op)
    }

    fn has_predicated_memory_access(ptx: &str, mnemonics: &[&str], address: &str) -> bool {
        ptx.lines().any(|line| {
            line.contains(address)
                && predicated_mnemonic(line).is_some_and(|op| mnemonics.contains(&op))
        })
    }

    fn has_predicated_global_load_access(ptx: &str) -> bool {
        ptx.lines().any(|line| {
            line.contains("[%rd6 + ")
                && predicated_mnemonic(line).is_some_and(is_global_load_mnemonic)
        })
    }

    fn has_predicated_global_roundtrip_access(ptx: &str) -> bool {
        ptx.lines().any(|line| {
            line.contains("[%rd8 + ")
                && predicated_mnemonic(line).is_some_and(|op| {
                    is_global_load_mnemonic(op)
                        || is_global_store_mnemonic(op)
                        || is_vector_memory_mnemonic(op)
                })
        })
    }

    fn has_any_predicated_memory_access(ptx: &str) -> bool {
        has_predicated_global_load_access(ptx)
            || has_predicated_global_roundtrip_access(ptx)
            || has_predicated_memory_access(ptx, CONST_LOAD_MNEMONICS, "[%rd6 + ")
            || has_predicated_memory_access(ptx, LOCAL_MEM_LOAD_MNEMONICS, "[%rd6 + ")
            || has_predicated_memory_access(ptx, LOCAL_MEM_STORE_MNEMONICS, "[%rd6 + ")
            || has_predicated_memory_access(ptx, SHARED_MEM_LOAD_MNEMONICS, "[%rd6 + ")
            || has_predicated_memory_access(ptx, SHARED_MEM_STORE_MNEMONICS, "[%rd6 + ")
            || has_predicated_memory_access(ptx, VECTOR_MEMORY_MNEMONICS, "[%rd6 + ")
            || has_predicated_memory_access(ptx, VECTOR_MEMORY_MNEMONICS, "[%rd8 + ")
    }

    fn has_predicated_vector_memory_access(ptx: &str) -> bool {
        ptx.lines().any(|line| {
            (line.contains("[%rd6 + ") || line.contains("[%rd8 + "))
                && predicated_mnemonic(line).is_some_and(is_vector_memory_mnemonic)
        })
    }

    fn has_negated_predicate(ptx: &str) -> bool {
        ptx.lines()
            .any(|line| line.trim_start().starts_with("@!%p"))
    }

    fn has_register_shift(ptx: &str) -> bool {
        let lines: Vec<_> = ptx.lines().map(str::trim_start).collect();
        lines.windows(2).any(|pair| {
            let mask = pair[0];
            let shift = pair[1];
            if !mask.starts_with("and.b32") || !mask.ends_with(", 31;") {
                return false;
            }
            let Some(mask_dst) = mask
                .split_whitespace()
                .nth(1)
                .map(|token| token.trim_end_matches(','))
            else {
                return false;
            };
            ["shl.b32", "shr.u32", "shr.s32"]
                .iter()
                .any(|mnemonic| shift.starts_with(mnemonic))
                && shift.ends_with(&format!(", {mask_dst};"))
        })
    }

    fn has_predicated_register_shift(ptx: &str) -> bool {
        ptx.lines().any(|line| {
            let line = line.trim_start();
            predicated_mnemonic(line).is_some_and(|op| SHIFT_MNEMONICS.contains(&op))
                && line
                    .trim_end_matches(';')
                    .split(',')
                    .next_back()
                    .is_some_and(|amount| amount.trim_start().starts_with("%r"))
        })
    }

    fn has_predicated_alu(ptx: &str) -> bool {
        ptx.lines().filter_map(predicated_mnemonic).any(|op| {
            BIN_MNEMONICS.contains(&op)
                || PACKED_ADD_MNEMONICS.contains(&op)
                || PACKED_MINMAX_MNEMONICS.contains(&op)
                || SCALAR_16BIT_MNEMONICS.contains(&op)
                || F32_ARITH_MNEMONICS.contains(&op)
                || F32_ROUNDING_MNEMONICS.contains(&op)
                || F64_ARITH_MNEMONICS.contains(&op)
                || F64_ROUNDING_MNEMONICS.contains(&op)
        })
    }

    fn assert_predicated_mnemonic_coverage(
        cfg: &GenConfig,
        program_bytes: usize,
        n_seeds: u64,
        mnemonics: &[&'static str],
    ) {
        let mut found = vec![false; mnemonics.len()];

        for seed in 0..n_seeds {
            let bytes = bytes_from_seed(seed, program_bytes);
            let ptx = generate_from_bytes_with_config(&bytes, cfg).unwrap();
            let seen: HashSet<_> = ptx.lines().filter_map(predicated_mnemonic).collect();
            for (i, mnemonic) in mnemonics.iter().enumerate() {
                found[i] |= seen.contains(mnemonic);
            }
            if found.iter().all(|seen| *seen) {
                return;
            }
        }

        let missing: Vec<_> = mnemonics
            .iter()
            .zip(found)
            .filter_map(|(mnemonic, seen)| (!seen).then_some(*mnemonic))
            .collect();
        assert!(
            missing.is_empty(),
            "sample did not emit predicated {missing:?}"
        );
    }

    fn setp_bool_suffix(line: &str) -> Option<&'static str> {
        let op = line.trim_start().split_whitespace().next()?;
        if !op.starts_with("setp.") {
            return None;
        }
        if op.contains(".and.") {
            Some("and")
        } else if op.contains(".or.") {
            Some("or")
        } else if op.contains(".xor.") {
            Some("xor")
        } else {
            None
        }
    }

    fn has_setp_bool(ptx: &str) -> bool {
        ptx.lines().any(|line| setp_bool_suffix(line).is_some())
    }

    fn has_setp_dual(ptx: &str) -> bool {
        ptx.lines().any(|line| {
            let line = line.trim_start();
            line.starts_with("setp.") && line.contains("|%p")
        })
    }

    fn has_pred_logic(ptx: &str) -> bool {
        ptx.lines()
            .filter_map(|line| line.trim_start().split_whitespace().next())
            .any(|op| PRED_LOGIC_MNEMONICS.contains(&op))
    }

    fn has_predicated_shift(ptx: &str) -> bool {
        ptx.lines()
            .filter_map(predicated_mnemonic)
            .any(|op| SHIFT_MNEMONICS.contains(&op))
    }

    fn has_predicated_unary(ptx: &str) -> bool {
        ptx.lines().filter_map(predicated_mnemonic).any(|op| {
            UNARY_MNEMONICS.contains(&op)
                || F32_UNARY_MNEMONICS.contains(&op)
                || F32_SPECIAL_MATH_MNEMONICS.contains(&op)
                || F64_UNARY_MNEMONICS.contains(&op)
                || F64_SPECIAL_MATH_MNEMONICS.contains(&op)
        })
    }

    fn has_predicated_cvt(ptx: &str) -> bool {
        ptx.lines().filter_map(predicated_mnemonic).any(|op| {
            CVT_MNEMONICS.contains(&op)
                || F32_CVT_MNEMONICS.contains(&op)
                || F64_CVT_MNEMONICS.contains(&op)
        })
    }

    fn has_predicated_narrow_cvt(ptx: &str) -> bool {
        ptx.lines()
            .filter_map(predicated_mnemonic)
            .any(|op| NARROW_CVT_MNEMONICS.contains(&op))
    }

    fn has_predicated_wide_cvt(ptx: &str) -> bool {
        ptx.lines()
            .filter_map(predicated_mnemonic)
            .any(|op| WIDE_CVT_MNEMONICS.contains(&op))
    }

    fn has_predicated_szext(ptx: &str) -> bool {
        ptx.lines()
            .filter_map(predicated_mnemonic)
            .any(|op| SZEXT_MNEMONICS.contains(&op))
    }

    fn has_predicated_fns(ptx: &str) -> bool {
        ptx.lines()
            .filter_map(predicated_mnemonic)
            .any(|op| FNS_MNEMONICS.contains(&op))
    }

    fn has_predicated_bfind(ptx: &str) -> bool {
        ptx.lines()
            .filter_map(predicated_mnemonic)
            .any(|op| BFIND_MNEMONICS.contains(&op))
    }

    fn has_predicated_wide_bfind(ptx: &str) -> bool {
        ptx.lines()
            .filter_map(predicated_mnemonic)
            .any(|op| WIDE_BFIND_MNEMONICS.contains(&op))
    }

    fn has_predicated_mad(ptx: &str) -> bool {
        ptx.lines()
            .filter_map(predicated_mnemonic)
            .any(|op| MAD_LO_MNEMONICS.contains(&op))
    }

    fn has_predicated_set(ptx: &str) -> bool {
        ptx.lines().filter_map(predicated_mnemonic).any(|op| {
            SET_MNEMONICS.contains(&op)
                || F32_COMPARE_MNEMONICS.contains(&op)
                || F32_SETP_BOOL_MNEMONICS.contains(&op)
                || F32_TESTP_MNEMONICS.contains(&op)
                || F64_COMPARE_MNEMONICS.contains(&op)
                || F64_SETP_BOOL_MNEMONICS.contains(&op)
                || F64_TESTP_MNEMONICS.contains(&op)
        })
    }

    fn has_predicated_selp(ptx: &str) -> bool {
        ptx.lines().filter_map(predicated_mnemonic).any(|op| {
            SELP_MNEMONICS.contains(&op)
                || F32_SELP_MNEMONICS.contains(&op)
                || F64_SELP_MNEMONICS.contains(&op)
        })
    }

    fn has_direct_typed_selp(ptx: &str) -> bool {
        ptx.lines().any(|line| {
            let Some(op) = body_mnemonic(line) else {
                return false;
            };
            op == "selp.s32" || (op == "selp.u32" && !line.contains(", 1, 0,"))
        })
    }

    fn has_predicated_divrem(ptx: &str) -> bool {
        ptx.lines()
            .filter_map(predicated_mnemonic)
            .any(|op| DIVREM_MNEMONICS.contains(&op))
    }

    fn is_reg_divrem_line(line: &str, predicated: bool) -> bool {
        let line = line.trim_start();
        if predicated != line.starts_with('@') {
            return false;
        }
        let mut tokens = line.split_whitespace();
        let op = if predicated {
            let _pred = tokens.next();
            tokens.next()
        } else {
            tokens.next()
        };
        if !matches!(op, Some("div.u32" | "rem.u32")) {
            return false;
        }
        line.trim_end_matches(';')
            .split(',')
            .next_back()
            .is_some_and(|divisor| divisor.trim_start().starts_with("%r"))
    }

    fn has_reg_divrem(ptx: &str) -> bool {
        ptx.lines().any(|line| is_reg_divrem_line(line, false))
    }

    fn has_predicated_reg_divrem(ptx: &str) -> bool {
        ptx.lines().any(|line| is_reg_divrem_line(line, true))
    }

    fn has_predicated_lop3(ptx: &str) -> bool {
        ptx.lines()
            .filter_map(predicated_mnemonic)
            .any(|op| op == "lop3.b32")
    }

    fn has_predicated_prmt(ptx: &str) -> bool {
        ptx.lines()
            .filter_map(predicated_mnemonic)
            .any(|op| op == "prmt.b32")
    }

    fn has_predicated_24bit(ptx: &str) -> bool {
        ptx.lines()
            .filter_map(predicated_mnemonic)
            .any(|op| MAD24_MNEMONICS.contains(&op) || MUL24_MNEMONICS.contains(&op))
    }

    fn has_predicated_mul_wide(ptx: &str) -> bool {
        ptx.lines()
            .filter_map(predicated_mnemonic)
            .any(|op| MUL_WIDE_MNEMONICS.contains(&op))
    }

    fn has_predicated_mad_wide(ptx: &str) -> bool {
        ptx.lines()
            .filter_map(predicated_mnemonic)
            .any(|op| MAD_WIDE_MNEMONICS.contains(&op))
    }

    fn has_predicated_subword_wide(ptx: &str) -> bool {
        ptx.lines()
            .filter_map(predicated_mnemonic)
            .any(|op| SUBWORD_WIDE_MNEMONICS.contains(&op))
    }

    fn has_predicated_wide_int(ptx: &str) -> bool {
        ptx.lines()
            .filter_map(predicated_mnemonic)
            .any(|op| WIDE_INT_MNEMONICS.contains(&op))
    }

    fn has_predicated_wide_mad64(ptx: &str) -> bool {
        ptx.lines()
            .filter_map(predicated_mnemonic)
            .any(|op| WIDE_MAD64_MNEMONICS.contains(&op))
    }

    fn has_predicated_wide_set(ptx: &str) -> bool {
        ptx.lines()
            .filter_map(predicated_mnemonic)
            .any(|op| WIDE_SET_MNEMONICS.contains(&op))
    }

    fn has_predicated_wide_shift(ptx: &str) -> bool {
        ptx.lines()
            .filter_map(predicated_mnemonic)
            .any(|op| WIDE_SHIFT_MNEMONICS.contains(&op))
    }

    fn is_wide_reg_shift_line(line: &str, predicated: bool) -> bool {
        let line = line.trim_start();
        if predicated != line.starts_with('@') {
            return false;
        }
        let mut tokens = line.split_whitespace();
        let op = if predicated {
            let _pred = tokens.next();
            tokens.next()
        } else {
            tokens.next()
        };
        if !op.is_some_and(|op| WIDE_SHIFT_MNEMONICS.contains(&op)) {
            return false;
        }
        line.trim_end_matches(';')
            .split(',')
            .next_back()
            .is_some_and(|amount| amount.trim_start().starts_with("%r"))
    }

    fn has_wide_reg_shift(ptx: &str) -> bool {
        ptx.lines().any(|line| is_wide_reg_shift_line(line, false))
    }

    fn has_predicated_wide_reg_shift(ptx: &str) -> bool {
        ptx.lines().any(|line| is_wide_reg_shift_line(line, true))
    }

    fn has_predicated_wide_unary(ptx: &str) -> bool {
        ptx.lines()
            .filter_map(predicated_mnemonic)
            .any(|op| WIDE_UNARY_MNEMONICS.contains(&op))
    }

    fn has_signed_wide_unary_mnemonic(ptx: &str) -> bool {
        ptx.lines()
            .filter_map(body_mnemonic)
            .any(|op| SIGNED_WIDE_UNARY_MNEMONICS.contains(&op))
    }

    fn has_predicated_wide_divrem(ptx: &str) -> bool {
        ptx.lines()
            .filter_map(predicated_mnemonic)
            .any(|op| WIDE_DIVREM_MNEMONICS.contains(&op))
    }

    fn is_wide_reg_divrem_line(line: &str, predicated: bool) -> bool {
        let line = line.trim_start();
        if predicated != line.starts_with('@') {
            return false;
        }
        let mut tokens = line.split_whitespace();
        let op = if predicated {
            let _pred = tokens.next();
            tokens.next()
        } else {
            tokens.next()
        };
        if !op.is_some_and(|op| WIDE_DIVREM_MNEMONICS.contains(&op)) {
            return false;
        }
        line.trim_end_matches(';')
            .split(',')
            .next_back()
            .is_some_and(|divisor| divisor.trim_start().starts_with("%rd"))
    }

    fn has_wide_reg_divrem(ptx: &str) -> bool {
        ptx.lines().any(|line| is_wide_reg_divrem_line(line, false))
    }

    fn has_predicated_wide_reg_divrem(ptx: &str) -> bool {
        ptx.lines().any(|line| is_wide_reg_divrem_line(line, true))
    }

    fn has_predicated_carry(ptx: &str) -> bool {
        ptx.lines()
            .filter_map(predicated_mnemonic)
            .any(|op| CARRY_MNEMONICS.contains(&op))
    }

    fn has_predicated_wide_carry(ptx: &str) -> bool {
        ptx.lines()
            .filter_map(predicated_mnemonic)
            .any(|op| WIDE_CARRY_MNEMONICS.contains(&op))
    }

    fn has_predicated_wide_carry_chain(ptx: &str) -> bool {
        ptx.lines()
            .filter_map(predicated_mnemonic)
            .any(|op| WIDE_CARRY_CHAIN_CC_MNEMONICS.contains(&op))
    }

    fn has_predicated_carry_chain(ptx: &str) -> bool {
        ptx.lines()
            .filter_map(predicated_mnemonic)
            .any(|op| CARRY_CHAIN_CC_MNEMONICS.contains(&op))
    }

    fn has_predicated_mad_carry(ptx: &str) -> bool {
        ptx.lines()
            .filter_map(predicated_mnemonic)
            .any(|op| MAD_CARRY_MNEMONICS.contains(&op))
    }

    fn has_predicated_sad(ptx: &str) -> bool {
        ptx.lines()
            .filter_map(predicated_mnemonic)
            .any(|op| SAD_MNEMONICS.contains(&op))
    }

    fn is_slct_mnemonic(mnemonic: &str) -> bool {
        SLCT_MNEMONICS.contains(&mnemonic)
            || F32_SLCT_MNEMONICS.contains(&mnemonic)
            || WIDE_SLCT_MNEMONICS.contains(&mnemonic)
            || F64_SLCT_MNEMONICS.contains(&mnemonic)
    }

    fn is_s32_slct_mnemonic(mnemonic: &str) -> bool {
        mnemonic.starts_with("slct.s32.")
    }

    fn has_slct_mnemonic(ptx: &str) -> bool {
        ptx.lines().filter_map(body_mnemonic).any(is_slct_mnemonic)
    }

    fn has_f32_slct_mnemonic(ptx: &str) -> bool {
        ptx.lines()
            .filter_map(body_mnemonic)
            .any(|op| F32_SLCT_MNEMONICS.contains(&op))
    }

    fn has_wide_slct_mnemonic(ptx: &str) -> bool {
        ptx.lines()
            .filter_map(body_mnemonic)
            .any(|op| WIDE_SLCT_MNEMONICS.contains(&op))
    }

    fn has_f64_slct_mnemonic(ptx: &str) -> bool {
        ptx.lines()
            .filter_map(body_mnemonic)
            .any(|op| F64_SLCT_MNEMONICS.contains(&op))
    }

    fn has_s32_slct_mnemonic(ptx: &str) -> bool {
        ptx.lines()
            .filter_map(body_mnemonic)
            .any(is_s32_slct_mnemonic)
    }

    fn has_predicated_slct(ptx: &str) -> bool {
        ptx.lines()
            .filter_map(predicated_mnemonic)
            .any(is_slct_mnemonic)
    }

    fn has_predicated_dp(ptx: &str) -> bool {
        ptx.lines()
            .filter_map(predicated_mnemonic)
            .any(|op| DP4A_MNEMONICS.contains(&op) || DP2A_MNEMONICS.contains(&op))
    }

    fn has_predicated_video(ptx: &str) -> bool {
        ptx.lines()
            .filter_map(predicated_mnemonic)
            .any(is_video_mnemonic)
    }

    fn has_predicated_funnel(ptx: &str) -> bool {
        ptx.lines()
            .filter_map(predicated_mnemonic)
            .any(|op| FUNNEL_MNEMONICS.contains(&op))
    }

    fn has_predicated_bitfield(ptx: &str) -> bool {
        ptx.lines()
            .filter_map(predicated_mnemonic)
            .any(|op| BITFIELD_MNEMONICS.contains(&op))
    }

    fn has_predicated_wide_bitfield(ptx: &str) -> bool {
        ptx.lines()
            .filter_map(predicated_mnemonic)
            .any(|op| WIDE_BITFIELD_MNEMONICS.contains(&op))
    }

    fn bitfield_param_registers(line: &str) -> Option<(bool, bool, bool)> {
        let line = line.trim_start();
        let predicated = line.starts_with('@');
        let inst = if predicated {
            line.split_once(char::is_whitespace)?.1.trim_start()
        } else {
            line
        };
        let op = inst.split_whitespace().next()?;
        if !BITFIELD_MNEMONICS.contains(&op) {
            return None;
        }
        let args: Vec<_> = inst
            .strip_prefix(op)?
            .trim()
            .trim_end_matches(';')
            .split(',')
            .map(str::trim)
            .collect();
        let (pos, len) = match op {
            "bfe.u32" | "bfe.s32" => (args.get(2)?, args.get(3)?),
            "bfi.b32" => (args.get(3)?, args.get(4)?),
            "bmsk.clamp.b32" | "bmsk.wrap.b32" => (args.get(1)?, args.get(2)?),
            _ => return None,
        };
        Some((predicated, pos.starts_with("%r"), len.starts_with("%r")))
    }

    fn has_register_bitfield_param(ptx: &str) -> bool {
        ptx.lines()
            .filter_map(bitfield_param_registers)
            .any(|(_, pos_reg, len_reg)| pos_reg || len_reg)
    }

    fn has_predicated_register_bitfield_param(ptx: &str) -> bool {
        ptx.lines()
            .filter_map(bitfield_param_registers)
            .any(|(predicated, pos_reg, len_reg)| predicated && (pos_reg || len_reg))
    }

    fn fns_param_registers(line: &str) -> Option<(bool, bool, bool)> {
        let line = line.trim_start();
        let predicated = line.starts_with('@');
        let inst = if predicated {
            line.split_once(char::is_whitespace)?.1.trim_start()
        } else {
            line
        };
        let op = inst.split_whitespace().next()?;
        if op != "fns.b32" {
            return None;
        }
        let args: Vec<_> = inst
            .strip_prefix(op)?
            .trim()
            .trim_end_matches(';')
            .split(',')
            .map(str::trim)
            .collect();
        let base = args.get(2)?;
        let offset = args.get(3)?;
        Some((predicated, base.starts_with("%r"), offset.starts_with("%r")))
    }

    fn has_register_fns_param(ptx: &str) -> bool {
        ptx.lines()
            .filter_map(fns_param_registers)
            .any(|(_, base_reg, offset_reg)| base_reg || offset_reg)
    }

    fn has_predicated_register_fns_param(ptx: &str) -> bool {
        ptx.lines()
            .filter_map(fns_param_registers)
            .any(|(predicated, base_reg, offset_reg)| predicated && (base_reg || offset_reg))
    }

    fn wide_bitfield_param_registers(line: &str) -> Option<(bool, bool, bool)> {
        let line = line.trim_start();
        let predicated = line.starts_with('@');
        let inst = if predicated {
            line.split_once(char::is_whitespace)?.1.trim_start()
        } else {
            line
        };
        let op = inst.split_whitespace().next()?;
        if !WIDE_BITFIELD_MNEMONICS.contains(&op) {
            return None;
        }
        let args: Vec<_> = inst
            .strip_prefix(op)?
            .trim()
            .trim_end_matches(';')
            .split(',')
            .map(str::trim)
            .collect();
        let (pos, len) = match op {
            "bfe.u64" | "bfe.s64" => (args.get(2)?, args.get(3)?),
            "bfi.b64" => (args.get(3)?, args.get(4)?),
            _ => return None,
        };
        Some((predicated, pos.starts_with("%r"), len.starts_with("%r")))
    }

    fn has_register_wide_bitfield_param(ptx: &str) -> bool {
        ptx.lines()
            .filter_map(wide_bitfield_param_registers)
            .any(|(_, pos_reg, len_reg)| pos_reg || len_reg)
    }

    fn has_predicated_register_wide_bitfield_param(ptx: &str) -> bool {
        ptx.lines()
            .filter_map(wide_bitfield_param_registers)
            .any(|(predicated, pos_reg, len_reg)| predicated && (pos_reg || len_reg))
    }

    fn has_register_funnel(ptx: &str) -> bool {
        ptx.lines().any(|line| {
            let line = line.trim_start();
            if !FUNNEL_MNEMONICS
                .iter()
                .any(|mnemonic| line.starts_with(mnemonic))
            {
                return false;
            }
            line.trim_end_matches(';')
                .split(',')
                .next_back()
                .is_some_and(|amount| amount.trim_start().starts_with("%r"))
        })
    }

    fn prmt_dst(line: &str) -> Option<&str> {
        let mut tokens = line.trim_start().split_whitespace();
        let first = tokens.next()?;
        let op = if first.starts_with('@') {
            tokens.next()?
        } else {
            first
        };
        (op == "prmt.b32" || PRMT_MODE_MNEMONICS.contains(&op))
            .then(|| tokens.next())
            .flatten()
            .map(|token| token.trim_end_matches(','))
    }

    fn prmt_control_register(line: &str) -> Option<(bool, bool)> {
        let line = line.trim_start();
        let predicated = line.starts_with('@');
        let mut tokens = line.split_whitespace();
        let op = if predicated {
            let _pred = tokens.next()?;
            tokens.next()?
        } else {
            tokens.next()?
        };
        if op != "prmt.b32" && !PRMT_MODE_MNEMONICS.contains(&op) {
            return None;
        }
        let last_arg = line
            .trim_end_matches(';')
            .split(',')
            .next_back()?
            .trim_start();
        Some((predicated, last_arg.starts_with("%r")))
    }

    fn has_register_prmt_control(ptx: &str) -> bool {
        ptx.lines()
            .filter_map(prmt_control_register)
            .any(|(_, is_reg)| is_reg)
    }

    fn has_predicated_register_prmt_control(ptx: &str) -> bool {
        ptx.lines()
            .filter_map(prmt_control_register)
            .any(|(predicated, is_reg)| predicated && is_reg)
    }

    fn cvt_src(line: &str) -> Option<&str> {
        let mut tokens = line.trim_start().split_whitespace();
        let first = tokens.next()?;
        let op = if first.starts_with('@') {
            tokens.next()?
        } else {
            first
        };
        if !CVT_MNEMONICS.contains(&op) {
            return None;
        }
        let _dst = tokens.next()?;
        tokens
            .next()
            .map(|token| token.trim_end_matches([';', ',']))
    }

    fn has_direct_prmt_cvt_dependency(ptx: &str) -> bool {
        let lines: Vec<_> = ptx.lines().collect();
        lines.windows(2).any(|pair| {
            prmt_dst(pair[0])
                .zip(cvt_src(pair[1]))
                .is_some_and(|(dst, src)| dst == src)
        })
    }

    fn assert_mnemonic_coverage(
        cfg: &GenConfig,
        program_bytes: usize,
        n_seeds: u64,
        mnemonics: &[&'static str],
    ) {
        let mut found = vec![false; mnemonics.len()];

        for seed in 0..n_seeds {
            let bytes = bytes_from_seed(seed, program_bytes);
            let ptx = generate_from_bytes_with_config(&bytes, cfg).unwrap();
            let seen = mnemonic_set(&ptx);
            for (i, mnemonic) in mnemonics.iter().enumerate() {
                found[i] |= seen.contains(mnemonic);
            }
            if found.iter().all(|seen| *seen) {
                return;
            }
        }

        let missing: Vec<_> = mnemonics
            .iter()
            .zip(found)
            .filter_map(|(mnemonic, seen)| (!seen).then_some(*mnemonic))
            .collect();
        assert!(missing.is_empty(), "sample did not emit {missing:?}");
    }

    fn assert_body_mnemonic_coverage(
        cfg: &GenConfig,
        program_bytes: usize,
        n_seeds: u64,
        mnemonics: &[&'static str],
    ) {
        let mut found = vec![false; mnemonics.len()];

        for seed in 0..n_seeds {
            let bytes = bytes_from_seed(seed, program_bytes);
            let ptx = generate_from_bytes_with_config(&bytes, cfg).unwrap();
            let seen: HashSet<_> = ptx.lines().filter_map(body_mnemonic).collect();
            for (i, mnemonic) in mnemonics.iter().enumerate() {
                found[i] |= seen.contains(mnemonic);
            }
            if found.iter().all(|seen| *seen) {
                return;
            }
        }

        let missing: Vec<_> = mnemonics
            .iter()
            .zip(found)
            .filter_map(|(mnemonic, seen)| (!seen).then_some(*mnemonic))
            .collect();
        assert!(
            missing.is_empty(),
            "sample did not emit body mnemonics {missing:?}"
        );
    }

    fn coverage_heavy_config() -> GenConfig {
        GenConfig {
            min_blocks: 16,
            max_blocks: 24,
            min_insts_per_block: 16,
            max_insts_per_block: 24,
            n_working_regs: 24,
            max_immediate: 65536,
            emit_f32_slct: false,
            emit_wide_slct: false,
            emit_f64_slct: false,
            emit_signed_video: false,
            emit_video_sat: false,
            ..GenConfig::default()
        }
    }

    fn post_known_bug_suppression_config() -> GenConfig {
        GenConfig {
            control_flow: ControlFlowMode::Structured,
            min_blocks: 16,
            max_blocks: 24,
            min_insts_per_block: 16,
            max_insts_per_block: 24,
            n_working_regs: 24,
            max_immediate: 65536,
            max_structured_depth: 6,
            emit_structured_loops: false,
            emit_arbitrary_loops: false,
            emit_lop3: false,
            emit_minmax: false,
            emit_selp: false,
            emit_mul_lo: false,
            emit_sat_arith: false,
            emit_packed_add: false,
            emit_signed_packed_add: false,
            emit_packed_minmax: false,
            emit_signed_packed_minmax: false,
            emit_predicated_packed_minmax: false,
            emit_mulhi: false,
            emit_or: false,
            emit_xor: false,
            emit_prmt: false,
            emit_not: false,
            emit_brev: false,
            emit_cnot: false,
            emit_abs: false,
            emit_predicated_unary: false,
            emit_signed_cmp: false,
            emit_funnel: false,
            emit_neg: false,
            emit_shl: false,
            emit_shr: false,
            emit_signed_shr: false,
            emit_bfind: false,
            emit_bfi: false,
            emit_reg_bitfield: false,
            emit_scalar_16bit_min: false,
            emit_scalar_16bit_signed_unary: false,
            emit_addc: false,
            emit_subc: false,
            emit_f32_arith: false,
            emit_f32_rounding: false,
            emit_i32_boundary_immediates: false,
            emit_set: false,
            emit_s32_slct: false,
            emit_f32_slct: false,
            emit_wide_slct: false,
            emit_f64_slct: false,
            emit_vsub4: false,
            ..GenConfig::default()
        }
    }

    fn dot_video_focused_config() -> GenConfig {
        GenConfig {
            control_flow: ControlFlowMode::Arbitrary,
            min_blocks: 1,
            max_blocks: 1,
            min_insts_per_block: 1024,
            max_insts_per_block: 1024,
            n_working_regs: 96,
            max_immediate: u32::MAX,
            emit_structured_loops: false,
            emit_arbitrary_loops: false,
            emit_lop3: false,
            emit_minmax: false,
            emit_selp: false,
            emit_sub: false,
            emit_mul_lo: false,
            emit_mulhi: false,
            emit_bitwise_binops: false,
            emit_or: false,
            emit_xor: false,
            emit_prmt: false,
            emit_not: false,
            emit_clz: false,
            emit_brev: false,
            emit_cnot: false,
            emit_abs: false,
            emit_signed_cmp: false,
            emit_signed_divrem: false,
            emit_funnel: false,
            emit_neg: false,
            emit_shl: false,
            emit_shr: false,
            emit_signed_shr: false,
            emit_bfind: false,
            emit_bfi: false,
            emit_bmsk: false,
            emit_wide_bfe: false,
            emit_wide_bfi: false,
            emit_mad24: false,
            emit_mul24: false,
            emit_mul_wide: false,
            emit_wide_int: false,
            emit_addc: false,
            emit_subc: false,
            emit_i32_boundary_immediates: false,
            emit_dp2a: true,
            emit_set: false,
            emit_s32_slct: false,
            emit_f32_slct: false,
            emit_wide_slct: false,
            emit_f64_slct: false,
            emit_video: true,
            emit_signed_video: false,
            emit_video_sat: false,
            emit_vsub4: false,
            ..GenConfig::default()
        }
    }

    fn dot_video_focused_mnemonics() -> Vec<&'static str> {
        let mut mnemonics = Vec::new();
        for group in [
            CVT_MNEMONICS,
            BFE_MNEMONICS,
            &["div.u32", "rem.u32"],
            SAD_MNEMONICS,
            POST_KNOWN_SLCT_MNEMONICS,
            DP4A_MNEMONICS,
            DP2A_MNEMONICS,
            POST_KNOWN_VIDEO_MNEMONICS,
        ] {
            mnemonics.extend_from_slice(group);
        }
        mnemonics.push("popc.b32");
        mnemonics
    }

    #[test]
    fn empty_bytes_does_not_panic() {
        // `arbitrary` is happy to keep handing back default values when out of
        // entropy, so empty input is still a valid generator run. We just want
        // to make sure it doesn't crash.
        let _ = generate_from_bytes(&[]);
    }

    #[test]
    fn deterministic() {
        let bytes: Vec<u8> = (0..4096u32).map(|i| (i ^ (i >> 3)) as u8).collect();
        let a = generate_from_bytes(&bytes).unwrap();
        let b = generate_from_bytes(&bytes).unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn emitted_ptx_has_kernel_skeleton() {
        let bytes: Vec<u8> = (0..2048u32).map(|i| i as u8).collect();
        let ptx = generate_from_bytes(&bytes).unwrap();
        assert!(ptx.contains(".version 8.8"));
        assert!(ptx.contains(".target sm_103"));
        assert!(ptx.contains(&format!(".visible .entry {KERNEL_NAME}")));
        assert!(ptx.contains("ret;"));
        assert!(ptx.contains("ld.param.u64"));
        assert!(ptx.contains("st.global.u32"));
    }

    #[test]
    fn default_profile_covers_broad_instruction_surface() {
        let cfg = coverage_heavy_config();
        let mnemonics = default_profile_mnemonics();

        assert_mnemonic_coverage(&cfg, 32768, 4096, &mnemonics);
    }

    #[test]
    fn fallback_profiles_cover_mad_and_24bit_instruction_forms() {
        let mad_cfg = GenConfig {
            emit_lop3: false,
            ..coverage_heavy_config()
        };
        assert_mnemonic_coverage(&mad_cfg, 32768, 256, MAD_LO_MNEMONICS);

        let mad24_cfg = GenConfig {
            emit_addc: false,
            emit_subc: false,
            emit_bfind: false,
            emit_mul24: false,
            ..coverage_heavy_config()
        };
        assert_mnemonic_coverage(&mad24_cfg, 32768, 1024, MAD24_MNEMONICS);

        let mul24_cfg = GenConfig {
            emit_addc: false,
            emit_subc: false,
            emit_bfind: false,
            emit_mad24: false,
            ..coverage_heavy_config()
        };
        assert_mnemonic_coverage(&mul24_cfg, 32768, 1024, MUL24_MNEMONICS);
    }

    #[test]
    fn post_known_bug_suppression_profile_still_covers_remaining_instructions() {
        let cfg = post_known_bug_suppression_config();
        let mnemonics = post_known_bug_suppression_mnemonics();

        assert_mnemonic_coverage(&cfg, 32768, 4096, &mnemonics);
    }

    #[test]
    fn dot_video_focused_profile_still_covers_targeted_instructions() {
        let cfg = dot_video_focused_config();
        let mnemonics = dot_video_focused_mnemonics();

        assert_mnemonic_coverage(&cfg, 131072, 4096, &mnemonics);
    }

    #[test]
    fn structured_mode_does_not_emit_arbitrary_block_graph() {
        let bytes = bytes_from_seed(0x1234, 4096);
        let cfg = GenConfig {
            control_flow: ControlFlowMode::Structured,
            ..GenConfig::default()
        };
        let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
        assert!(!ptx.contains("block_"));
        assert!(ptx.contains("bra             exit;"));
    }

    #[test]
    fn structured_depth_zero_emits_only_basic_sequence() {
        let bytes = bytes_from_seed(0x1234, 4096);
        let cfg = GenConfig {
            control_flow: ControlFlowMode::Structured,
            max_structured_depth: 0,
            ..GenConfig::default()
        };
        let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
        assert!(!ptx.contains("structured_loop_"));
        assert!(!ptx.contains("structured_if_"));
        assert!(ptx.contains("bra             exit;"));
    }

    #[test]
    fn structured_loop_generation_can_be_disabled() {
        let cfg = GenConfig {
            control_flow: ControlFlowMode::Structured,
            emit_structured_loops: false,
            ..GenConfig::default()
        };

        let mut saw_if = false;
        for seed in 0..256 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            assert!(
                !ptx.contains("structured_loop_"),
                "seed {seed:x} emitted structured loop"
            );
            saw_if |= ptx.contains("structured_if_");
        }
        assert!(
            saw_if,
            "structured if/else coverage was unexpectedly absent"
        );
    }

    #[test]
    fn arbitrary_loop_generation_can_be_disabled() {
        let cfg = GenConfig {
            control_flow: ControlFlowMode::Arbitrary,
            emit_arbitrary_loops: false,
            ..GenConfig::default()
        };

        let mut saw_conditional_branch = false;
        for seed in 0..256 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            assert!(
                !ptx.contains("loop_done_"),
                "seed {seed:x} emitted arbitrary backedge loop"
            );
            saw_conditional_branch |= ptx.contains("@%p");
        }
        assert!(
            saw_conditional_branch,
            "arbitrary conditional branch coverage was unexpectedly absent"
        );
    }

    #[test]
    fn lop3_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_lop3: false,
            ..GenConfig::default()
        };

        for seed in 0..256 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            assert!(!ptx.contains("lop3.b32"), "seed {seed:x} emitted lop3");
        }
    }

    #[test]
    fn predicated_lop3_generation_is_reachable() {
        let cfg = coverage_heavy_config();
        let mut saw_predicated_lop3 = false;

        for seed in 0..1024 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            saw_predicated_lop3 |= has_predicated_lop3(&ptx);
            if saw_predicated_lop3 {
                break;
            }
        }

        assert!(
            saw_predicated_lop3,
            "no seed in sample emitted predicated lop3"
        );
    }

    #[test]
    fn predicated_lop3_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_predicated_lop3: false,
            ..GenConfig::default()
        };

        for seed in 0..512 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            assert!(
                !has_predicated_lop3(&ptx),
                "seed {seed:x} emitted predicated lop3"
            );
        }
    }

    #[test]
    fn minmax_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_minmax: false,
            ..GenConfig::default()
        };

        for seed in 0..256 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            for mnemonic in ["min.u32", "max.u32", "min.s32", "max.s32"] {
                assert!(!ptx.contains(mnemonic), "seed {seed:x} emitted {mnemonic}");
            }
        }
    }

    #[test]
    fn sub_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_sub: false,
            emit_structured_loops: false,
            emit_arbitrary_loops: false,
            ..GenConfig::default()
        };

        for seed in 0..256 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            assert!(!ptx.contains("sub.u32"), "seed {seed:x} emitted sub.u32");
            assert!(!ptx.contains("sub.s32"), "seed {seed:x} emitted sub.s32");
            assert!(
                !ptx.contains("sub.sat.s32"),
                "seed {seed:x} emitted sub.sat.s32"
            );
        }
    }

    #[test]
    fn mul_lo_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_mul_lo: false,
            ..GenConfig::default()
        };

        for seed in 0..256 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            assert!(
                !ptx.contains("mul.lo.u32"),
                "seed {seed:x} emitted mul.lo.u32"
            );
            assert!(
                !ptx.contains("mad.lo.u32"),
                "seed {seed:x} emitted mad.lo.u32"
            );
            assert!(
                !ptx.contains("mul.lo.s32"),
                "seed {seed:x} emitted mul.lo.s32"
            );
            assert!(
                !ptx.contains("mad.lo.s32"),
                "seed {seed:x} emitted mad.lo.s32"
            );
        }
    }

    #[test]
    fn signed_low_alu_generation_is_reachable() {
        let cfg = coverage_heavy_config();
        assert_mnemonic_coverage(&cfg, 32768, 2048, SIGNED_LO_BIN_MNEMONICS);

        let mad_cfg = GenConfig {
            emit_lop3: false,
            ..coverage_heavy_config()
        };
        assert_mnemonic_coverage(&mad_cfg, 32768, 256, &["mad.lo.s32"]);
    }

    #[test]
    fn signed_low_alu_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_signed_lo_alu: false,
            emit_lop3: false,
            ..GenConfig::default()
        };

        for seed in 0..512 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            for mnemonic in [
                "add.s32",
                "add.sat.s32",
                "sub.s32",
                "sub.sat.s32",
                "mul.lo.s32",
                "mad.lo.s32",
            ] {
                assert!(!ptx.contains(mnemonic), "seed {seed:x} emitted {mnemonic}");
            }
        }
    }

    #[test]
    fn sat_arith_generation_is_reachable() {
        let cfg = coverage_heavy_config();
        assert_mnemonic_coverage(&cfg, 32768, 2048, SAT_ARITH_MNEMONICS);
    }

    #[test]
    fn sat_arith_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_sat_arith: false,
            ..GenConfig::default()
        };

        for seed in 0..512 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            for mnemonic in SAT_ARITH_MNEMONICS {
                assert!(!ptx.contains(mnemonic), "seed {seed:x} emitted {mnemonic}");
            }
        }
    }

    #[test]
    fn packed_add_generation_is_reachable() {
        let cfg = coverage_heavy_config();
        assert_mnemonic_coverage(&cfg, 32768, 2048, PACKED_ADD_MNEMONICS);
    }

    #[test]
    fn packed_add_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_packed_add: false,
            ..GenConfig::default()
        };

        for seed in 0..512 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            for mnemonic in PACKED_ADD_MNEMONICS {
                assert!(!ptx.contains(mnemonic), "seed {seed:x} emitted {mnemonic}");
            }
        }
    }

    #[test]
    fn signed_packed_add_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_signed_packed_add: false,
            ..coverage_heavy_config()
        };

        let mut saw_unsigned = false;
        for seed in 0..1024 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            for mnemonic in SIGNED_PACKED_ADD_MNEMONICS {
                assert!(!ptx.contains(mnemonic), "seed {seed:x} emitted {mnemonic}");
            }
            saw_unsigned |= has_mnemonic(&ptx, "add.u16x2");
        }
        assert!(saw_unsigned, "sample did not retain add.u16x2 coverage");
    }

    #[test]
    fn predicated_packed_add_generation_is_reachable() {
        let cfg = coverage_heavy_config();
        let mut found = vec![false; PACKED_ADD_MNEMONICS.len()];

        for seed in 0..4096 {
            let bytes = bytes_from_seed(seed, 32768);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            for op in ptx.lines().filter_map(predicated_mnemonic) {
                for (i, mnemonic) in PACKED_ADD_MNEMONICS.iter().enumerate() {
                    found[i] |= op == *mnemonic;
                }
            }
            if found.iter().all(|seen| *seen) {
                break;
            }
        }

        let missing: Vec<_> = PACKED_ADD_MNEMONICS
            .iter()
            .zip(found)
            .filter_map(|(mnemonic, seen)| (!seen).then_some(*mnemonic))
            .collect();
        assert!(
            missing.is_empty(),
            "sample did not emit predicated {missing:?}"
        );
    }

    #[test]
    fn predicated_packed_add_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_predicated_packed_add: false,
            ..coverage_heavy_config()
        };

        for seed in 0..1024 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            for op in ptx.lines().filter_map(predicated_mnemonic) {
                assert!(
                    !PACKED_ADD_MNEMONICS.contains(&op),
                    "seed {seed:x} emitted predicated {op}"
                );
            }
        }
    }

    #[test]
    fn packed_minmax_generation_is_reachable() {
        let cfg = coverage_heavy_config();
        assert_mnemonic_coverage(&cfg, 32768, 4096, PACKED_MINMAX_MNEMONICS);
    }

    #[test]
    fn packed_minmax_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_packed_minmax: false,
            ..GenConfig::default()
        };

        for seed in 0..512 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            for mnemonic in PACKED_MINMAX_MNEMONICS {
                assert!(!ptx.contains(mnemonic), "seed {seed:x} emitted {mnemonic}");
            }
        }
    }

    #[test]
    fn signed_packed_minmax_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_signed_packed_minmax: false,
            ..coverage_heavy_config()
        };

        let mut saw_unsigned = false;
        for seed in 0..2048 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            for mnemonic in SIGNED_PACKED_MINMAX_MNEMONICS {
                assert!(!ptx.contains(mnemonic), "seed {seed:x} emitted {mnemonic}");
            }
            saw_unsigned |= has_mnemonic(&ptx, "min.u16x2") || has_mnemonic(&ptx, "max.u16x2");
        }
        assert!(
            saw_unsigned,
            "sample did not retain unsigned packed min/max coverage"
        );
    }

    #[test]
    fn predicated_packed_minmax_generation_is_reachable() {
        let cfg = coverage_heavy_config();
        let mut found = vec![false; PACKED_MINMAX_MNEMONICS.len()];

        for seed in 0..8192 {
            let bytes = bytes_from_seed(seed, 32768);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            for op in ptx.lines().filter_map(predicated_mnemonic) {
                for (i, mnemonic) in PACKED_MINMAX_MNEMONICS.iter().enumerate() {
                    found[i] |= op == *mnemonic;
                }
            }
            if found.iter().all(|seen| *seen) {
                break;
            }
        }

        let missing: Vec<_> = PACKED_MINMAX_MNEMONICS
            .iter()
            .zip(found)
            .filter_map(|(mnemonic, seen)| (!seen).then_some(*mnemonic))
            .collect();
        assert!(
            missing.is_empty(),
            "sample did not emit predicated {missing:?}"
        );
    }

    #[test]
    fn predicated_packed_minmax_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_predicated_packed_minmax: false,
            ..coverage_heavy_config()
        };

        for seed in 0..2048 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            for op in ptx.lines().filter_map(predicated_mnemonic) {
                assert!(
                    !PACKED_MINMAX_MNEMONICS.contains(&op),
                    "seed {seed:x} emitted predicated {op}"
                );
            }
        }
    }

    #[test]
    fn scalar_16bit_generation_is_reachable() {
        let cfg = coverage_heavy_config();
        assert_mnemonic_coverage(&cfg, 32768, 8192, SCALAR_16BIT_MNEMONICS);
    }

    #[test]
    fn scalar_16bit_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_scalar_16bit: false,
            ..GenConfig::default()
        };

        for seed in 0..1024 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            for mnemonic in SCALAR_16BIT_MNEMONICS
                .iter()
                .chain(SCALAR_16BIT_COMPARE_MNEMONICS)
                .chain(SCALAR_16BIT_SELP_MNEMONICS)
            {
                assert!(
                    !has_mnemonic(&ptx, *mnemonic),
                    "seed {seed:x} emitted {mnemonic}"
                );
            }
        }
    }

    #[test]
    fn signed_scalar_16bit_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_signed_scalar_16bit: false,
            ..coverage_heavy_config()
        };

        let mut saw_unsigned = false;
        for seed in 0..2048 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            for mnemonic in SIGNED_SCALAR_16BIT_MNEMONICS {
                assert!(
                    !has_mnemonic(&ptx, mnemonic),
                    "seed {seed:x} emitted {mnemonic}"
                );
            }
            saw_unsigned |= has_mnemonic(&ptx, "add.u16")
                || has_mnemonic(&ptx, "sub.u16")
                || has_mnemonic(&ptx, "mul.lo.u16")
                || has_mnemonic(&ptx, "setp.lt.u16")
                || has_mnemonic(&ptx, "selp.u16");
        }
        assert!(
            saw_unsigned,
            "sample did not retain unsigned scalar 16-bit coverage"
        );
    }

    #[test]
    fn scalar_16bit_signed_unary_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_scalar_16bit_signed_unary: false,
            ..coverage_heavy_config()
        };

        let mut saw_other_signed = false;
        for seed in 0..2048 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            assert!(
                !has_mnemonic(&ptx, "abs.s16"),
                "seed {seed:x} emitted abs.s16"
            );
            assert!(
                !has_mnemonic(&ptx, "neg.s16"),
                "seed {seed:x} emitted neg.s16"
            );
            saw_other_signed |= has_mnemonic(&ptx, "add.s16") || has_mnemonic(&ptx, "mul.lo.s16");
        }
        assert!(
            saw_other_signed,
            "sample did not retain other signed scalar 16-bit coverage"
        );
    }

    #[test]
    fn scalar_16bit_minmax_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_scalar_16bit_min: false,
            ..coverage_heavy_config()
        };

        for seed in 0..2048 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            assert!(
                !has_mnemonic(&ptx, "min.u16"),
                "seed {seed:x} emitted min.u16"
            );
            assert!(
                !has_mnemonic(&ptx, "min.s16"),
                "seed {seed:x} emitted min.s16"
            );
            assert!(
                !has_mnemonic(&ptx, "max.u16"),
                "seed {seed:x} emitted max.u16"
            );
            assert!(
                !has_mnemonic(&ptx, "max.s16"),
                "seed {seed:x} emitted max.s16"
            );
        }
    }

    #[test]
    fn scalar_16bit_bitwise_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_scalar_16bit_bitwise: false,
            ..coverage_heavy_config()
        };

        let mut saw_arithmetic = false;
        for seed in 0..2048 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            for mnemonic in SCALAR_16BIT_BITWISE_MNEMONICS {
                assert!(
                    !has_mnemonic(&ptx, mnemonic),
                    "seed {seed:x} emitted {mnemonic}"
                );
            }
            saw_arithmetic |= has_mnemonic(&ptx, "add.u16") || has_mnemonic(&ptx, "mul.lo.s16");
        }
        assert!(
            saw_arithmetic,
            "sample did not retain scalar 16-bit arithmetic coverage"
        );
    }

    #[test]
    fn scalar_16bit_shift_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_scalar_16bit_shifts: false,
            ..coverage_heavy_config()
        };

        let mut saw_arithmetic = false;
        for seed in 0..2048 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            for mnemonic in SCALAR_16BIT_SHIFT_MNEMONICS {
                assert!(
                    !has_mnemonic(&ptx, mnemonic),
                    "seed {seed:x} emitted {mnemonic}"
                );
            }
            saw_arithmetic |= has_mnemonic(&ptx, "add.u16") || has_mnemonic(&ptx, "mul.lo.s16");
        }
        assert!(
            saw_arithmetic,
            "sample did not retain scalar 16-bit arithmetic coverage"
        );
    }

    #[test]
    fn scalar_16bit_compare_generation_is_reachable() {
        let cfg = coverage_heavy_config();
        assert_mnemonic_coverage(&cfg, 32768, 8192, SCALAR_16BIT_COMPARE_MNEMONICS);
    }

    #[test]
    fn scalar_16bit_compare_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_scalar_16bit_compare: false,
            ..coverage_heavy_config()
        };

        let mut saw_arithmetic = false;
        for seed in 0..2048 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            for mnemonic in SCALAR_16BIT_COMPARE_MNEMONICS
                .iter()
                .chain(SCALAR_16BIT_SELP_MNEMONICS)
            {
                assert!(
                    !has_mnemonic(&ptx, *mnemonic),
                    "seed {seed:x} emitted {mnemonic}"
                );
            }
            saw_arithmetic |= has_mnemonic(&ptx, "add.u16") || has_mnemonic(&ptx, "mul.lo.s16");
        }
        assert!(
            saw_arithmetic,
            "sample did not retain scalar 16-bit arithmetic coverage"
        );
    }

    #[test]
    fn scalar_16bit_selp_generation_is_reachable() {
        let cfg = coverage_heavy_config();
        assert_mnemonic_coverage(&cfg, 32768, 4096, SCALAR_16BIT_SELP_MNEMONICS);
    }

    #[test]
    fn scalar_16bit_selp_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_scalar_16bit_selp: false,
            ..coverage_heavy_config()
        };

        let mut saw_compare = false;
        for seed in 0..2048 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            for mnemonic in SCALAR_16BIT_SELP_MNEMONICS {
                assert!(
                    !has_mnemonic(&ptx, mnemonic),
                    "seed {seed:x} emitted {mnemonic}"
                );
            }
            saw_compare |=
                has_mnemonic(&ptx, "setp.lt.u16") || has_mnemonic(&ptx, "set.lt.u32.s16");
        }
        assert!(
            saw_compare,
            "sample did not retain scalar 16-bit compare coverage"
        );
    }

    #[test]
    fn predicated_scalar_16bit_generation_is_reachable() {
        let cfg = coverage_heavy_config();
        let mut found = vec![false; SCALAR_16BIT_MNEMONICS.len()];

        for seed in 0..8192 {
            let bytes = bytes_from_seed(seed, 32768);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            for op in ptx.lines().filter_map(predicated_mnemonic) {
                for (i, mnemonic) in SCALAR_16BIT_MNEMONICS.iter().enumerate() {
                    found[i] |= op == *mnemonic;
                }
            }
            if found.iter().all(|seen| *seen) {
                break;
            }
        }

        let missing: Vec<_> = SCALAR_16BIT_MNEMONICS
            .iter()
            .zip(found)
            .filter_map(|(mnemonic, seen)| (!seen).then_some(*mnemonic))
            .collect();
        assert!(
            missing.is_empty(),
            "sample did not emit predicated {missing:?}"
        );
    }

    #[test]
    fn predicated_scalar_16bit_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_predicated_scalar_16bit: false,
            ..coverage_heavy_config()
        };

        for seed in 0..2048 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            for op in ptx.lines().filter_map(predicated_mnemonic) {
                assert!(
                    !SCALAR_16BIT_MNEMONICS.contains(&op),
                    "seed {seed:x} emitted predicated {op}"
                );
            }
        }
    }

    #[test]
    fn predicated_mad_generation_is_reachable() {
        let cfg = GenConfig {
            emit_lop3: false,
            ..coverage_heavy_config()
        };
        let mut found = vec![false; MAD_LO_MNEMONICS.len()];

        for seed in 0..2048 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            for op in ptx.lines().filter_map(predicated_mnemonic) {
                for (i, mnemonic) in MAD_LO_MNEMONICS.iter().enumerate() {
                    found[i] |= op == *mnemonic;
                }
            }
            if found.iter().all(|seen| *seen) {
                break;
            }
        }

        let missing: Vec<_> = MAD_LO_MNEMONICS
            .iter()
            .zip(found)
            .filter_map(|(mnemonic, seen)| (!seen).then_some(*mnemonic))
            .collect();
        assert!(
            missing.is_empty(),
            "sample did not emit predicated {missing:?}"
        );
    }

    #[test]
    fn predicated_mad_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_lop3: false,
            emit_predicated_mad: false,
            ..GenConfig::default()
        };

        for seed in 0..512 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            assert!(
                !has_predicated_mad(&ptx),
                "seed {seed:x} emitted predicated mad"
            );
        }
    }

    #[test]
    fn mad_hi_generation_is_reachable() {
        let cfg = GenConfig {
            emit_lop3: false,
            emit_mul_lo: false,
            ..coverage_heavy_config()
        };
        assert_mnemonic_coverage(&cfg, 32768, 1024, MAD_HI_MNEMONICS);
    }

    #[test]
    fn mad_hi_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_lop3: false,
            emit_mul_lo: false,
            emit_mad_hi: false,
            ..GenConfig::default()
        };

        for seed in 0..512 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            for mnemonic in MAD_HI_MNEMONICS {
                assert!(!ptx.contains(mnemonic), "seed {seed:x} emitted {mnemonic}");
            }
        }
    }

    #[test]
    fn signed_mad_hi_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_lop3: false,
            emit_mul_lo: false,
            emit_signed_mad_hi: false,
            ..GenConfig::default()
        };

        let mut saw_unsigned = false;
        for seed in 0..1024 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            assert!(
                !ptx.contains("mad.hi.s32"),
                "seed {seed:x} emitted mad.hi.s32"
            );
            saw_unsigned |= ptx.contains("mad.hi.u32");
        }
        assert!(saw_unsigned, "sample did not retain mad.hi.u32 coverage");
    }

    #[test]
    fn predicated_mad_hi_generation_is_reachable() {
        let cfg = GenConfig {
            emit_lop3: false,
            emit_mul_lo: false,
            ..coverage_heavy_config()
        };
        let mut found = vec![false; MAD_HI_MNEMONICS.len()];

        for seed in 0..2048 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            for op in ptx.lines().filter_map(predicated_mnemonic) {
                for (i, mnemonic) in MAD_HI_MNEMONICS.iter().enumerate() {
                    found[i] |= op == *mnemonic;
                }
            }
            if found.iter().all(|seen| *seen) {
                break;
            }
        }

        let missing: Vec<_> = MAD_HI_MNEMONICS
            .iter()
            .zip(found)
            .filter_map(|(mnemonic, seen)| (!seen).then_some(*mnemonic))
            .collect();
        assert!(
            missing.is_empty(),
            "sample did not emit predicated {missing:?}"
        );
    }

    #[test]
    fn predicated_mad_hi_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_lop3: false,
            emit_mul_lo: false,
            emit_predicated_mad_hi: false,
            ..GenConfig::default()
        };

        for seed in 0..512 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            for op in ptx.lines().filter_map(predicated_mnemonic) {
                assert!(
                    !MAD_HI_MNEMONICS.contains(&op),
                    "seed {seed:x} emitted predicated {op}"
                );
            }
        }
    }

    #[test]
    fn mad_carry_generation_is_reachable() {
        let cfg = GenConfig {
            emit_lop3: false,
            emit_bfind: false,
            emit_fns: false,
            emit_addc: false,
            emit_subc: false,
            emit_mad24: false,
            emit_mul24: false,
            emit_subword_wide: false,
            emit_predicated_mad_carry: false,
            ..coverage_heavy_config()
        };
        assert_mnemonic_coverage(&cfg, 32768, 8192, MAD_CARRY_MNEMONICS);
    }

    #[test]
    fn mad_carry_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_mad_carry: false,
            ..coverage_heavy_config()
        };

        for seed in 0..2048 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            for mnemonic in MAD_CARRY_MNEMONICS {
                assert!(
                    !has_mnemonic(&ptx, mnemonic),
                    "seed {seed:x} emitted {mnemonic}"
                );
            }
        }
    }

    #[test]
    fn signed_mad_carry_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_signed_mad_carry: false,
            ..coverage_heavy_config()
        };

        let mut saw_unsigned = false;
        for seed in 0..4096 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            for mnemonic in SIGNED_MAD_CARRY_MNEMONICS {
                assert!(
                    !has_mnemonic(&ptx, mnemonic),
                    "seed {seed:x} emitted {mnemonic}"
                );
            }
            saw_unsigned |=
                has_mnemonic(&ptx, "mad.lo.cc.u32") || has_mnemonic(&ptx, "mad.hi.cc.u32");
        }
        assert!(
            saw_unsigned,
            "sample did not retain unsigned mad carry coverage"
        );
    }

    #[test]
    fn predicated_mad_carry_generation_is_reachable() {
        let cfg = GenConfig {
            emit_lop3: false,
            emit_bfind: false,
            emit_fns: false,
            emit_addc: false,
            emit_subc: false,
            emit_mad24: false,
            emit_mul24: false,
            emit_subword_wide: false,
            ..coverage_heavy_config()
        };

        for seed in 0..8192 {
            let bytes = bytes_from_seed(seed, 32768);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            if has_predicated_mad_carry(&ptx) {
                return;
            }
        }

        panic!("sample did not emit predicated mad carry chain");
    }

    #[test]
    fn predicated_mad_carry_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_predicated_mad_carry: false,
            ..coverage_heavy_config()
        };

        for seed in 0..2048 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            assert!(
                !has_predicated_mad_carry(&ptx),
                "seed {seed:x} emitted predicated mad carry chain"
            );
        }
    }

    #[test]
    fn mulhi_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_mulhi: false,
            ..GenConfig::default()
        };

        for seed in 0..256 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            for mnemonic in ["mul.hi.u32", "mul.hi.s32"] {
                assert!(!ptx.contains(mnemonic), "seed {seed:x} emitted {mnemonic}");
            }
        }
    }

    #[test]
    fn signed_mulhi_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_signed_mulhi: false,
            ..GenConfig::default()
        };

        let mut saw_unsigned = false;
        for seed in 0..512 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            assert!(
                !ptx.contains("mul.hi.s32"),
                "seed {seed:x} emitted mul.hi.s32"
            );
            saw_unsigned |= ptx.contains("mul.hi.u32");
        }
        assert!(saw_unsigned, "sample did not retain mul.hi.u32 coverage");
    }

    #[test]
    fn bitwise_binop_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_bitwise_binops: false,
            ..GenConfig::default()
        };

        for seed in 0..256 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            for mnemonic in ["and.b32", "or.b32", "xor.b32"] {
                assert!(!ptx.contains(mnemonic), "seed {seed:x} emitted {mnemonic}");
            }
        }
    }

    #[test]
    fn xor_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_xor: false,
            ..GenConfig::default()
        };

        let mut saw_and = false;
        let mut saw_or = false;
        for seed in 0..512 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            assert!(
                !has_mnemonic(&ptx, "xor.b32"),
                "seed {seed:x} emitted xor.b32"
            );
            saw_and |= has_mnemonic(&ptx, "and.b32");
            saw_or |= has_mnemonic(&ptx, "or.b32");
        }
        assert!(saw_and, "sample did not retain and.b32 coverage");
        assert!(saw_or, "sample did not retain or.b32 coverage");
    }

    #[test]
    fn or_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_or: false,
            ..GenConfig::default()
        };

        let mut saw_and = false;
        let mut saw_xor = false;
        for seed in 0..512 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            assert!(
                !has_mnemonic(&ptx, "or.b32"),
                "seed {seed:x} emitted or.b32"
            );
            saw_and |= has_mnemonic(&ptx, "and.b32");
            saw_xor |= has_mnemonic(&ptx, "xor.b32");
        }
        assert!(saw_and, "sample did not retain and.b32 coverage");
        assert!(saw_xor, "sample did not retain xor.b32 coverage");
    }

    #[test]
    fn prmt_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_prmt: false,
            ..GenConfig::default()
        };

        for seed in 0..256 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            assert!(!ptx.contains("prmt.b32"), "seed {seed:x} emitted prmt");
        }
    }

    #[test]
    fn predicated_prmt_generation_is_reachable() {
        let cfg = coverage_heavy_config();
        let mut saw_predicated_prmt = false;

        for seed in 0..1024 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            saw_predicated_prmt |= has_predicated_prmt(&ptx);
            if saw_predicated_prmt {
                break;
            }
        }

        assert!(
            saw_predicated_prmt,
            "no seed in sample emitted predicated prmt"
        );
    }

    #[test]
    fn predicated_prmt_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_predicated_prmt: false,
            ..GenConfig::default()
        };

        for seed in 0..512 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            assert!(
                !has_predicated_prmt(&ptx),
                "seed {seed:x} emitted predicated prmt"
            );
        }
    }

    #[test]
    fn register_prmt_generation_is_reachable() {
        let cfg = GenConfig {
            emit_predicated_prmt: false,
            ..coverage_heavy_config()
        };

        for seed in 0..4096 {
            let bytes = bytes_from_seed(seed, 32768);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            if has_register_prmt_control(&ptx) {
                return;
            }
        }

        panic!("sample did not emit register-control prmt.b32");
    }

    #[test]
    fn register_prmt_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_reg_prmt: false,
            ..coverage_heavy_config()
        };

        for seed in 0..1024 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            assert!(
                !has_register_prmt_control(&ptx),
                "seed {seed:x} emitted register-control prmt.b32"
            );
        }
    }

    #[test]
    fn predicated_register_prmt_generation_is_reachable() {
        let cfg = coverage_heavy_config();

        for seed in 0..4096 {
            let bytes = bytes_from_seed(seed, 32768);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            if has_predicated_register_prmt_control(&ptx) {
                return;
            }
        }

        panic!("sample did not emit predicated register-control prmt.b32");
    }

    #[test]
    fn predicated_register_prmt_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_predicated_reg_prmt: false,
            ..coverage_heavy_config()
        };

        for seed in 0..1024 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            assert!(
                !has_predicated_register_prmt_control(&ptx),
                "seed {seed:x} emitted predicated register-control prmt.b32"
            );
        }
    }

    #[test]
    fn prmt_mode_generation_is_reachable() {
        let cfg = coverage_heavy_config();
        assert_mnemonic_coverage(&cfg, 32768, 4096, PRMT_MODE_MNEMONICS);
    }

    #[test]
    fn prmt_mode_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_prmt_modes: false,
            ..coverage_heavy_config()
        };

        let mut saw_generic_prmt = false;
        for seed in 0..1024 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            for mnemonic in PRMT_MODE_MNEMONICS {
                assert!(
                    !has_mnemonic(&ptx, mnemonic),
                    "seed {seed:x} emitted {mnemonic}"
                );
            }
            saw_generic_prmt |= has_mnemonic(&ptx, "prmt.b32");
        }
        assert!(
            saw_generic_prmt,
            "sample did not retain generic prmt.b32 coverage"
        );
    }

    #[test]
    fn direct_prmt_cvt_dependencies_are_suppressed() {
        let cfg = coverage_heavy_config();

        for seed in 0..2048 {
            let bytes = bytes_from_seed(seed, 32768);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            assert!(
                !has_direct_prmt_cvt_dependency(&ptx),
                "seed {seed:x} emitted direct prmt-to-cvt dependency"
            );
        }
    }

    #[test]
    fn not_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_not: false,
            ..GenConfig::default()
        };

        for seed in 0..256 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            assert!(
                !ptx.lines()
                    .any(|line| line.trim_start().starts_with("not.b32")),
                "seed {seed:x} emitted not.b32"
            );
            assert!(
                !ptx.lines().any(|line| {
                    let line = line.trim_start();
                    line.starts_with("xor.b32") && line.contains("4294967295")
                }),
                "seed {seed:x} emitted xor.b32 with 0xffffffff"
            );
        }
    }

    #[test]
    fn not_suppression_sanitizes_xor_all_ones() {
        assert_eq!(
            sanitize_xor_not_operand(Operand::Imm(0xFFFF_FFFF)),
            Operand::Imm(0xFFFF_FFFE)
        );
        assert_eq!(
            sanitize_xor_not_operand(Operand::Imm(0xFFFF_FFFD)),
            Operand::Imm(0xFFFF_FFFD)
        );
        assert_eq!(sanitize_xor_not_operand(Operand::Reg(3)), Operand::Reg(3));
    }

    #[test]
    fn clz_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_clz: false,
            ..GenConfig::default()
        };

        for seed in 0..256 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            assert!(!ptx.contains("clz.b32"), "seed {seed:x} emitted clz.b32");
        }
    }

    #[test]
    fn brev_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_brev: false,
            ..GenConfig::default()
        };

        for seed in 0..256 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            assert!(!ptx.contains("brev.b32"), "seed {seed:x} emitted brev.b32");
        }
    }

    #[test]
    fn cnot_generation_is_reachable() {
        let mut saw_cnot = false;
        for seed in 0..1024 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes(&bytes).unwrap();
            if ptx.contains("cnot.b32") {
                saw_cnot = true;
                break;
            }
        }
        assert!(saw_cnot, "no seed in sample emitted cnot.b32");
    }

    #[test]
    fn cnot_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_cnot: false,
            ..GenConfig::default()
        };

        for seed in 0..256 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            assert!(!ptx.contains("cnot.b32"), "seed {seed:x} emitted cnot.b32");
        }
    }

    #[test]
    fn popc_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_popc: false,
            ..GenConfig::default()
        };

        for seed in 0..256 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            assert!(!ptx.contains("popc.b32"), "seed {seed:x} emitted popc.b32");
        }
    }

    #[test]
    fn all_unary_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_not: false,
            emit_clz: false,
            emit_brev: false,
            emit_neg: false,
            emit_cnot: false,
            emit_popc: false,
            emit_abs: false,
            ..GenConfig::default()
        };

        for seed in 0..256 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            for mnemonic in UNARY_MNEMONICS {
                assert!(!ptx.contains(mnemonic), "seed {seed:x} emitted {mnemonic}");
            }
        }
    }

    #[test]
    fn abs_generation_is_reachable() {
        let mut saw_abs = false;
        for seed in 0..1024 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes(&bytes).unwrap();
            if ptx.contains("abs.s32") {
                saw_abs = true;
                break;
            }
        }
        assert!(saw_abs, "no seed in sample emitted abs.s32");
    }

    #[test]
    fn abs_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_abs: false,
            ..GenConfig::default()
        };

        for seed in 0..256 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            assert!(!ptx.contains("abs.s32"), "seed {seed:x} emitted abs.s32");
        }
    }

    #[test]
    fn global_load_generation_is_reachable() {
        let cfg = coverage_heavy_config();
        let mut found = vec![false; GLOBAL_LOAD_MNEMONICS.len()];

        for seed in 0..2048 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            for (i, mnemonic) in GLOBAL_LOAD_MNEMONICS.iter().enumerate() {
                found[i] |= has_body_global_load(&ptx, mnemonic);
            }
            if found.iter().all(|seen| *seen) {
                break;
            }
        }

        let missing: Vec<_> = GLOBAL_LOAD_MNEMONICS
            .iter()
            .zip(found)
            .filter_map(|(mnemonic, seen)| (!seen).then_some(*mnemonic))
            .collect();
        assert!(
            missing.is_empty(),
            "sample did not emit body global loads {missing:?}"
        );
    }

    #[test]
    fn global_load_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_global_loads: false,
            ..coverage_heavy_config()
        };

        for seed in 0..1024 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            for mnemonic in GLOBAL_LOAD_MNEMONICS {
                assert!(
                    !has_body_global_load(&ptx, mnemonic),
                    "seed {seed:x} emitted body {mnemonic}"
                );
            }
            for mnemonic in UNIFORM_GLOBAL_LOAD_MNEMONICS {
                assert!(
                    !has_body_global_load(&ptx, mnemonic),
                    "seed {seed:x} emitted body {mnemonic}"
                );
            }
            for mnemonic in UNIFORM_GLOBAL_VECTOR_LOAD_MNEMONICS {
                assert!(
                    !has_mnemonic(&ptx, mnemonic),
                    "seed {seed:x} emitted body {mnemonic}"
                );
            }
        }
    }

    #[test]
    fn uniform_global_load_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_uniform_global_loads: false,
            ..coverage_heavy_config()
        };

        for seed in 0..1024 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            for mnemonic in UNIFORM_GLOBAL_LOAD_MNEMONICS {
                assert!(
                    !has_body_global_load(&ptx, mnemonic),
                    "seed {seed:x} emitted body {mnemonic}"
                );
            }
            for mnemonic in UNIFORM_GLOBAL_VECTOR_LOAD_MNEMONICS {
                assert!(
                    !has_mnemonic(&ptx, mnemonic),
                    "seed {seed:x} emitted body {mnemonic}"
                );
            }
        }
    }

    #[test]
    fn global_load_offsets_are_bounded_and_aligned() {
        let cfg = coverage_heavy_config();

        for seed in 0..512 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            for (mnemonic, offset) in ptx.lines().filter_map(body_global_load) {
                let width = global_load_width(mnemonic).unwrap();
                assert_eq!(
                    offset % width,
                    0,
                    "seed {seed:x} emitted unaligned {mnemonic} offset {offset}"
                );
                assert!(
                    offset + width <= input_len() as u32,
                    "seed {seed:x} emitted out-of-bounds {mnemonic} offset {offset}"
                );
            }
        }
    }

    #[test]
    fn global_store_roundtrip_generation_is_reachable() {
        let cfg = coverage_heavy_config();
        let mut loads = vec![false; GLOBAL_LOAD_MNEMONICS.len()];
        let mut stores = vec![false; GLOBAL_STORE_MNEMONICS.len()];

        for seed in 0..2048 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            for (i, mnemonic) in GLOBAL_LOAD_MNEMONICS.iter().enumerate() {
                loads[i] |= has_body_global_store_roundtrip_access(&ptx, mnemonic);
            }
            for (i, mnemonic) in GLOBAL_STORE_MNEMONICS.iter().enumerate() {
                stores[i] |= has_body_global_store_roundtrip_access(&ptx, mnemonic);
            }
            if loads.iter().all(|seen| *seen) && stores.iter().all(|seen| *seen) {
                break;
            }
        }

        let missing_loads: Vec<_> = GLOBAL_LOAD_MNEMONICS
            .iter()
            .zip(loads)
            .filter_map(|(mnemonic, seen)| (!seen).then_some(*mnemonic))
            .collect();
        let missing_stores: Vec<_> = GLOBAL_STORE_MNEMONICS
            .iter()
            .zip(stores)
            .filter_map(|(mnemonic, seen)| (!seen).then_some(*mnemonic))
            .collect();
        assert!(
            missing_loads.is_empty() && missing_stores.is_empty(),
            "sample missed global roundtrip loads {missing_loads:?} stores {missing_stores:?}"
        );
    }

    #[test]
    fn global_store_roundtrip_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_global_store_roundtrips: false,
            ..coverage_heavy_config()
        };

        for seed in 0..1024 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            for mnemonic in GLOBAL_LOAD_MNEMONICS
                .iter()
                .chain(GLOBAL_STORE_MNEMONICS.iter())
            {
                assert!(
                    !has_body_global_store_roundtrip_access(&ptx, mnemonic),
                    "seed {seed:x} emitted body {mnemonic}"
                );
            }
        }
    }

    #[test]
    fn global_store_roundtrip_offsets_are_bounded_and_aligned() {
        let cfg = coverage_heavy_config();

        for seed in 0..512 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            for (mnemonic, offset) in ptx.lines().filter_map(body_global_store_roundtrip_access) {
                let width = global_load_width(mnemonic)
                    .or_else(|| global_store_width(mnemonic))
                    .unwrap();
                assert_eq!(
                    offset % width,
                    0,
                    "seed {seed:x} emitted unaligned {mnemonic} offset {offset}"
                );
                assert!(
                    offset + width <= N_OUTPUTS * 4,
                    "seed {seed:x} emitted out-of-output-slice {mnemonic} offset {offset}"
                );
            }
        }
    }

    #[test]
    fn global_memory_cache_ops_generation_is_reachable() {
        let cfg = coverage_heavy_config();
        let mut loads = vec![false; GLOBAL_LOAD_CACHE_PREFIXES.len()];
        let mut stores = vec![false; GLOBAL_STORE_CACHE_PREFIXES.len()];

        for seed in 0..8192 {
            let bytes = bytes_from_seed(seed, 8192);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            for (mnemonic, _) in ptx.lines().filter_map(body_global_load) {
                for (i, prefix) in GLOBAL_LOAD_CACHE_PREFIXES.iter().enumerate() {
                    loads[i] |= mnemonic.starts_with(prefix);
                }
            }
            for (mnemonic, _) in ptx.lines().filter_map(body_global_store_roundtrip_access) {
                for (i, prefix) in GLOBAL_STORE_CACHE_PREFIXES.iter().enumerate() {
                    stores[i] |= mnemonic.starts_with(prefix);
                }
            }
            if loads.iter().all(|seen| *seen) && stores.iter().all(|seen| *seen) {
                break;
            }
        }

        let missing_loads: Vec<_> = GLOBAL_LOAD_CACHE_PREFIXES
            .iter()
            .zip(loads)
            .filter_map(|(prefix, seen)| (!seen).then_some(*prefix))
            .collect();
        let missing_stores: Vec<_> = GLOBAL_STORE_CACHE_PREFIXES
            .iter()
            .zip(stores)
            .filter_map(|(prefix, seen)| (!seen).then_some(*prefix))
            .collect();
        assert!(
            missing_loads.is_empty() && missing_stores.is_empty(),
            "sample missed global memory cache loads {missing_loads:?} stores {missing_stores:?}"
        );
    }

    #[test]
    fn global_memory_cache_ops_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_memory_cache_ops: false,
            ..coverage_heavy_config()
        };

        for seed in 0..1024 {
            let bytes = bytes_from_seed(seed, 8192);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            for (mnemonic, _) in ptx
                .lines()
                .filter_map(body_global_load)
                .chain(ptx.lines().filter_map(body_global_store_roundtrip_access))
            {
                assert!(
                    !is_global_memory_cache_mnemonic(mnemonic),
                    "seed {seed:x} emitted body {mnemonic}"
                );
            }
            for (mnemonic, _, _) in ptx.lines().filter_map(body_vector_memory_access) {
                assert!(
                    !is_global_memory_cache_mnemonic(mnemonic),
                    "seed {seed:x} emitted body {mnemonic}"
                );
            }
        }
    }

    #[test]
    fn volatile_memory_generation_is_reachable() {
        assert_body_mnemonic_coverage(
            &coverage_heavy_config(),
            8192,
            32768,
            VOLATILE_MEMORY_MNEMONICS,
        );
    }

    #[test]
    fn volatile_memory_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_volatile_memory: false,
            ..coverage_heavy_config()
        };

        for seed in 0..1024 {
            let bytes = bytes_from_seed(seed, 8192);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            for mnemonic in ptx.lines().filter_map(body_mnemonic) {
                assert!(
                    !is_volatile_memory_mnemonic(mnemonic),
                    "seed {seed:x} emitted body {mnemonic}"
                );
            }
        }
    }

    #[test]
    fn bit_memory_generation_is_reachable() {
        let cfg = GenConfig {
            emit_memory_cache_ops: false,
            emit_volatile_memory: false,
            ..coverage_heavy_config()
        };
        assert_body_mnemonic_coverage(&cfg, 16384, 32768, BIT_MEMORY_MNEMONICS);
    }

    #[test]
    fn bit_memory_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_bit_memory: false,
            ..coverage_heavy_config()
        };

        for seed in 0..1024 {
            let bytes = bytes_from_seed(seed, 8192);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            for mnemonic in ptx.lines().filter_map(body_mnemonic) {
                assert!(
                    !is_bit_memory_mnemonic(mnemonic),
                    "seed {seed:x} emitted body {mnemonic}"
                );
            }
        }
    }

    #[test]
    fn const_load_generation_is_reachable() {
        let cfg = coverage_heavy_config();
        let mut found = vec![false; CONST_LOAD_MNEMONICS.len()];

        for seed in 0..2048 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            for (i, mnemonic) in CONST_LOAD_MNEMONICS.iter().enumerate() {
                found[i] |= has_body_const_load(&ptx, mnemonic);
            }
            if found.iter().all(|seen| *seen) {
                break;
            }
        }

        let missing: Vec<_> = CONST_LOAD_MNEMONICS
            .iter()
            .zip(found)
            .filter_map(|(mnemonic, seen)| (!seen).then_some(*mnemonic))
            .collect();
        assert!(
            missing.is_empty(),
            "sample did not emit body const loads {missing:?}"
        );
    }

    #[test]
    fn const_load_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_const_memory: false,
            ..coverage_heavy_config()
        };

        for seed in 0..1024 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            for mnemonic in CONST_LOAD_MNEMONICS {
                assert!(
                    !has_body_const_load(&ptx, mnemonic),
                    "seed {seed:x} emitted body {mnemonic}"
                );
            }
        }
    }

    #[test]
    fn const_load_offsets_are_bounded_and_aligned() {
        let cfg = coverage_heavy_config();

        for seed in 0..512 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            for (mnemonic, offset) in ptx.lines().filter_map(body_const_load) {
                let width = const_load_width(mnemonic).unwrap();
                assert_eq!(
                    offset % width,
                    0,
                    "seed {seed:x} emitted unaligned {mnemonic} offset {offset}"
                );
                assert!(
                    offset + width <= CONST_MEM_BYTES,
                    "seed {seed:x} emitted out-of-bounds {mnemonic} offset {offset}"
                );
            }
        }
    }

    #[test]
    fn local_memory_generation_is_reachable() {
        let cfg = coverage_heavy_config();
        let mut loads = vec![false; LOCAL_MEM_LOAD_MNEMONICS.len()];
        let mut stores = vec![false; LOCAL_MEM_STORE_MNEMONICS.len()];

        for seed in 0..2048 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            for (i, mnemonic) in LOCAL_MEM_LOAD_MNEMONICS.iter().enumerate() {
                loads[i] |= has_body_local_mem_access(&ptx, mnemonic);
            }
            for (i, mnemonic) in LOCAL_MEM_STORE_MNEMONICS.iter().enumerate() {
                stores[i] |= has_body_local_mem_access(&ptx, mnemonic);
            }
            if loads.iter().all(|seen| *seen) && stores.iter().all(|seen| *seen) {
                break;
            }
        }

        let missing_loads: Vec<_> = LOCAL_MEM_LOAD_MNEMONICS
            .iter()
            .zip(loads)
            .filter_map(|(mnemonic, seen)| (!seen).then_some(*mnemonic))
            .collect();
        let missing_stores: Vec<_> = LOCAL_MEM_STORE_MNEMONICS
            .iter()
            .zip(stores)
            .filter_map(|(mnemonic, seen)| (!seen).then_some(*mnemonic))
            .collect();
        assert!(
            missing_loads.is_empty() && missing_stores.is_empty(),
            "sample missed local loads {missing_loads:?} stores {missing_stores:?}"
        );
    }

    #[test]
    fn local_memory_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_local_memory: false,
            ..coverage_heavy_config()
        };

        for seed in 0..1024 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            for mnemonic in LOCAL_MEM_LOAD_MNEMONICS
                .iter()
                .chain(LOCAL_MEM_STORE_MNEMONICS.iter())
            {
                assert!(
                    !has_body_local_mem_access(&ptx, mnemonic),
                    "seed {seed:x} emitted body {mnemonic}"
                );
            }
        }
    }

    #[test]
    fn local_memory_offsets_are_bounded_and_aligned() {
        let cfg = coverage_heavy_config();

        for seed in 0..512 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            for (mnemonic, offset) in ptx.lines().filter_map(body_local_mem_access) {
                let width = local_memory_width(mnemonic).unwrap();
                assert_eq!(
                    offset % width,
                    0,
                    "seed {seed:x} emitted unaligned {mnemonic} offset {offset}"
                );
                assert!(
                    offset + width <= LOCAL_MEM_BYTES,
                    "seed {seed:x} emitted out-of-bounds {mnemonic} offset {offset}"
                );
            }
        }
    }

    #[test]
    fn shared_memory_generation_is_reachable() {
        let cfg = coverage_heavy_config();
        let mut loads = vec![false; SHARED_MEM_LOAD_MNEMONICS.len()];
        let mut stores = vec![false; SHARED_MEM_STORE_MNEMONICS.len()];

        for seed in 0..2048 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            for (i, mnemonic) in SHARED_MEM_LOAD_MNEMONICS.iter().enumerate() {
                loads[i] |= has_body_shared_mem_access(&ptx, mnemonic);
            }
            for (i, mnemonic) in SHARED_MEM_STORE_MNEMONICS.iter().enumerate() {
                stores[i] |= has_body_shared_mem_access(&ptx, mnemonic);
            }
            if loads.iter().all(|seen| *seen) && stores.iter().all(|seen| *seen) {
                break;
            }
        }

        let missing_loads: Vec<_> = SHARED_MEM_LOAD_MNEMONICS
            .iter()
            .zip(loads)
            .filter_map(|(mnemonic, seen)| (!seen).then_some(*mnemonic))
            .collect();
        let missing_stores: Vec<_> = SHARED_MEM_STORE_MNEMONICS
            .iter()
            .zip(stores)
            .filter_map(|(mnemonic, seen)| (!seen).then_some(*mnemonic))
            .collect();
        assert!(
            missing_loads.is_empty() && missing_stores.is_empty(),
            "sample missed shared loads {missing_loads:?} stores {missing_stores:?}"
        );
    }

    #[test]
    fn shared_memory_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_shared_memory: false,
            ..coverage_heavy_config()
        };

        for seed in 0..1024 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            for mnemonic in SHARED_MEM_LOAD_MNEMONICS
                .iter()
                .chain(SHARED_MEM_STORE_MNEMONICS.iter())
            {
                assert!(
                    !has_body_shared_mem_access(&ptx, mnemonic),
                    "seed {seed:x} emitted body {mnemonic}"
                );
            }
        }
    }

    #[test]
    fn shared_memory_offsets_are_bounded_and_aligned() {
        let cfg = coverage_heavy_config();

        for seed in 0..512 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            for (mnemonic, offset) in ptx.lines().filter_map(body_shared_mem_access) {
                let width = shared_memory_width(mnemonic).unwrap();
                assert_eq!(
                    offset % width,
                    0,
                    "seed {seed:x} emitted unaligned {mnemonic} offset {offset}"
                );
                assert!(
                    offset + width <= SHARED_SLOT_BYTES,
                    "seed {seed:x} emitted out-of-slot {mnemonic} offset {offset}"
                );
            }
        }
    }

    #[test]
    fn predicated_memory_generation_is_reachable() {
        let cfg = coverage_heavy_config();
        let mut saw_global_load = false;
        let mut saw_global_roundtrip = false;
        let mut saw_const = false;
        let mut saw_local = false;
        let mut saw_shared = false;
        let mut saw_vector = false;

        for seed in 0..4096 {
            let bytes = bytes_from_seed(seed, 8192);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            saw_global_load |=
                has_predicated_memory_access(&ptx, GLOBAL_LOAD_MNEMONICS, "[%rd6 + ");
            saw_global_roundtrip |= has_predicated_global_roundtrip_access(&ptx);
            saw_const |= has_predicated_memory_access(&ptx, CONST_LOAD_MNEMONICS, "[%rd6 + ");
            saw_local |= has_predicated_memory_access(&ptx, LOCAL_MEM_LOAD_MNEMONICS, "[%rd6 + ")
                || has_predicated_memory_access(&ptx, LOCAL_MEM_STORE_MNEMONICS, "[%rd6 + ");
            saw_shared |= has_predicated_memory_access(&ptx, SHARED_MEM_LOAD_MNEMONICS, "[%rd6 + ")
                || has_predicated_memory_access(&ptx, SHARED_MEM_STORE_MNEMONICS, "[%rd6 + ");
            saw_vector |= has_predicated_vector_memory_access(&ptx);
            if saw_global_load
                && saw_global_roundtrip
                && saw_const
                && saw_local
                && saw_shared
                && saw_vector
            {
                return;
            }
        }

        assert!(
            saw_global_load
                && saw_global_roundtrip
                && saw_const
                && saw_local
                && saw_shared
                && saw_vector,
            "sample missed predicated memory: global_load={saw_global_load} \
             global_roundtrip={saw_global_roundtrip} const={saw_const} \
             local={saw_local} shared={saw_shared} vector={saw_vector}"
        );
    }

    #[test]
    fn predicated_memory_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_predicated_memory: false,
            ..coverage_heavy_config()
        };

        for seed in 0..1024 {
            let bytes = bytes_from_seed(seed, 8192);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            assert!(
                !has_any_predicated_memory_access(&ptx),
                "seed {seed:x} emitted predicated memory"
            );
        }
    }

    #[test]
    fn vector_memory_generation_is_reachable() {
        assert_mnemonic_coverage(
            &coverage_heavy_config(),
            8192,
            4096,
            VECTOR_MEMORY_MNEMONICS,
        );
    }

    #[test]
    fn vector_memory_cache_ops_generation_is_reachable() {
        let cfg = coverage_heavy_config();
        let mut loads = vec![false; GLOBAL_LOAD_CACHE_PREFIXES.len()];
        let mut stores = vec![false; GLOBAL_STORE_CACHE_PREFIXES.len()];

        for seed in 0..8192 {
            let bytes = bytes_from_seed(seed, 8192);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            for (mnemonic, _, _) in ptx.lines().filter_map(body_vector_memory_access) {
                if global_vector_width(mnemonic).is_none() {
                    continue;
                }
                for (i, prefix) in GLOBAL_LOAD_CACHE_PREFIXES.iter().enumerate() {
                    loads[i] |= mnemonic.starts_with(prefix);
                }
                for (i, prefix) in GLOBAL_STORE_CACHE_PREFIXES.iter().enumerate() {
                    stores[i] |= mnemonic.starts_with(prefix);
                }
            }
            if loads.iter().all(|seen| *seen) && stores.iter().all(|seen| *seen) {
                break;
            }
        }

        let missing_loads: Vec<_> = GLOBAL_LOAD_CACHE_PREFIXES
            .iter()
            .zip(loads)
            .filter_map(|(prefix, seen)| (!seen).then_some(*prefix))
            .collect();
        let missing_stores: Vec<_> = GLOBAL_STORE_CACHE_PREFIXES
            .iter()
            .zip(stores)
            .filter_map(|(prefix, seen)| (!seen).then_some(*prefix))
            .collect();
        assert!(
            missing_loads.is_empty() && missing_stores.is_empty(),
            "sample missed vector memory cache loads {missing_loads:?} stores {missing_stores:?}"
        );
    }

    #[test]
    fn predicated_vector_memory_generation_is_reachable() {
        let cfg = coverage_heavy_config();

        for seed in 0..4096 {
            let bytes = bytes_from_seed(seed, 8192);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            if has_predicated_vector_memory_access(&ptx) {
                return;
            }
        }

        panic!("sample did not emit predicated vector memory");
    }

    #[test]
    fn vector_memory_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_vector_memory: false,
            ..coverage_heavy_config()
        };

        for seed in 0..1024 {
            let bytes = bytes_from_seed(seed, 8192);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            for mnemonic in VECTOR_MEMORY_MNEMONICS {
                assert!(
                    !has_mnemonic(&ptx, mnemonic),
                    "seed {seed:x} emitted {mnemonic}"
                );
            }
        }
    }

    #[test]
    fn vector_memory_offsets_are_bounded_and_aligned() {
        let cfg = coverage_heavy_config();

        for seed in 0..512 {
            let bytes = bytes_from_seed(seed, 8192);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            for (mnemonic, address, offset) in ptx.lines().filter_map(body_vector_memory_access) {
                let width = global_vector_width(mnemonic)
                    .or_else(|| shared_vector_width(mnemonic))
                    .unwrap_or_else(|| {
                        if mnemonic.contains(".u64") {
                            16
                        } else if mnemonic.contains(".v2.") {
                            8
                        } else {
                            16
                        }
                    });
                let limit = if global_vector_width(mnemonic).is_some()
                    && (mnemonic.starts_with("ld.global")
                        || mnemonic.starts_with("ldu.global")
                        || mnemonic.starts_with("ld.volatile.global"))
                    && address == "%rd6"
                {
                    input_len() as u32
                } else if global_vector_width(mnemonic).is_some()
                    && (mnemonic.starts_with("ld.global")
                        || mnemonic.starts_with("st.global")
                        || mnemonic.starts_with("ld.volatile.global")
                        || mnemonic.starts_with("st.volatile.global"))
                    && address == "%rd8"
                {
                    N_OUTPUTS * 4
                } else if shared_vector_width(mnemonic).is_some() && address == "%rd6" {
                    SHARED_SLOT_BYTES
                } else if const_vector_width(mnemonic).is_some() && address == "%rd6" {
                    CONST_MEM_BYTES
                } else if local_vector_width(mnemonic).is_some() && address == "%rd6" {
                    LOCAL_MEM_BYTES
                } else {
                    match (mnemonic, address) {
                        ("ld.const.v2.u32" | "ld.const.v4.u32" | "ld.const.v2.u64", "%rd6") => {
                            CONST_MEM_BYTES
                        }
                        (
                            "ld.local.v2.u32" | "ld.local.v4.u32" | "ld.local.v2.u64"
                            | "st.local.v2.u32" | "st.local.v4.u32" | "st.local.v2.u64",
                            "%rd6",
                        ) => LOCAL_MEM_BYTES,
                        (
                            "ld.shared.v2.u32" | "ld.shared.v4.u32" | "ld.shared.v2.u64"
                            | "st.shared.v2.u32" | "st.shared.v4.u32" | "st.shared.v2.u64",
                            "%rd6",
                        ) => SHARED_SLOT_BYTES,
                        _ => unreachable!(),
                    }
                };
                assert_eq!(
                    offset % width,
                    0,
                    "seed {seed:x} emitted unaligned {mnemonic} offset {offset}"
                );
                assert!(
                    offset + width <= limit,
                    "seed {seed:x} emitted out-of-bounds {mnemonic} offset {offset}"
                );
            }
        }
    }

    #[test]
    fn wide_memory_generation_is_reachable() {
        assert_mnemonic_coverage(&coverage_heavy_config(), 32768, 8192, WIDE_MEMORY_MNEMONICS);
    }

    #[test]
    fn wide_memory_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_wide_memory: false,
            ..coverage_heavy_config()
        };

        for seed in 0..2048 {
            let bytes = bytes_from_seed(seed, 8192);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            for mnemonic in WIDE_MEMORY_MNEMONICS {
                assert!(
                    !has_mnemonic(&ptx, mnemonic),
                    "seed {seed:x} emitted {mnemonic}"
                );
            }
        }
    }

    #[test]
    fn f32_arith_generation_is_reachable() {
        assert_mnemonic_coverage(&coverage_heavy_config(), 8192, 4096, F32_ARITH_MNEMONICS);
    }

    #[test]
    fn predicated_f32_arith_generation_is_reachable() {
        let cfg = GenConfig {
            emit_f64_arith: false,
            ..coverage_heavy_config()
        };
        assert_predicated_mnemonic_coverage(&cfg, 16384, 8192, F32_ARITH_MNEMONICS);
    }

    #[test]
    fn f32_arith_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_f32_arith: false,
            ..coverage_heavy_config()
        };

        for seed in 0..1024 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            for mnemonic in F32_ARITH_MNEMONICS
                .iter()
                .chain(F32_ROUNDING_MNEMONICS.iter())
            {
                assert!(
                    !has_mnemonic(&ptx, mnemonic),
                    "seed {seed:x} emitted {mnemonic}"
                );
            }
        }
    }

    #[test]
    fn f32_rounding_generation_is_reachable() {
        assert_mnemonic_coverage(
            &coverage_heavy_config(),
            16384,
            8192,
            F32_ROUNDING_MNEMONICS,
        );
    }

    #[test]
    fn predicated_f32_rounding_generation_is_reachable() {
        let cfg = GenConfig {
            emit_f64_arith: false,
            emit_f64_rounding: false,
            ..coverage_heavy_config()
        };
        assert_predicated_mnemonic_coverage(&cfg, 32768, 16384, F32_ROUNDING_MNEMONICS);
    }

    #[test]
    fn f32_rounding_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_f32_rounding: false,
            ..coverage_heavy_config()
        };

        for seed in 0..1024 {
            let bytes = bytes_from_seed(seed, 8192);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            for mnemonic in F32_ROUNDING_MNEMONICS {
                assert!(
                    !has_mnemonic(&ptx, mnemonic),
                    "seed {seed:x} emitted {mnemonic}"
                );
            }
        }
    }

    #[test]
    fn f32_unary_generation_is_reachable() {
        assert_mnemonic_coverage(&coverage_heavy_config(), 8192, 4096, F32_UNARY_MNEMONICS);
    }

    #[test]
    fn predicated_f32_unary_generation_is_reachable() {
        let cfg = GenConfig {
            emit_f64_unary: false,
            ..coverage_heavy_config()
        };
        assert_predicated_mnemonic_coverage(&cfg, 8192, 4096, F32_UNARY_MNEMONICS);
    }

    #[test]
    fn f32_unary_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_f32_unary: false,
            ..coverage_heavy_config()
        };

        for seed in 0..1024 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            for mnemonic in F32_UNARY_MNEMONICS {
                assert!(
                    !has_mnemonic(&ptx, mnemonic),
                    "seed {seed:x} emitted {mnemonic}"
                );
            }
        }
    }

    #[test]
    fn f32_cvt_generation_is_reachable() {
        let cfg = GenConfig {
            emit_f32_arith: false,
            emit_f32_rounding: false,
            emit_f32_unary: false,
            emit_f32_special_math: false,
            emit_f32_compare: false,
            emit_f32_selp: false,
            emit_f64_arith: false,
            emit_f64_rounding: false,
            emit_f64_unary: false,
            emit_f64_cvt: false,
            emit_f64_special_math: false,
            emit_f64_compare: false,
            emit_f64_selp: false,
            ..coverage_heavy_config()
        };
        assert_mnemonic_coverage(&cfg, 65536, 32768, F32_CVT_MNEMONICS);
    }

    #[test]
    fn predicated_f32_cvt_generation_is_reachable() {
        let cfg = GenConfig {
            emit_f32_arith: false,
            emit_f32_rounding: false,
            emit_f32_unary: false,
            emit_f32_special_math: false,
            emit_f32_compare: false,
            emit_f32_selp: false,
            emit_f64_arith: false,
            emit_f64_rounding: false,
            emit_f64_unary: false,
            emit_f64_cvt: false,
            emit_f64_special_math: false,
            emit_f64_compare: false,
            emit_f64_selp: false,
            ..coverage_heavy_config()
        };
        assert_predicated_mnemonic_coverage(&cfg, 131072, 65536, F32_CVT_MNEMONICS);
    }

    #[test]
    fn f32_cvt_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_f32_cvt: false,
            ..coverage_heavy_config()
        };

        for seed in 0..1024 {
            let bytes = bytes_from_seed(seed, 8192);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            for mnemonic in F32_CVT_DISABLE_MNEMONICS {
                assert!(
                    !has_mnemonic(&ptx, mnemonic),
                    "seed {seed:x} emitted {mnemonic}"
                );
            }
        }
    }

    #[test]
    fn f32_special_math_generation_is_reachable() {
        assert_mnemonic_coverage(
            &coverage_heavy_config(),
            16384,
            8192,
            F32_SPECIAL_MATH_MNEMONICS,
        );
    }

    #[test]
    fn predicated_f32_special_math_generation_is_reachable() {
        let cfg = GenConfig {
            emit_f64_special_math: false,
            ..coverage_heavy_config()
        };
        assert_predicated_mnemonic_coverage(&cfg, 32768, 16384, F32_SPECIAL_MATH_MNEMONICS);
    }

    #[test]
    fn f32_special_math_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_f32_special_math: false,
            ..coverage_heavy_config()
        };

        for seed in 0..1024 {
            let bytes = bytes_from_seed(seed, 8192);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            for mnemonic in F32_SPECIAL_MATH_MNEMONICS {
                assert!(
                    !has_mnemonic(&ptx, mnemonic),
                    "seed {seed:x} emitted {mnemonic}"
                );
            }
        }
    }

    #[test]
    fn f32_compare_generation_is_reachable() {
        let cfg = GenConfig {
            emit_f32_selp: false,
            ..coverage_heavy_config()
        };
        assert_mnemonic_coverage(&cfg, 16384, 8192, F32_COMPARE_MNEMONICS);
        assert_mnemonic_coverage(&cfg, 4096, 2048, F32_TESTP_MNEMONICS);
    }

    #[test]
    fn predicated_f32_set_generation_is_reachable() {
        let cfg = GenConfig {
            emit_f32_selp: false,
            emit_f64_compare: false,
            emit_setp_bool: false,
            ..coverage_heavy_config()
        };
        assert_predicated_mnemonic_coverage(&cfg, 32768, 16384, F32_COMPARE_MNEMONICS);
    }

    #[test]
    fn predicated_f32_testp_generation_is_reachable() {
        let cfg = GenConfig {
            emit_f32_selp: false,
            emit_f64_compare: false,
            emit_set: false,
            emit_setp_bool: false,
            ..coverage_heavy_config()
        };
        assert_predicated_mnemonic_coverage(&cfg, 8192, 4096, F32_TESTP_MNEMONICS);
    }

    #[test]
    fn f32_compare_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_f32_compare: false,
            ..coverage_heavy_config()
        };

        for seed in 0..1024 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            let seen = mnemonic_set(&ptx);
            for mnemonic in F32_COMPARE_MNEMONICS
                .iter()
                .chain(F32_SETP_MNEMONICS.iter())
                .chain(F32_SETP_BOOL_MNEMONICS.iter())
                .chain(F32_TESTP_MNEMONICS.iter())
            {
                assert!(!seen.contains(mnemonic), "seed {seed:x} emitted {mnemonic}");
            }
            assert!(!seen.contains("selp.f32"), "seed {seed:x} emitted selp.f32");
        }
    }

    #[test]
    fn f32_setp_bool_generation_is_reachable() {
        let cfg = GenConfig {
            emit_f64_compare: false,
            emit_set: false,
            emit_f32_selp: false,
            ..coverage_heavy_config()
        };
        assert_mnemonic_coverage(&cfg, 65536, 32768, F32_SETP_BOOL_MNEMONICS);
    }

    #[test]
    fn predicated_f32_setp_bool_generation_is_reachable() {
        let cfg = GenConfig {
            emit_f64_compare: false,
            emit_set: false,
            emit_f32_selp: false,
            ..coverage_heavy_config()
        };
        assert_predicated_mnemonic_coverage(&cfg, 65536, 32768, F32_SETP_BOOL_MNEMONICS);
    }

    #[test]
    fn f32_selp_generation_is_reachable() {
        let cfg = GenConfig {
            emit_set: false,
            ..coverage_heavy_config()
        };
        assert_mnemonic_coverage(&cfg, 16384, 8192, F32_SETP_MNEMONICS);
        assert_mnemonic_coverage(&cfg, 4096, 2048, F32_SELP_MNEMONICS);
    }

    #[test]
    fn predicated_f32_selp_generation_is_reachable() {
        let cfg = GenConfig {
            emit_set: false,
            emit_f64_compare: false,
            emit_setp_bool: false,
            ..coverage_heavy_config()
        };
        assert_predicated_mnemonic_coverage(&cfg, 4096, 2048, F32_SELP_MNEMONICS);
    }

    #[test]
    fn f32_selp_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_f32_selp: false,
            ..coverage_heavy_config()
        };

        for seed in 0..1024 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            assert!(
                !has_mnemonic(&ptx, "selp.f32"),
                "seed {seed:x} emitted selp.f32"
            );
            for mnemonic in F32_SETP_MNEMONICS {
                assert!(
                    !has_mnemonic(&ptx, mnemonic),
                    "seed {seed:x} emitted {mnemonic}"
                );
            }
        }
    }

    #[test]
    fn f64_arith_generation_is_reachable() {
        assert_mnemonic_coverage(&coverage_heavy_config(), 8192, 4096, F64_ARITH_MNEMONICS);
    }

    #[test]
    fn predicated_f64_arith_generation_is_reachable() {
        let cfg = GenConfig {
            emit_f32_arith: false,
            ..coverage_heavy_config()
        };
        assert_predicated_mnemonic_coverage(&cfg, 8192, 4096, F64_ARITH_MNEMONICS);
    }

    #[test]
    fn f64_arith_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_f64_arith: false,
            ..coverage_heavy_config()
        };

        for seed in 0..1024 {
            let bytes = bytes_from_seed(seed, 8192);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            for mnemonic in F64_ARITH_MNEMONICS
                .iter()
                .chain(F64_ROUNDING_MNEMONICS.iter())
            {
                assert!(
                    !has_mnemonic(&ptx, mnemonic),
                    "seed {seed:x} emitted {mnemonic}"
                );
            }
        }
    }

    #[test]
    fn f64_rounding_generation_is_reachable() {
        assert_mnemonic_coverage(&coverage_heavy_config(), 8192, 4096, F64_ROUNDING_MNEMONICS);
    }

    #[test]
    fn predicated_f64_rounding_generation_is_reachable() {
        let cfg = GenConfig {
            emit_f32_arith: false,
            emit_f32_rounding: false,
            ..coverage_heavy_config()
        };
        assert_predicated_mnemonic_coverage(&cfg, 16384, 8192, F64_ROUNDING_MNEMONICS);
    }

    #[test]
    fn f64_rounding_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_f64_rounding: false,
            ..coverage_heavy_config()
        };

        for seed in 0..1024 {
            let bytes = bytes_from_seed(seed, 8192);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            for mnemonic in F64_ROUNDING_MNEMONICS {
                assert!(
                    !has_mnemonic(&ptx, mnemonic),
                    "seed {seed:x} emitted {mnemonic}"
                );
            }
        }
    }

    #[test]
    fn f64_unary_generation_is_reachable() {
        assert_mnemonic_coverage(&coverage_heavy_config(), 4096, 2048, F64_UNARY_MNEMONICS);
    }

    #[test]
    fn predicated_f64_unary_generation_is_reachable() {
        let cfg = GenConfig {
            emit_f32_unary: false,
            ..coverage_heavy_config()
        };
        assert_predicated_mnemonic_coverage(&cfg, 4096, 2048, F64_UNARY_MNEMONICS);
    }

    #[test]
    fn f64_unary_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_f64_unary: false,
            ..coverage_heavy_config()
        };

        for seed in 0..1024 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            for mnemonic in F64_UNARY_MNEMONICS {
                assert!(
                    !has_mnemonic(&ptx, mnemonic),
                    "seed {seed:x} emitted {mnemonic}"
                );
            }
        }
    }

    #[test]
    fn f64_cvt_generation_is_reachable() {
        let cfg = GenConfig {
            emit_f32_arith: false,
            emit_f32_rounding: false,
            emit_f32_unary: false,
            emit_f32_cvt: false,
            emit_f32_special_math: false,
            emit_f32_compare: false,
            emit_f32_selp: false,
            emit_f64_arith: false,
            emit_f64_rounding: false,
            emit_f64_unary: false,
            emit_f64_special_math: false,
            emit_f64_compare: false,
            emit_f64_selp: false,
            ..coverage_heavy_config()
        };
        assert_mnemonic_coverage(&cfg, 32768, 16384, F64_CVT_MNEMONICS);
    }

    #[test]
    fn predicated_f64_cvt_generation_is_reachable() {
        let cfg = GenConfig {
            emit_f32_arith: false,
            emit_f32_rounding: false,
            emit_f32_unary: false,
            emit_f32_cvt: false,
            emit_f32_special_math: false,
            emit_f32_compare: false,
            emit_f32_selp: false,
            emit_f64_arith: false,
            emit_f64_rounding: false,
            emit_f64_unary: false,
            emit_f64_special_math: false,
            emit_f64_compare: false,
            emit_f64_selp: false,
            ..coverage_heavy_config()
        };
        assert_predicated_mnemonic_coverage(&cfg, 65536, 32768, F64_CVT_MNEMONICS);
    }

    #[test]
    fn f64_cvt_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_f64_cvt: false,
            ..coverage_heavy_config()
        };

        for seed in 0..1024 {
            let bytes = bytes_from_seed(seed, 8192);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            for mnemonic in F64_CVT_DISABLE_MNEMONICS {
                assert!(
                    !has_mnemonic(&ptx, mnemonic),
                    "seed {seed:x} emitted {mnemonic}"
                );
            }
        }
    }

    #[test]
    fn f64_special_math_generation_is_reachable() {
        assert_mnemonic_coverage(
            &coverage_heavy_config(),
            4096,
            2048,
            F64_SPECIAL_MATH_MNEMONICS,
        );
    }

    #[test]
    fn predicated_f64_special_math_generation_is_reachable() {
        let cfg = GenConfig {
            emit_f32_special_math: false,
            ..coverage_heavy_config()
        };
        assert_predicated_mnemonic_coverage(&cfg, 16384, 4096, F64_SPECIAL_MATH_MNEMONICS);
    }

    #[test]
    fn f64_special_math_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_f64_special_math: false,
            ..coverage_heavy_config()
        };

        for seed in 0..1024 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            for mnemonic in F64_SPECIAL_MATH_MNEMONICS {
                assert!(
                    !has_mnemonic(&ptx, mnemonic),
                    "seed {seed:x} emitted {mnemonic}"
                );
            }
        }
    }

    #[test]
    fn f64_compare_generation_is_reachable() {
        let cfg = GenConfig {
            emit_f64_selp: false,
            ..coverage_heavy_config()
        };
        assert_mnemonic_coverage(&cfg, 4096, 2048, F64_COMPARE_MNEMONICS);
        assert_mnemonic_coverage(&cfg, 4096, 2048, F64_TESTP_MNEMONICS);
    }

    #[test]
    fn predicated_f64_set_generation_is_reachable() {
        let cfg = GenConfig {
            emit_f32_compare: false,
            emit_f64_selp: false,
            emit_setp_bool: false,
            ..coverage_heavy_config()
        };
        assert_predicated_mnemonic_coverage(&cfg, 16384, 8192, F64_COMPARE_MNEMONICS);
    }

    #[test]
    fn predicated_f64_testp_generation_is_reachable() {
        let cfg = GenConfig {
            emit_f32_compare: false,
            emit_f64_selp: false,
            emit_set: false,
            emit_setp_bool: false,
            ..coverage_heavy_config()
        };
        assert_predicated_mnemonic_coverage(&cfg, 8192, 4096, F64_TESTP_MNEMONICS);
    }

    #[test]
    fn f64_compare_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_f64_compare: false,
            ..coverage_heavy_config()
        };

        for seed in 0..1024 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            let seen = mnemonic_set(&ptx);
            for mnemonic in F64_COMPARE_MNEMONICS
                .iter()
                .chain(F64_SETP_MNEMONICS.iter())
                .chain(F64_SETP_BOOL_MNEMONICS.iter())
                .chain(F64_TESTP_MNEMONICS.iter())
            {
                assert!(!seen.contains(mnemonic), "seed {seed:x} emitted {mnemonic}");
            }
            assert!(!seen.contains("selp.f64"), "seed {seed:x} emitted selp.f64");
        }
    }

    #[test]
    fn f64_setp_bool_generation_is_reachable() {
        let cfg = GenConfig {
            emit_f32_compare: false,
            emit_set: false,
            emit_f64_selp: false,
            ..coverage_heavy_config()
        };
        assert_mnemonic_coverage(&cfg, 32768, 8192, F64_SETP_BOOL_MNEMONICS);
    }

    #[test]
    fn predicated_f64_setp_bool_generation_is_reachable() {
        let cfg = GenConfig {
            emit_f32_compare: false,
            emit_set: false,
            emit_f64_selp: false,
            ..coverage_heavy_config()
        };
        assert_predicated_mnemonic_coverage(&cfg, 32768, 16384, F64_SETP_BOOL_MNEMONICS);
    }

    #[test]
    fn f64_selp_generation_is_reachable() {
        let cfg = GenConfig {
            emit_set: false,
            ..coverage_heavy_config()
        };
        assert_mnemonic_coverage(&cfg, 4096, 2048, F64_SETP_MNEMONICS);
        assert_mnemonic_coverage(&cfg, 4096, 2048, F64_SELP_MNEMONICS);
    }

    #[test]
    fn predicated_f64_selp_generation_is_reachable() {
        let cfg = GenConfig {
            emit_f32_compare: false,
            emit_set: false,
            emit_setp_bool: false,
            ..coverage_heavy_config()
        };
        assert_predicated_mnemonic_coverage(&cfg, 4096, 2048, F64_SELP_MNEMONICS);
    }

    #[test]
    fn f64_selp_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_f64_selp: false,
            ..coverage_heavy_config()
        };

        for seed in 0..1024 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            assert!(
                !has_mnemonic(&ptx, "selp.f64"),
                "seed {seed:x} emitted selp.f64"
            );
            for mnemonic in F64_SETP_MNEMONICS {
                assert!(
                    !has_mnemonic(&ptx, mnemonic),
                    "seed {seed:x} emitted {mnemonic}"
                );
            }
        }
    }

    #[test]
    fn special_reg_generation_is_reachable() {
        let mut found = vec![false; SPECIAL_REG_NAMES.len()];

        for seed in 0..4096 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes(&bytes).unwrap();
            for (i, reg_name) in SPECIAL_REG_NAMES.iter().enumerate() {
                found[i] |= has_special_reg(&ptx, reg_name);
            }
            if found.iter().all(|seen| *seen) {
                break;
            }
        }

        let missing: Vec<_> = SPECIAL_REG_NAMES
            .iter()
            .zip(found)
            .filter_map(|(reg_name, seen)| (!seen).then_some(*reg_name))
            .collect();
        assert!(missing.is_empty(), "sample did not emit {missing:?}");
    }

    #[test]
    fn special_reg_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_special_regs: false,
            ..GenConfig::default()
        };

        for seed in 0..1024 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            for reg_name in SPECIAL_REG_NAMES {
                assert!(
                    !has_special_reg(&ptx, reg_name),
                    "seed {seed:x} emitted {reg_name}"
                );
            }
            assert!(
                !has_predicated_special_reg(&ptx),
                "seed {seed:x} emitted predicated special register"
            );
        }
    }

    #[test]
    fn predicated_special_reg_generation_is_reachable() {
        let cfg = GenConfig {
            emit_not: false,
            emit_clz: false,
            emit_brev: false,
            emit_neg: false,
            emit_cnot: false,
            emit_popc: false,
            emit_abs: false,
            ..coverage_heavy_config()
        };

        for seed in 0..4096 {
            let bytes = bytes_from_seed(seed, 32768);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            if has_predicated_special_reg(&ptx) {
                return;
            }
        }

        panic!("sample did not emit predicated special-register mov");
    }

    #[test]
    fn predicated_special_reg_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_predicated_special_regs: false,
            ..coverage_heavy_config()
        };

        for seed in 0..2048 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            assert!(
                !has_predicated_special_reg(&ptx),
                "seed {seed:x} emitted predicated special-register mov"
            );
        }
    }

    #[test]
    fn neg_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_neg: false,
            ..GenConfig::default()
        };

        for seed in 0..256 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            assert!(!ptx.contains("neg.s32"), "seed {seed:x} emitted neg.s32");
        }
    }

    #[test]
    fn predicated_unary_generation_is_reachable() {
        let cfg = coverage_heavy_config();
        let mut saw_predicated_unary = false;

        for seed in 0..1024 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            saw_predicated_unary |= has_predicated_unary(&ptx);
            if saw_predicated_unary {
                break;
            }
        }

        assert!(
            saw_predicated_unary,
            "no seed in sample emitted predicated unary"
        );
    }

    #[test]
    fn predicated_unary_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_predicated_unary: false,
            ..GenConfig::default()
        };

        for seed in 0..512 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            assert!(
                !has_predicated_unary(&ptx),
                "seed {seed:x} emitted predicated unary"
            );
        }
    }

    #[test]
    fn shl_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_shl: false,
            ..GenConfig::default()
        };

        for seed in 0..256 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            assert!(!ptx.contains("shl.b32"), "seed {seed:x} emitted shl.b32");
        }
    }

    #[test]
    fn shr_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_shr: false,
            ..GenConfig::default()
        };

        for seed in 0..256 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            assert!(!ptx.contains("shr.u32"), "seed {seed:x} emitted shr.u32");
        }
    }

    #[test]
    fn signed_shr_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_signed_shr: false,
            ..GenConfig::default()
        };

        for seed in 0..256 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            assert!(!ptx.contains("shr.s32"), "seed {seed:x} emitted shr.s32");
        }
    }

    #[test]
    fn predicated_shift_generation_is_reachable() {
        let cfg = GenConfig {
            emit_predicated_reg_shifts: false,
            ..coverage_heavy_config()
        };
        let mut saw_predicated_shift = false;

        for seed in 0..1024 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            saw_predicated_shift |= has_predicated_shift(&ptx);
            if saw_predicated_shift {
                break;
            }
        }

        assert!(
            saw_predicated_shift,
            "no seed in sample emitted predicated shift"
        );
    }

    #[test]
    fn predicated_shift_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_predicated_shifts: false,
            emit_predicated_reg_shifts: false,
            ..GenConfig::default()
        };

        for seed in 0..512 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            assert!(
                !has_predicated_shift(&ptx),
                "seed {seed:x} emitted predicated shift"
            );
        }
    }

    #[test]
    fn register_shift_generation_is_reachable() {
        let cfg = coverage_heavy_config();
        let mut saw_register_shift = false;

        for seed in 0..1024 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            saw_register_shift |= has_register_shift(&ptx);
            if saw_register_shift {
                break;
            }
        }

        assert!(
            saw_register_shift,
            "no seed in sample emitted a masked register-count shift"
        );
    }

    #[test]
    fn register_shift_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_reg_shifts: false,
            ..GenConfig::default()
        };

        for seed in 0..512 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            assert!(
                !has_register_shift(&ptx),
                "seed {seed:x} emitted a masked register-count shift"
            );
        }
    }

    #[test]
    fn predicated_register_shift_generation_is_reachable() {
        let cfg = GenConfig {
            emit_predicated_shifts: false,
            ..coverage_heavy_config()
        };
        let mut saw_predicated_register_shift = false;

        for seed in 0..2048 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            saw_predicated_register_shift |= has_predicated_register_shift(&ptx);
            if saw_predicated_register_shift {
                break;
            }
        }

        assert!(
            saw_predicated_register_shift,
            "no seed in sample emitted a predicated masked register-count shift"
        );
    }

    #[test]
    fn predicated_register_shift_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_predicated_shifts: false,
            emit_predicated_reg_shifts: false,
            ..GenConfig::default()
        };

        for seed in 0..1024 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            assert!(
                !has_predicated_register_shift(&ptx),
                "seed {seed:x} emitted a predicated masked register-count shift"
            );
        }
    }

    #[test]
    fn all_shift_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_shl: false,
            emit_shr: false,
            emit_signed_shr: false,
            ..GenConfig::default()
        };

        for seed in 0..256 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            assert!(!ptx.contains("shl.b32"), "seed {seed:x} emitted shl.b32");
            assert!(!ptx.contains("shr.u32"), "seed {seed:x} emitted shr.u32");
            assert!(!ptx.contains("shr.s32"), "seed {seed:x} emitted shr.s32");
        }
    }

    #[test]
    fn bfind_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_bfind: false,
            ..GenConfig::default()
        };

        for seed in 0..256 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            for mnemonic in BFIND_MNEMONICS {
                assert!(!ptx.contains(mnemonic), "seed {seed:x} emitted {mnemonic}");
            }
        }
    }

    #[test]
    fn bmsk_generation_is_reachable() {
        let cfg = GenConfig {
            emit_predicated_bitfield: false,
            ..coverage_heavy_config()
        };
        assert_mnemonic_coverage(&cfg, 32768, 4096, BMSK_MNEMONICS);
    }

    #[test]
    fn bmsk_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_bmsk: false,
            ..GenConfig::default()
        };

        for seed in 0..256 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            for mnemonic in BMSK_MNEMONICS {
                assert!(!ptx.contains(mnemonic), "seed {seed:x} emitted {mnemonic}");
            }
        }
    }

    #[test]
    fn bmsk_wrap_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_bmsk_wrap: false,
            ..coverage_heavy_config()
        };

        for seed in 0..1024 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            assert!(
                !ptx.contains("bmsk.wrap.b32"),
                "seed {seed:x} emitted bmsk.wrap.b32"
            );
        }
    }

    #[test]
    fn bfi_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_bfi: false,
            ..GenConfig::default()
        };

        for seed in 0..256 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            assert!(!ptx.contains("bfi.b32"), "seed {seed:x} emitted bfi.b32");
        }
    }

    #[test]
    fn bfi_generation_is_reachable() {
        let mut saw_bfi = false;
        for seed in 0..2048 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes(&bytes).unwrap();
            if ptx.contains("bfi.b32") {
                saw_bfi = true;
                break;
            }
        }
        assert!(saw_bfi, "no seed in sample emitted bfi.b32");
    }

    #[test]
    fn bfe_generation_is_reachable() {
        let mnemonics = ["bfe.u32", "bfe.s32"];
        let mut found = [false; 2];

        for seed in 0..2048 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes(&bytes).unwrap();
            for (i, mnemonic) in mnemonics.iter().enumerate() {
                found[i] |= has_mnemonic(&ptx, mnemonic);
            }
            if found.iter().all(|seen| *seen) {
                break;
            }
        }

        let missing: Vec<_> = mnemonics
            .iter()
            .zip(found)
            .filter_map(|(mnemonic, seen)| (!seen).then_some(*mnemonic))
            .collect();
        assert!(missing.is_empty(), "sample did not emit {missing:?}");
    }

    #[test]
    fn bfe_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_bfe: false,
            ..GenConfig::default()
        };

        for seed in 0..1024 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            for mnemonic in BFE_MNEMONICS {
                assert!(
                    !has_mnemonic(&ptx, mnemonic),
                    "seed {seed:x} emitted {mnemonic}"
                );
            }
        }
    }

    #[test]
    fn register_bitfield_generation_is_reachable() {
        let cfg = GenConfig {
            emit_predicated_bitfield: false,
            emit_wide_bfe: false,
            emit_wide_bfi: false,
            ..coverage_heavy_config()
        };

        for seed in 0..4096 {
            let bytes = bytes_from_seed(seed, 32768);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            if has_register_bitfield_param(&ptx) {
                return;
            }
        }

        panic!("sample did not emit a register bitfield pos/len operand");
    }

    #[test]
    fn register_bitfield_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_reg_bitfield: false,
            ..coverage_heavy_config()
        };

        for seed in 0..1024 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            assert!(
                !has_register_bitfield_param(&ptx),
                "seed {seed:x} emitted a register bitfield pos/len operand"
            );
        }
    }

    #[test]
    fn wide_bfe_generation_is_reachable() {
        let cfg = GenConfig {
            emit_bmsk: false,
            emit_bfi: false,
            emit_wide_bfi: false,
            emit_predicated_wide_bitfield: false,
            ..coverage_heavy_config()
        };
        assert_mnemonic_coverage(&cfg, 32768, 2048, WIDE_BFE_MNEMONICS);
    }

    #[test]
    fn wide_bfe_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_wide_bfe: false,
            ..GenConfig::default()
        };

        for seed in 0..1024 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            for mnemonic in WIDE_BFE_MNEMONICS {
                assert!(
                    !has_mnemonic(&ptx, mnemonic),
                    "seed {seed:x} emitted {mnemonic}"
                );
            }
        }
    }

    #[test]
    fn signed_wide_bfe_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_signed_wide_bfe: false,
            ..GenConfig::default()
        };

        for seed in 0..1024 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            for mnemonic in SIGNED_WIDE_BFE_MNEMONICS {
                assert!(
                    !has_mnemonic(&ptx, mnemonic),
                    "seed {seed:x} emitted {mnemonic}"
                );
            }
        }
    }

    #[test]
    fn wide_bfi_generation_is_reachable() {
        let cfg = GenConfig {
            emit_bfi: false,
            emit_wide_bfe: false,
            emit_predicated_wide_bitfield: false,
            ..coverage_heavy_config()
        };
        assert_mnemonic_coverage(&cfg, 32768, 1024, WIDE_BFI_MNEMONICS);
    }

    #[test]
    fn wide_bfi_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_wide_bfi: false,
            ..GenConfig::default()
        };

        for seed in 0..1024 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            for mnemonic in WIDE_BFI_MNEMONICS {
                assert!(
                    !has_mnemonic(&ptx, mnemonic),
                    "seed {seed:x} emitted {mnemonic}"
                );
            }
        }
    }

    #[test]
    fn predicated_bitfield_generation_is_reachable() {
        let cfg = coverage_heavy_config();
        let mut found = vec![false; BITFIELD_MNEMONICS.len()];

        for seed in 0..4096 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            for op in ptx.lines().filter_map(predicated_mnemonic) {
                for (i, mnemonic) in BITFIELD_MNEMONICS.iter().enumerate() {
                    found[i] |= op == *mnemonic;
                }
            }
            if found.iter().all(|seen| *seen) {
                break;
            }
        }

        let missing: Vec<_> = BITFIELD_MNEMONICS
            .iter()
            .zip(found)
            .filter_map(|(mnemonic, seen)| (!seen).then_some(*mnemonic))
            .collect();
        assert!(
            missing.is_empty(),
            "sample did not emit predicated {missing:?}"
        );
    }

    #[test]
    fn predicated_bitfield_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_predicated_bitfield: false,
            ..GenConfig::default()
        };

        for seed in 0..512 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            assert!(
                !has_predicated_bitfield(&ptx),
                "seed {seed:x} emitted predicated bitfield instruction"
            );
        }
    }

    #[test]
    fn predicated_register_bitfield_generation_is_reachable() {
        let cfg = GenConfig {
            emit_wide_bfe: false,
            emit_wide_bfi: false,
            ..coverage_heavy_config()
        };

        for seed in 0..4096 {
            let bytes = bytes_from_seed(seed, 32768);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            if has_predicated_register_bitfield_param(&ptx) {
                return;
            }
        }

        panic!("sample did not emit a predicated register bitfield pos/len operand");
    }

    #[test]
    fn predicated_register_bitfield_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_predicated_reg_bitfield: false,
            ..coverage_heavy_config()
        };

        for seed in 0..1024 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            assert!(
                !has_predicated_register_bitfield_param(&ptx),
                "seed {seed:x} emitted a predicated register bitfield pos/len operand"
            );
        }
    }

    #[test]
    fn predicated_wide_bitfield_generation_is_reachable() {
        let cfg = GenConfig {
            emit_bmsk: false,
            ..coverage_heavy_config()
        };
        let mut found = vec![false; WIDE_BITFIELD_MNEMONICS.len()];

        for seed in 0..4096 {
            let bytes = bytes_from_seed(seed, 32768);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            for op in ptx.lines().filter_map(predicated_mnemonic) {
                for (i, mnemonic) in WIDE_BITFIELD_MNEMONICS.iter().enumerate() {
                    found[i] |= op == *mnemonic;
                }
            }
            if found.iter().all(|seen| *seen) {
                break;
            }
        }

        let missing: Vec<_> = WIDE_BITFIELD_MNEMONICS
            .iter()
            .zip(found)
            .filter_map(|(mnemonic, seen)| (!seen).then_some(*mnemonic))
            .collect();
        assert!(
            missing.is_empty(),
            "sample did not emit predicated {missing:?}"
        );
    }

    #[test]
    fn predicated_wide_bitfield_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_predicated_wide_bitfield: false,
            ..GenConfig::default()
        };

        for seed in 0..1024 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            assert!(
                !has_predicated_wide_bitfield(&ptx),
                "seed {seed:x} emitted predicated wide bitfield instruction"
            );
        }
    }

    #[test]
    fn register_wide_bitfield_generation_is_reachable() {
        let cfg = GenConfig {
            emit_bfi: false,
            emit_bmsk: false,
            emit_predicated_wide_bitfield: false,
            ..coverage_heavy_config()
        };

        for seed in 0..4096 {
            let bytes = bytes_from_seed(seed, 32768);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            if has_register_wide_bitfield_param(&ptx) {
                return;
            }
        }

        panic!("sample did not emit a register wide bitfield pos/len operand");
    }

    #[test]
    fn register_wide_bitfield_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_reg_wide_bitfield: false,
            ..coverage_heavy_config()
        };

        for seed in 0..2048 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            assert!(
                !has_register_wide_bitfield_param(&ptx),
                "seed {seed:x} emitted a register wide bitfield pos/len operand"
            );
        }
    }

    #[test]
    fn predicated_register_wide_bitfield_generation_is_reachable() {
        let cfg = GenConfig {
            emit_bfi: false,
            emit_bmsk: false,
            ..coverage_heavy_config()
        };

        for seed in 0..8192 {
            let bytes = bytes_from_seed(seed, 32768);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            if has_predicated_register_wide_bitfield_param(&ptx) {
                return;
            }
        }

        panic!("sample did not emit a predicated register wide bitfield pos/len operand");
    }

    #[test]
    fn predicated_register_wide_bitfield_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_predicated_reg_wide_bitfield: false,
            ..coverage_heavy_config()
        };

        for seed in 0..2048 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            assert!(
                !has_predicated_register_wide_bitfield_param(&ptx),
                "seed {seed:x} emitted a predicated register wide bitfield pos/len operand"
            );
        }
    }

    #[test]
    fn mad24_generation_is_reachable_when_bfind_is_disabled() {
        let cfg = GenConfig {
            emit_bfind: false,
            ..GenConfig::default()
        };

        let mnemonics = [
            "mad24.lo.u32",
            "mad24.hi.u32",
            "mad24.lo.s32",
            "mad24.hi.s32",
        ];
        let mut found = [false; 4];

        for seed in 0..4096 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            for (i, mnemonic) in mnemonics.iter().enumerate() {
                found[i] |= has_mnemonic(&ptx, mnemonic);
            }
            if found.iter().all(|seen| *seen) {
                break;
            }
        }

        let missing: Vec<_> = mnemonics
            .iter()
            .zip(found)
            .filter_map(|(mnemonic, seen)| (!seen).then_some(*mnemonic))
            .collect();
        assert!(missing.is_empty(), "sample did not emit {missing:?}");
    }

    #[test]
    fn mad24_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_bfind: false,
            emit_mad24: false,
            emit_mul24: false,
            ..GenConfig::default()
        };

        for seed in 0..256 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            for mnemonic in [
                "mad24.lo.u32",
                "mad24.hi.u32",
                "mad24.lo.s32",
                "mad24.hi.s32",
            ] {
                assert!(!ptx.contains(mnemonic), "seed {seed:x} emitted {mnemonic}");
            }
        }
    }

    #[test]
    fn mul24_generation_is_reachable_when_bfind_is_disabled() {
        let cfg = GenConfig {
            emit_bfind: false,
            emit_mad24: false,
            ..GenConfig::default()
        };

        let mnemonics = [
            "mul24.lo.u32",
            "mul24.hi.u32",
            "mul24.lo.s32",
            "mul24.hi.s32",
        ];
        let mut found = [false; 4];

        for seed in 0..4096 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            for (i, mnemonic) in mnemonics.iter().enumerate() {
                found[i] |= has_mnemonic(&ptx, mnemonic);
            }
            if found.iter().all(|seen| *seen) {
                break;
            }
        }

        let missing: Vec<_> = mnemonics
            .iter()
            .zip(found)
            .filter_map(|(mnemonic, seen)| (!seen).then_some(*mnemonic))
            .collect();
        assert!(missing.is_empty(), "sample did not emit {missing:?}");
    }

    #[test]
    fn mul24_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_bfind: false,
            emit_mad24: false,
            emit_mul24: false,
            ..GenConfig::default()
        };

        for seed in 0..256 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            for mnemonic in [
                "mul24.lo.u32",
                "mul24.hi.u32",
                "mul24.lo.s32",
                "mul24.hi.s32",
            ] {
                assert!(!ptx.contains(mnemonic), "seed {seed:x} emitted {mnemonic}");
            }
        }
    }

    #[test]
    fn predicated_24bit_generation_is_reachable() {
        let mad_cfg = GenConfig {
            emit_addc: false,
            emit_subc: false,
            emit_bfind: false,
            emit_mul24: false,
            ..coverage_heavy_config()
        };
        let mut found_mad = vec![false; MAD24_MNEMONICS.len()];
        for seed in 0..2048 {
            let bytes = bytes_from_seed(seed, 32768);
            let ptx = generate_from_bytes_with_config(&bytes, &mad_cfg).unwrap();
            for op in ptx.lines().filter_map(predicated_mnemonic) {
                for (i, mnemonic) in MAD24_MNEMONICS.iter().enumerate() {
                    found_mad[i] |= op == *mnemonic;
                }
            }
            if found_mad.iter().all(|seen| *seen) {
                break;
            }
        }

        let missing_mad: Vec<_> = MAD24_MNEMONICS
            .iter()
            .zip(found_mad)
            .filter_map(|(mnemonic, seen)| (!seen).then_some(*mnemonic))
            .collect();
        assert!(
            missing_mad.is_empty(),
            "sample did not emit predicated {missing_mad:?}"
        );

        let mul_cfg = GenConfig {
            emit_addc: false,
            emit_subc: false,
            emit_bfind: false,
            emit_mad24: false,
            ..coverage_heavy_config()
        };
        let mut found_mul = vec![false; MUL24_MNEMONICS.len()];
        for seed in 0..2048 {
            let bytes = bytes_from_seed(seed, 32768);
            let ptx = generate_from_bytes_with_config(&bytes, &mul_cfg).unwrap();
            for op in ptx.lines().filter_map(predicated_mnemonic) {
                for (i, mnemonic) in MUL24_MNEMONICS.iter().enumerate() {
                    found_mul[i] |= op == *mnemonic;
                }
            }
            if found_mul.iter().all(|seen| *seen) {
                break;
            }
        }

        let missing_mul: Vec<_> = MUL24_MNEMONICS
            .iter()
            .zip(found_mul)
            .filter_map(|(mnemonic, seen)| (!seen).then_some(*mnemonic))
            .collect();
        assert!(
            missing_mul.is_empty(),
            "sample did not emit predicated {missing_mul:?}"
        );
    }

    #[test]
    fn predicated_24bit_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_addc: false,
            emit_subc: false,
            emit_bfind: false,
            emit_predicated_24bit: false,
            ..GenConfig::default()
        };

        for seed in 0..1024 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            assert!(
                !has_predicated_24bit(&ptx),
                "seed {seed:x} emitted predicated 24-bit instruction"
            );
        }
    }

    #[test]
    fn subword_wide_generation_is_reachable() {
        let cfg = GenConfig {
            emit_addc: false,
            emit_subc: false,
            emit_bfind: false,
            emit_fns: false,
            emit_mad24: false,
            emit_mul24: false,
            emit_predicated_subword_wide: false,
            ..coverage_heavy_config()
        };
        assert_mnemonic_coverage(&cfg, 32768, 8192, SUBWORD_WIDE_MNEMONICS);
    }

    #[test]
    fn subword_wide_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_subword_wide: false,
            ..coverage_heavy_config()
        };

        for seed in 0..2048 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            for mnemonic in SUBWORD_WIDE_MNEMONICS {
                assert!(
                    !has_mnemonic(&ptx, mnemonic),
                    "seed {seed:x} emitted {mnemonic}"
                );
            }
        }
    }

    #[test]
    fn signed_subword_wide_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_signed_subword_wide: false,
            ..coverage_heavy_config()
        };

        let mut saw_unsigned = false;
        for seed in 0..4096 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            for mnemonic in SIGNED_SUBWORD_WIDE_MNEMONICS {
                assert!(
                    !has_mnemonic(&ptx, mnemonic),
                    "seed {seed:x} emitted {mnemonic}"
                );
            }
            saw_unsigned |=
                has_mnemonic(&ptx, "mul.wide.u16") || has_mnemonic(&ptx, "mad.wide.u16");
        }
        assert!(
            saw_unsigned,
            "sample did not retain unsigned subword wide coverage"
        );
    }

    #[test]
    fn predicated_subword_wide_generation_is_reachable() {
        let cfg = GenConfig {
            emit_addc: false,
            emit_subc: false,
            emit_bfind: false,
            emit_fns: false,
            emit_mad24: false,
            emit_mul24: false,
            ..coverage_heavy_config()
        };

        for seed in 0..8192 {
            let bytes = bytes_from_seed(seed, 32768);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            if has_predicated_subword_wide(&ptx) {
                return;
            }
        }

        panic!("sample did not emit predicated subword wide instruction");
    }

    #[test]
    fn predicated_subword_wide_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_predicated_subword_wide: false,
            ..coverage_heavy_config()
        };

        for seed in 0..2048 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            assert!(
                !has_predicated_subword_wide(&ptx),
                "seed {seed:x} emitted predicated subword wide instruction"
            );
        }
    }

    #[test]
    fn mul_wide_generation_is_reachable() {
        let mnemonics = ["mul.wide.u32", "mul.wide.s32"];
        let mut found = [false; 2];

        for seed in 0..8192 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes(&bytes).unwrap();
            let mul_wide_u32_count = ptx
                .lines()
                .filter(|line| line.trim_start().starts_with("mul.wide.u32"))
                .count();
            found[0] |= mul_wide_u32_count > 2; // prologue/epilogue always use two.
            found[1] |= has_mnemonic(&ptx, mnemonics[1]);
            if found.iter().all(|seen| *seen) {
                break;
            }
        }

        let missing: Vec<_> = mnemonics
            .iter()
            .zip(found)
            .filter_map(|(mnemonic, seen)| (!seen).then_some(*mnemonic))
            .collect();
        assert!(missing.is_empty(), "sample did not emit {missing:?}");
    }

    #[test]
    fn mul_wide_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_mul_wide: false,
            ..GenConfig::default()
        };

        for seed in 0..1024 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            let mul_wide_u32_count = ptx
                .lines()
                .filter(|line| line.trim_start().starts_with("mul.wide.u32"))
                .count();
            assert_eq!(
                mul_wide_u32_count, 2,
                "seed {seed:x} emitted body mul.wide.u32"
            );
            assert!(
                !ptx.contains("mul.wide.s32"),
                "seed {seed:x} emitted mul.wide.s32"
            );
        }
    }

    #[test]
    fn predicated_mul_wide_generation_is_reachable() {
        let cfg = GenConfig {
            emit_wide_int: false,
            ..coverage_heavy_config()
        };
        let mut found = vec![false; MUL_WIDE_MNEMONICS.len()];

        for seed in 0..4096 {
            let bytes = bytes_from_seed(seed, 32768);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            for op in ptx.lines().filter_map(predicated_mnemonic) {
                for (i, mnemonic) in MUL_WIDE_MNEMONICS.iter().enumerate() {
                    found[i] |= op == *mnemonic;
                }
            }
            if found.iter().all(|seen| *seen) {
                break;
            }
        }

        let missing: Vec<_> = MUL_WIDE_MNEMONICS
            .iter()
            .zip(found)
            .filter_map(|(mnemonic, seen)| (!seen).then_some(*mnemonic))
            .collect();
        assert!(
            missing.is_empty(),
            "sample did not emit predicated {missing:?}"
        );
    }

    #[test]
    fn predicated_mul_wide_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_wide_int: false,
            emit_predicated_mul_wide: false,
            ..GenConfig::default()
        };

        for seed in 0..1024 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            assert!(
                !has_predicated_mul_wide(&ptx),
                "seed {seed:x} emitted predicated mul.wide"
            );
        }
    }

    #[test]
    fn mad_wide_generation_is_reachable() {
        let cfg = GenConfig {
            emit_mul_wide: false,
            emit_wide_int: false,
            emit_wide_setp: false,
            emit_wide_setp_bool: false,
            emit_wide_selp: false,
            emit_wide_unary: false,
            emit_wide_shifts: false,
            emit_wide_divrem: false,
            emit_predicated_mad_wide: false,
            ..coverage_heavy_config()
        };
        assert_mnemonic_coverage(&cfg, 32768, 4096, MAD_WIDE_MNEMONICS);
    }

    #[test]
    fn mad_wide_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_mad_wide: false,
            ..GenConfig::default()
        };

        for seed in 0..2048 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            for mnemonic in MAD_WIDE_MNEMONICS {
                assert!(
                    !has_mnemonic(&ptx, mnemonic),
                    "seed {seed:x} emitted {mnemonic}"
                );
            }
        }
    }

    #[test]
    fn signed_mad_wide_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_signed_mad_wide: false,
            ..GenConfig::default()
        };

        for seed in 0..2048 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            for mnemonic in SIGNED_MAD_WIDE_MNEMONICS {
                assert!(
                    !has_mnemonic(&ptx, mnemonic),
                    "seed {seed:x} emitted {mnemonic}"
                );
            }
        }
    }

    #[test]
    fn predicated_mad_wide_generation_is_reachable() {
        let cfg = GenConfig {
            emit_mul_wide: false,
            emit_wide_int: false,
            emit_wide_setp: false,
            emit_wide_setp_bool: false,
            emit_wide_selp: false,
            emit_wide_unary: false,
            emit_wide_shifts: false,
            emit_wide_divrem: false,
            ..coverage_heavy_config()
        };
        let mut found = vec![false; MAD_WIDE_MNEMONICS.len()];

        for seed in 0..4096 {
            let bytes = bytes_from_seed(seed, 32768);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            for op in ptx.lines().filter_map(predicated_mnemonic) {
                for (i, mnemonic) in MAD_WIDE_MNEMONICS.iter().enumerate() {
                    found[i] |= op == *mnemonic;
                }
            }
            if found.iter().all(|seen| *seen) {
                break;
            }
        }

        let missing: Vec<_> = MAD_WIDE_MNEMONICS
            .iter()
            .zip(found)
            .filter_map(|(mnemonic, seen)| (!seen).then_some(*mnemonic))
            .collect();
        assert!(
            missing.is_empty(),
            "sample did not emit predicated {missing:?}"
        );
    }

    #[test]
    fn predicated_mad_wide_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_predicated_mad_wide: false,
            ..GenConfig::default()
        };

        for seed in 0..2048 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            assert!(
                !has_predicated_mad_wide(&ptx),
                "seed {seed:x} emitted predicated mad.wide"
            );
        }
    }

    #[test]
    fn wide_mad64_generation_is_reachable() {
        let cfg = GenConfig {
            emit_mul_wide: false,
            emit_mad_wide: false,
            emit_wide_int: false,
            emit_wide_setp: false,
            emit_wide_setp_bool: false,
            emit_wide_selp: false,
            emit_wide_unary: false,
            emit_wide_shifts: false,
            emit_wide_divrem: false,
            emit_wide_addc: false,
            emit_wide_subc: false,
            emit_predicated_wide_mad64: false,
            ..coverage_heavy_config()
        };
        assert_mnemonic_coverage(&cfg, 32768, 8192, WIDE_MAD64_MNEMONICS);
    }

    #[test]
    fn wide_mad64_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_wide_mad64: false,
            emit_predicated_wide_mad64: false,
            ..GenConfig::default()
        };

        for seed in 0..2048 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            for mnemonic in WIDE_MAD64_MNEMONICS {
                assert!(
                    !has_mnemonic(&ptx, mnemonic),
                    "seed {seed:x} emitted {mnemonic}"
                );
            }
            assert!(
                !has_predicated_wide_mad64(&ptx),
                "seed {seed:x} emitted predicated wide mad64"
            );
        }
    }

    #[test]
    fn signed_wide_mad64_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_signed_wide_mad64: false,
            ..GenConfig::default()
        };

        for seed in 0..2048 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            for mnemonic in SIGNED_WIDE_MAD64_MNEMONICS {
                assert!(
                    !has_mnemonic(&ptx, mnemonic),
                    "seed {seed:x} emitted {mnemonic}"
                );
            }
        }
    }

    #[test]
    fn predicated_wide_mad64_generation_is_reachable() {
        let cfg = GenConfig {
            emit_mul_wide: false,
            emit_mad_wide: false,
            emit_wide_int: false,
            emit_wide_setp: false,
            emit_wide_setp_bool: false,
            emit_wide_selp: false,
            emit_wide_unary: false,
            emit_wide_shifts: false,
            emit_wide_divrem: false,
            emit_wide_addc: false,
            emit_wide_subc: false,
            ..coverage_heavy_config()
        };

        for seed in 0..8192 {
            let bytes = bytes_from_seed(seed, 32768);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            if has_predicated_wide_mad64(&ptx) {
                return;
            }
        }

        panic!("sample did not emit predicated wide mad64");
    }

    #[test]
    fn predicated_wide_mad64_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_predicated_wide_mad64: false,
            ..GenConfig::default()
        };

        for seed in 0..2048 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            assert!(
                !has_predicated_wide_mad64(&ptx),
                "seed {seed:x} emitted predicated wide mad64"
            );
        }
    }

    #[test]
    fn wide_high_result_generation_is_reachable() {
        let cfg = GenConfig {
            emit_wide_int: false,
            emit_wide_setp: false,
            emit_wide_setp_bool: false,
            emit_wide_selp: false,
            emit_wide_unary: false,
            emit_wide_shifts: false,
            emit_wide_divrem: false,
            ..coverage_heavy_config()
        };

        let mut saw_high_result = false;
        for seed in 0..4096 {
            let bytes = bytes_from_seed(seed, 32768);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            saw_high_result |= has_wide_high_result(&ptx);
            if saw_high_result {
                break;
            }
        }

        assert!(
            saw_high_result,
            "sample did not emit high-half wide result extraction"
        );
    }

    #[test]
    fn wide_high_result_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_wide_high_result: false,
            ..GenConfig::default()
        };

        for seed in 0..2048 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            assert!(
                !has_wide_high_result(&ptx),
                "seed {seed:x} emitted high-half wide result extraction"
            );
        }
    }

    #[test]
    fn wide_int_generation_is_reachable() {
        let mut found = vec![false; WIDE_INT_MNEMONICS.len()];

        for seed in 0..32768 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes(&bytes).unwrap();
            for (i, mnemonic) in WIDE_INT_MNEMONICS.iter().enumerate() {
                let count = ptx
                    .lines()
                    .filter(|line| line.trim_start().starts_with(mnemonic))
                    .count();
                found[i] |= if *mnemonic == "add.s64" {
                    count > 2 // prologue/epilogue address arithmetic always use two.
                } else {
                    count > 0
                };
            }
            if found.iter().all(|seen| *seen) {
                break;
            }
        }

        let missing: Vec<_> = WIDE_INT_MNEMONICS
            .iter()
            .zip(found)
            .filter_map(|(mnemonic, seen)| (!seen).then_some(*mnemonic))
            .collect();
        assert!(missing.is_empty(), "sample did not emit {missing:?}");
    }

    #[test]
    fn wide_int_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_wide_int: false,
            ..GenConfig::default()
        };

        for seed in 0..2048 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            for mnemonic in [
                "add.u64",
                "sub.u64",
                "mul.lo.u64",
                "mul.hi.u64",
                "min.u64",
                "max.u64",
                "sub.s64",
                "mul.lo.s64",
                "mul.hi.s64",
                "min.s64",
                "max.s64",
                "and.b64",
                "or.b64",
                "xor.b64",
            ] {
                assert!(
                    !has_mnemonic(&ptx, mnemonic),
                    "seed {seed:x} emitted {mnemonic}"
                );
            }
            let add_s64_count = ptx
                .lines()
                .filter(|line| line.trim_start().starts_with("add.s64"))
                .count();
            assert_eq!(add_s64_count, 2, "seed {seed:x} emitted body add.s64");
        }
    }

    #[test]
    fn wide_minmax_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_wide_minmax: false,
            ..GenConfig::default()
        };

        for seed in 0..2048 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            for mnemonic in WIDE_MINMAX_MNEMONICS {
                assert!(
                    !has_mnemonic(&ptx, mnemonic),
                    "seed {seed:x} emitted {mnemonic}"
                );
            }
        }
    }

    #[test]
    fn wide_mulhi_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_wide_mulhi: false,
            ..GenConfig::default()
        };

        for seed in 0..2048 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            for mnemonic in WIDE_MULHI_MNEMONICS {
                assert!(
                    !has_mnemonic(&ptx, mnemonic),
                    "seed {seed:x} emitted {mnemonic}"
                );
            }
        }
    }

    #[test]
    fn wide_set_generation_is_reachable() {
        let cfg = GenConfig {
            emit_mul_wide: false,
            emit_wide_int: false,
            emit_wide_setp: false,
            emit_wide_setp_bool: false,
            emit_wide_selp: false,
            emit_wide_unary: false,
            emit_wide_shifts: false,
            emit_wide_divrem: false,
            ..coverage_heavy_config()
        };
        assert_mnemonic_coverage(&cfg, 32768, 8192, WIDE_SET_MNEMONICS);
    }

    #[test]
    fn wide_set_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_wide_set: false,
            emit_predicated_wide_set: false,
            ..GenConfig::default()
        };

        for seed in 0..2048 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            for mnemonic in WIDE_SET_MNEMONICS {
                assert!(
                    !has_mnemonic(&ptx, mnemonic),
                    "seed {seed:x} emitted {mnemonic}"
                );
            }
            assert!(
                !has_predicated_wide_set(&ptx),
                "seed {seed:x} emitted predicated wide set"
            );
        }
    }

    #[test]
    fn predicated_wide_set_generation_is_reachable() {
        let cfg = GenConfig {
            emit_mul_wide: false,
            emit_wide_int: false,
            emit_wide_setp: false,
            emit_wide_setp_bool: false,
            emit_wide_selp: false,
            emit_wide_unary: false,
            emit_wide_shifts: false,
            emit_wide_divrem: false,
            ..coverage_heavy_config()
        };

        for seed in 0..8192 {
            let bytes = bytes_from_seed(seed, 32768);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            if has_predicated_wide_set(&ptx) {
                return;
            }
        }

        panic!("sample did not emit predicated wide set");
    }

    #[test]
    fn predicated_wide_set_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_predicated_wide_set: false,
            ..GenConfig::default()
        };

        for seed in 0..2048 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            assert!(
                !has_predicated_wide_set(&ptx),
                "seed {seed:x} emitted predicated wide set"
            );
        }
    }

    #[test]
    fn wide_setp_generation_is_reachable() {
        let cfg = GenConfig {
            emit_mul_wide: false,
            emit_wide_int: false,
            emit_wide_setp_bool: false,
            emit_wide_selp: false,
            emit_wide_unary: false,
            emit_wide_shifts: false,
            ..coverage_heavy_config()
        };
        assert_mnemonic_coverage(&cfg, 32768, 8192, WIDE_SETP_MNEMONICS);
    }

    #[test]
    fn wide_setp_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_wide_setp: false,
            emit_wide_setp_bool: false,
            emit_wide_selp: false,
            ..GenConfig::default()
        };

        for seed in 0..2048 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            for mnemonic in WIDE_SETP_MNEMONICS {
                assert!(
                    !has_mnemonic(&ptx, mnemonic),
                    "seed {seed:x} emitted {mnemonic}"
                );
            }
            for mnemonic in WIDE_SELP_MNEMONICS {
                assert!(
                    !has_mnemonic(&ptx, mnemonic),
                    "seed {seed:x} emitted {mnemonic}"
                );
            }
        }
    }

    #[test]
    fn wide_selp_generation_is_reachable() {
        let cfg = GenConfig {
            emit_mul_wide: false,
            emit_wide_int: false,
            emit_wide_setp: false,
            emit_wide_setp_bool: false,
            emit_wide_unary: false,
            emit_wide_shifts: false,
            ..coverage_heavy_config()
        };
        assert_mnemonic_coverage(&cfg, 32768, 1024, WIDE_SELP_MNEMONICS);
    }

    #[test]
    fn wide_selp_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_wide_selp: false,
            ..GenConfig::default()
        };

        for seed in 0..2048 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            for mnemonic in WIDE_SELP_MNEMONICS {
                assert!(
                    !has_mnemonic(&ptx, mnemonic),
                    "seed {seed:x} emitted {mnemonic}"
                );
            }
        }
    }

    #[test]
    fn wide_setp_bool_generation_is_reachable() {
        let cfg = GenConfig {
            emit_mul_wide: false,
            emit_wide_int: false,
            emit_wide_setp: false,
            emit_wide_selp: false,
            emit_wide_unary: false,
            emit_wide_shifts: false,
            ..coverage_heavy_config()
        };
        assert_mnemonic_coverage(&cfg, 32768, 16384, WIDE_SETP_BOOL_MNEMONICS);
    }

    #[test]
    fn wide_setp_bool_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_wide_setp_bool: false,
            ..GenConfig::default()
        };

        for seed in 0..2048 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            for mnemonic in WIDE_SETP_BOOL_MNEMONICS {
                assert!(
                    !has_mnemonic(&ptx, mnemonic),
                    "seed {seed:x} emitted {mnemonic}"
                );
            }
        }
    }

    #[test]
    fn wide_unary_generation_is_reachable() {
        let cfg = GenConfig {
            emit_mul_wide: false,
            emit_wide_int: false,
            emit_wide_setp: false,
            emit_wide_setp_bool: false,
            emit_wide_selp: false,
            emit_predicated_wide_unary: false,
            emit_wide_shifts: false,
            ..coverage_heavy_config()
        };
        assert_mnemonic_coverage(&cfg, 32768, 4096, WIDE_UNARY_MNEMONICS);
    }

    #[test]
    fn wide_unary_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_wide_unary: false,
            ..GenConfig::default()
        };

        for seed in 0..2048 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            for mnemonic in WIDE_UNARY_MNEMONICS {
                assert!(
                    !has_mnemonic(&ptx, mnemonic),
                    "seed {seed:x} emitted {mnemonic}"
                );
            }
            assert!(
                !has_predicated_wide_unary(&ptx),
                "seed {seed:x} emitted predicated wide unary"
            );
        }
    }

    #[test]
    fn signed_wide_unary_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_signed_wide_unary: false,
            ..GenConfig::default()
        };

        for seed in 0..2048 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            assert!(
                !has_signed_wide_unary_mnemonic(&ptx),
                "seed {seed:x} emitted signed wide unary"
            );
        }
    }

    #[test]
    fn predicated_wide_unary_generation_is_reachable() {
        let cfg = GenConfig {
            emit_mul_wide: false,
            emit_wide_int: false,
            emit_wide_setp: false,
            emit_wide_setp_bool: false,
            emit_wide_selp: false,
            emit_wide_shifts: false,
            ..coverage_heavy_config()
        };
        let mut found = vec![false; WIDE_UNARY_MNEMONICS.len()];

        for seed in 0..8192 {
            let bytes = bytes_from_seed(seed, 32768);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            for op in ptx.lines().filter_map(predicated_mnemonic) {
                for (i, mnemonic) in WIDE_UNARY_MNEMONICS.iter().enumerate() {
                    found[i] |= op == *mnemonic;
                }
            }
            if found.iter().all(|seen| *seen) {
                break;
            }
        }

        let missing: Vec<_> = WIDE_UNARY_MNEMONICS
            .iter()
            .zip(found)
            .filter_map(|(mnemonic, seen)| (!seen).then_some(*mnemonic))
            .collect();
        assert!(
            missing.is_empty(),
            "sample did not emit predicated {missing:?}"
        );
    }

    #[test]
    fn predicated_wide_unary_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_predicated_wide_unary: false,
            ..GenConfig::default()
        };

        for seed in 0..2048 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            assert!(
                !has_predicated_wide_unary(&ptx),
                "seed {seed:x} emitted predicated wide unary"
            );
        }
    }

    #[test]
    fn predicated_wide_int_generation_is_reachable() {
        let cfg = GenConfig {
            emit_mul_wide: false,
            ..coverage_heavy_config()
        };
        let mut found = vec![false; WIDE_INT_MNEMONICS.len()];

        for seed in 0..8192 {
            let bytes = bytes_from_seed(seed, 32768);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            for op in ptx.lines().filter_map(predicated_mnemonic) {
                for (i, mnemonic) in WIDE_INT_MNEMONICS.iter().enumerate() {
                    found[i] |= op == *mnemonic;
                }
            }
            if found.iter().all(|seen| *seen) {
                break;
            }
        }

        let missing: Vec<_> = WIDE_INT_MNEMONICS
            .iter()
            .zip(found)
            .filter_map(|(mnemonic, seen)| (!seen).then_some(*mnemonic))
            .collect();
        assert!(
            missing.is_empty(),
            "sample did not emit predicated {missing:?}"
        );
    }

    #[test]
    fn predicated_wide_int_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_mul_wide: false,
            emit_predicated_wide_int: false,
            ..GenConfig::default()
        };

        for seed in 0..1024 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            assert!(
                !has_predicated_wide_int(&ptx),
                "seed {seed:x} emitted predicated wide int"
            );
        }
    }

    #[test]
    fn wide_shift_generation_is_reachable() {
        let mut found = vec![false; WIDE_SHIFT_MNEMONICS.len()];

        for seed in 0..8192 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes(&bytes).unwrap();
            for (i, mnemonic) in WIDE_SHIFT_MNEMONICS.iter().enumerate() {
                found[i] |= has_mnemonic(&ptx, mnemonic);
            }
            if found.iter().all(|seen| *seen) {
                break;
            }
        }

        let missing: Vec<_> = WIDE_SHIFT_MNEMONICS
            .iter()
            .zip(found)
            .filter_map(|(mnemonic, seen)| (!seen).then_some(*mnemonic))
            .collect();
        assert!(missing.is_empty(), "sample did not emit {missing:?}");
    }

    #[test]
    fn wide_shift_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_wide_shifts: false,
            ..GenConfig::default()
        };

        for seed in 0..1024 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            for mnemonic in WIDE_SHIFT_MNEMONICS {
                assert!(
                    !has_mnemonic(&ptx, mnemonic),
                    "seed {seed:x} emitted {mnemonic}"
                );
            }
        }
    }

    #[test]
    fn wide_register_shift_generation_is_reachable() {
        let cfg = GenConfig {
            emit_mul_wide: false,
            emit_wide_int: false,
            emit_predicated_wide_reg_shifts: false,
            ..coverage_heavy_config()
        };
        let mut found = vec![false; WIDE_SHIFT_MNEMONICS.len()];

        for seed in 0..8192 {
            let bytes = bytes_from_seed(seed, 32768);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            for line in ptx.lines() {
                if !is_wide_reg_shift_line(line, false) {
                    continue;
                }
                let op = line.trim_start().split_whitespace().next().unwrap();
                for (i, mnemonic) in WIDE_SHIFT_MNEMONICS.iter().enumerate() {
                    found[i] |= op == *mnemonic;
                }
            }
            if found.iter().all(|seen| *seen) {
                break;
            }
        }

        let missing: Vec<_> = WIDE_SHIFT_MNEMONICS
            .iter()
            .zip(found)
            .filter_map(|(mnemonic, seen)| (!seen).then_some(*mnemonic))
            .collect();
        assert!(
            missing.is_empty(),
            "sample did not emit register-count {missing:?}"
        );
    }

    #[test]
    fn wide_register_shift_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_wide_reg_shifts: false,
            ..GenConfig::default()
        };

        for seed in 0..2048 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            assert!(
                !has_wide_reg_shift(&ptx),
                "seed {seed:x} emitted register-count wide shift"
            );
        }
    }

    #[test]
    fn predicated_wide_shift_generation_is_reachable() {
        let cfg = GenConfig {
            emit_mul_wide: false,
            emit_wide_int: false,
            emit_predicated_wide_reg_shifts: false,
            ..coverage_heavy_config()
        };
        let mut found = vec![false; WIDE_SHIFT_MNEMONICS.len()];

        for seed in 0..8192 {
            let bytes = bytes_from_seed(seed, 32768);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            for op in ptx.lines().filter_map(predicated_mnemonic) {
                for (i, mnemonic) in WIDE_SHIFT_MNEMONICS.iter().enumerate() {
                    found[i] |= op == *mnemonic;
                }
            }
            if found.iter().all(|seen| *seen) {
                break;
            }
        }

        let missing: Vec<_> = WIDE_SHIFT_MNEMONICS
            .iter()
            .zip(found)
            .filter_map(|(mnemonic, seen)| (!seen).then_some(*mnemonic))
            .collect();
        assert!(
            missing.is_empty(),
            "sample did not emit predicated {missing:?}"
        );
    }

    #[test]
    fn predicated_wide_shift_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_mul_wide: false,
            emit_wide_int: false,
            emit_predicated_wide_shifts: false,
            emit_predicated_wide_reg_shifts: false,
            ..GenConfig::default()
        };

        for seed in 0..1024 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            assert!(
                !has_predicated_wide_shift(&ptx),
                "seed {seed:x} emitted predicated wide shift"
            );
        }
    }

    #[test]
    fn predicated_wide_register_shift_generation_is_reachable() {
        let cfg = GenConfig {
            emit_mul_wide: false,
            emit_wide_int: false,
            emit_predicated_wide_shifts: false,
            ..coverage_heavy_config()
        };
        let mut found = vec![false; WIDE_SHIFT_MNEMONICS.len()];

        for seed in 0..8192 {
            let bytes = bytes_from_seed(seed, 32768);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            for line in ptx.lines() {
                if !is_wide_reg_shift_line(line, true) {
                    continue;
                }
                let op = predicated_mnemonic(line).unwrap();
                for (i, mnemonic) in WIDE_SHIFT_MNEMONICS.iter().enumerate() {
                    found[i] |= op == *mnemonic;
                }
            }
            if found.iter().all(|seen| *seen) {
                break;
            }
        }

        let missing: Vec<_> = WIDE_SHIFT_MNEMONICS
            .iter()
            .zip(found)
            .filter_map(|(mnemonic, seen)| (!seen).then_some(*mnemonic))
            .collect();
        assert!(
            missing.is_empty(),
            "sample did not emit predicated register-count {missing:?}"
        );
    }

    #[test]
    fn predicated_wide_register_shift_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_predicated_wide_reg_shifts: false,
            ..GenConfig::default()
        };

        for seed in 0..2048 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            assert!(
                !has_predicated_wide_reg_shift(&ptx),
                "seed {seed:x} emitted predicated register-count wide shift"
            );
        }
    }

    #[test]
    fn wide_divrem_generation_is_reachable() {
        let cfg = GenConfig {
            emit_mul_wide: false,
            emit_wide_int: false,
            emit_wide_setp: false,
            emit_wide_setp_bool: false,
            emit_wide_selp: false,
            emit_wide_unary: false,
            emit_wide_shifts: false,
            emit_predicated_wide_divrem: false,
            ..coverage_heavy_config()
        };
        assert_mnemonic_coverage(&cfg, 32768, 8192, WIDE_DIVREM_MNEMONICS);
    }

    #[test]
    fn wide_divrem_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_wide_divrem: false,
            ..GenConfig::default()
        };

        for seed in 0..2048 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            for mnemonic in WIDE_DIVREM_MNEMONICS {
                assert!(
                    !has_mnemonic(&ptx, mnemonic),
                    "seed {seed:x} emitted {mnemonic}"
                );
            }
            assert!(
                !has_predicated_wide_divrem(&ptx),
                "seed {seed:x} emitted predicated wide div/rem"
            );
            assert!(
                !has_wide_reg_divrem(&ptx),
                "seed {seed:x} emitted register-divisor wide div/rem"
            );
            assert!(
                !has_predicated_wide_reg_divrem(&ptx),
                "seed {seed:x} emitted predicated register-divisor wide div/rem"
            );
        }
    }

    #[test]
    fn signed_wide_divrem_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_signed_wide_divrem: false,
            ..GenConfig::default()
        };

        for seed in 0..2048 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            for mnemonic in SIGNED_WIDE_DIVREM_MNEMONICS {
                assert!(
                    !has_mnemonic(&ptx, mnemonic),
                    "seed {seed:x} emitted {mnemonic}"
                );
            }
        }
    }

    #[test]
    fn wide_reg_divrem_generation_is_reachable() {
        let cfg = GenConfig {
            emit_mul_wide: false,
            emit_wide_int: false,
            emit_wide_setp: false,
            emit_wide_setp_bool: false,
            emit_wide_selp: false,
            emit_wide_unary: false,
            emit_wide_shifts: false,
            emit_predicated_wide_divrem: false,
            ..coverage_heavy_config()
        };

        for seed in 0..8192 {
            let bytes = bytes_from_seed(seed, 32768);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            if has_wide_reg_divrem(&ptx) {
                return;
            }
        }

        panic!("sample did not emit register-divisor wide div/rem");
    }

    #[test]
    fn wide_reg_divrem_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_reg_wide_divrem: false,
            ..GenConfig::default()
        };

        for seed in 0..2048 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            assert!(
                !has_wide_reg_divrem(&ptx),
                "seed {seed:x} emitted register-divisor wide div/rem"
            );
        }
    }

    #[test]
    fn predicated_wide_divrem_generation_is_reachable() {
        let cfg = GenConfig {
            emit_mul_wide: false,
            emit_wide_int: false,
            emit_wide_setp: false,
            emit_wide_setp_bool: false,
            emit_wide_selp: false,
            emit_wide_unary: false,
            emit_wide_shifts: false,
            ..coverage_heavy_config()
        };
        let mut found = vec![false; WIDE_DIVREM_MNEMONICS.len()];

        for seed in 0..8192 {
            let bytes = bytes_from_seed(seed, 32768);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            for op in ptx.lines().filter_map(predicated_mnemonic) {
                for (i, mnemonic) in WIDE_DIVREM_MNEMONICS.iter().enumerate() {
                    found[i] |= op == *mnemonic;
                }
            }
            if found.iter().all(|seen| *seen) {
                break;
            }
        }

        let missing: Vec<_> = WIDE_DIVREM_MNEMONICS
            .iter()
            .zip(found)
            .filter_map(|(mnemonic, seen)| (!seen).then_some(*mnemonic))
            .collect();
        assert!(
            missing.is_empty(),
            "sample did not emit predicated {missing:?}"
        );
    }

    #[test]
    fn predicated_wide_divrem_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_predicated_wide_divrem: false,
            emit_predicated_reg_wide_divrem: false,
            ..GenConfig::default()
        };

        for seed in 0..2048 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            assert!(
                !has_predicated_wide_divrem(&ptx),
                "seed {seed:x} emitted predicated wide div/rem"
            );
            assert!(
                !has_predicated_wide_reg_divrem(&ptx),
                "seed {seed:x} emitted predicated register-divisor wide div/rem"
            );
        }
    }

    #[test]
    fn predicated_wide_reg_divrem_generation_is_reachable() {
        let cfg = GenConfig {
            emit_mul_wide: false,
            emit_wide_int: false,
            emit_wide_setp: false,
            emit_wide_setp_bool: false,
            emit_wide_selp: false,
            emit_wide_unary: false,
            emit_wide_shifts: false,
            ..coverage_heavy_config()
        };

        for seed in 0..8192 {
            let bytes = bytes_from_seed(seed, 32768);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            if has_predicated_wide_reg_divrem(&ptx) {
                return;
            }
        }

        panic!("sample did not emit predicated register-divisor wide div/rem");
    }

    #[test]
    fn predicated_wide_reg_divrem_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_predicated_reg_wide_divrem: false,
            ..GenConfig::default()
        };

        for seed in 0..2048 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            assert!(
                !has_predicated_wide_reg_divrem(&ptx),
                "seed {seed:x} emitted predicated register-divisor wide div/rem"
            );
        }
    }

    #[test]
    fn wide_carry_generation_is_reachable() {
        let cfg = GenConfig {
            emit_mul_wide: false,
            emit_mad_wide: false,
            emit_wide_int: false,
            emit_wide_mad64: false,
            emit_wide_setp: false,
            emit_wide_setp_bool: false,
            emit_wide_selp: false,
            emit_wide_unary: false,
            emit_wide_shifts: false,
            emit_wide_divrem: false,
            emit_predicated_wide_carry: false,
            ..coverage_heavy_config()
        };
        assert_mnemonic_coverage(&cfg, 32768, 8192, WIDE_CARRY_MNEMONICS);
    }

    #[test]
    fn wide_addc_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_wide_addc: false,
            ..GenConfig::default()
        };

        for seed in 0..2048 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            for mnemonic in WIDE_ADDC_MNEMONICS {
                assert!(
                    !has_mnemonic(&ptx, mnemonic),
                    "seed {seed:x} emitted {mnemonic}"
                );
            }
        }
    }

    #[test]
    fn wide_subc_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_wide_subc: false,
            ..GenConfig::default()
        };

        for seed in 0..2048 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            for mnemonic in WIDE_SUBC_MNEMONICS {
                assert!(
                    !has_mnemonic(&ptx, mnemonic),
                    "seed {seed:x} emitted {mnemonic}"
                );
            }
        }
    }

    #[test]
    fn predicated_wide_carry_generation_is_reachable() {
        let cfg = GenConfig {
            emit_mul_wide: false,
            emit_mad_wide: false,
            emit_wide_int: false,
            emit_wide_mad64: false,
            emit_wide_setp: false,
            emit_wide_setp_bool: false,
            emit_wide_selp: false,
            emit_wide_unary: false,
            emit_wide_shifts: false,
            emit_wide_divrem: false,
            ..coverage_heavy_config()
        };

        for seed in 0..8192 {
            let bytes = bytes_from_seed(seed, 32768);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            if has_predicated_wide_carry(&ptx) {
                return;
            }
        }

        panic!("sample did not emit predicated wide carry");
    }

    #[test]
    fn predicated_wide_carry_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_predicated_wide_carry: false,
            emit_predicated_wide_carry_chain: false,
            ..GenConfig::default()
        };

        for seed in 0..2048 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            assert!(
                !has_predicated_wide_carry(&ptx),
                "seed {seed:x} emitted predicated wide carry"
            );
        }
    }

    #[test]
    fn wide_carry_chain_generation_is_reachable() {
        let cfg = GenConfig {
            emit_mul_wide: false,
            emit_mad_wide: false,
            emit_wide_int: false,
            emit_wide_mad64: false,
            emit_wide_setp: false,
            emit_wide_setp_bool: false,
            emit_wide_selp: false,
            emit_wide_unary: false,
            emit_wide_shifts: false,
            emit_wide_divrem: false,
            emit_predicated_wide_carry_chain: false,
            ..coverage_heavy_config()
        };
        assert_mnemonic_coverage(&cfg, 32768, 8192, WIDE_CARRY_CHAIN_CC_MNEMONICS);
    }

    #[test]
    fn wide_carry_chain_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_wide_carry_chain: false,
            ..coverage_heavy_config()
        };

        for seed in 0..2048 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            for mnemonic in WIDE_CARRY_CHAIN_CC_MNEMONICS {
                assert!(
                    !has_mnemonic(&ptx, mnemonic),
                    "seed {seed:x} emitted {mnemonic}"
                );
            }
        }
    }

    #[test]
    fn predicated_wide_carry_chain_generation_is_reachable() {
        let cfg = GenConfig {
            emit_mul_wide: false,
            emit_mad_wide: false,
            emit_wide_int: false,
            emit_wide_mad64: false,
            emit_wide_setp: false,
            emit_wide_setp_bool: false,
            emit_wide_selp: false,
            emit_wide_unary: false,
            emit_wide_shifts: false,
            emit_wide_divrem: false,
            ..coverage_heavy_config()
        };

        for seed in 0..8192 {
            let bytes = bytes_from_seed(seed, 32768);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            if has_predicated_wide_carry_chain(&ptx) {
                return;
            }
        }

        panic!("sample did not emit predicated wide carry chain");
    }

    #[test]
    fn predicated_wide_carry_chain_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_predicated_wide_carry_chain: false,
            ..coverage_heavy_config()
        };

        for seed in 0..2048 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            assert!(
                !has_predicated_wide_carry_chain(&ptx),
                "seed {seed:x} emitted predicated wide carry chain"
            );
        }
    }

    #[test]
    fn addc_generation_is_reachable() {
        let mut saw_addc = false;

        for seed in 0..4096 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes(&bytes).unwrap();
            if ptx.contains("add.cc.u32") && ptx.contains("addc.u32") {
                saw_addc = true;
                break;
            }
        }

        assert!(saw_addc, "no seed in sample emitted addc");
    }

    #[test]
    fn addc_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_addc: false,
            ..GenConfig::default()
        };

        for seed in 0..512 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            for mnemonic in ["add.cc.u32", "addc.u32"] {
                assert!(!ptx.contains(mnemonic), "seed {seed:x} emitted {mnemonic}");
            }
        }
    }

    #[test]
    fn subc_generation_is_reachable_when_addc_is_disabled() {
        let cfg = GenConfig {
            emit_addc: false,
            ..GenConfig::default()
        };
        let mut saw_subc = false;

        for seed in 0..4096 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            if ptx.contains("sub.cc.u32") && ptx.contains("subc.u32") {
                saw_subc = true;
                break;
            }
        }

        assert!(saw_subc, "no seed in sample emitted subc");
    }

    #[test]
    fn subc_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_subc: false,
            ..GenConfig::default()
        };

        for seed in 0..512 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            for mnemonic in ["sub.cc.u32", "subc.u32"] {
                assert!(!ptx.contains(mnemonic), "seed {seed:x} emitted {mnemonic}");
            }
        }
    }

    #[test]
    fn predicated_carry_generation_is_reachable() {
        let cfg = GenConfig {
            emit_bfind: false,
            emit_mad24: false,
            emit_mul24: false,
            ..coverage_heavy_config()
        };
        let mut found = vec![false; CARRY_MNEMONICS.len()];

        for seed in 0..8192 {
            let bytes = bytes_from_seed(seed, 32768);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            for op in ptx.lines().filter_map(predicated_mnemonic) {
                for (i, mnemonic) in CARRY_MNEMONICS.iter().enumerate() {
                    found[i] |= op == *mnemonic;
                }
            }
            if found.iter().all(|seen| *seen) {
                break;
            }
        }

        let missing: Vec<_> = CARRY_MNEMONICS
            .iter()
            .zip(found)
            .filter_map(|(mnemonic, seen)| (!seen).then_some(*mnemonic))
            .collect();
        assert!(
            missing.is_empty(),
            "sample did not emit predicated {missing:?}"
        );
    }

    #[test]
    fn predicated_carry_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_bfind: false,
            emit_mad24: false,
            emit_mul24: false,
            emit_predicated_carry: false,
            emit_predicated_carry_chain: false,
            ..GenConfig::default()
        };

        for seed in 0..2048 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            assert!(
                !has_predicated_carry(&ptx),
                "seed {seed:x} emitted predicated carry pair"
            );
        }
    }

    #[test]
    fn carry_chain_generation_is_reachable() {
        let cfg = GenConfig {
            emit_bfind: false,
            emit_fns: false,
            emit_mad24: false,
            emit_mul24: false,
            emit_mad_carry: false,
            emit_subword_wide: false,
            emit_predicated_carry_chain: false,
            ..coverage_heavy_config()
        };
        assert_mnemonic_coverage(&cfg, 32768, 8192, CARRY_CHAIN_CC_MNEMONICS);
    }

    #[test]
    fn carry_chain_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_carry_chain: false,
            ..coverage_heavy_config()
        };

        for seed in 0..2048 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            for mnemonic in CARRY_CHAIN_CC_MNEMONICS {
                assert!(
                    !has_mnemonic(&ptx, mnemonic),
                    "seed {seed:x} emitted {mnemonic}"
                );
            }
        }
    }

    #[test]
    fn predicated_carry_chain_generation_is_reachable() {
        let cfg = GenConfig {
            emit_bfind: false,
            emit_fns: false,
            emit_mad24: false,
            emit_mul24: false,
            emit_mad_carry: false,
            emit_subword_wide: false,
            ..coverage_heavy_config()
        };

        for seed in 0..8192 {
            let bytes = bytes_from_seed(seed, 32768);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            if has_predicated_carry_chain(&ptx) {
                return;
            }
        }

        panic!("sample did not emit predicated carry chain");
    }

    #[test]
    fn predicated_carry_chain_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_predicated_carry_chain: false,
            ..coverage_heavy_config()
        };

        for seed in 0..2048 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            assert!(
                !has_predicated_carry_chain(&ptx),
                "seed {seed:x} emitted predicated carry chain"
            );
        }
    }

    #[test]
    fn i32_boundary_immediates_can_be_disabled() {
        let cfg = GenConfig {
            emit_i32_boundary_immediates: false,
            ..GenConfig::default()
        };

        for seed in 0..512 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            for imm in ["2147483647", "2147483648"] {
                assert!(!ptx.contains(imm), "seed {seed:x} emitted {imm}");
            }
        }
    }

    #[test]
    fn i32_boundary_suppression_covers_nearby_values() {
        let max_small = u32::MAX;
        assert_eq!(sanitize_imm32(0x7FFF_FEFF, max_small, false), 0x7FFF_FEFF);
        for imm in [
            0x7FFF_FF00,
            0x7FFF_FFFE,
            0x7FFF_FFFF,
            0x8000_0000,
            0x8000_0001,
            0x8000_00FF,
        ] {
            assert_eq!(sanitize_imm32(imm, max_small, false), 0x7FFF_FEFF);
        }
        assert_eq!(sanitize_imm32(0x8000_0100, max_small, false), 0x8000_0100);
        assert_eq!(sanitize_imm32(0x7FFF_FFFE, max_small, true), 0x7FFF_FFFE);
    }

    #[test]
    fn signed_cmp_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_signed_cmp: false,
            ..GenConfig::default()
        };

        for seed in 0..256 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            for mnemonic in [
                "setp.lt.s32",
                "setp.le.s32",
                "setp.gt.s32",
                "setp.ge.s32",
                "set.lt.u32.s32",
                "set.le.u32.s32",
                "set.gt.u32.s32",
                "set.ge.u32.s32",
                "setp.lt.s16",
                "setp.le.s16",
                "setp.gt.s16",
                "setp.ge.s16",
                "set.lt.u32.s16",
                "set.le.u32.s16",
                "set.gt.u32.s16",
                "set.ge.u32.s16",
                "selp.s16",
            ] {
                assert!(!ptx.contains(mnemonic), "seed {seed:x} emitted {mnemonic}");
            }
        }
    }

    #[test]
    fn cvt_generation_is_reachable() {
        let mnemonics = [
            "cvt.u32.u8",
            "cvt.u32.u16",
            "cvt.s32.u8",
            "cvt.s32.u16",
            "cvt.u32.s8",
            "cvt.u32.s16",
            "cvt.s32.s8",
            "cvt.s32.s16",
        ];
        let mut found = [false; 8];

        for seed in 0..4096 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes(&bytes).unwrap();
            for (i, mnemonic) in mnemonics.iter().enumerate() {
                found[i] |= has_mnemonic(&ptx, mnemonic);
            }
            if found.iter().all(|seen| *seen) {
                break;
            }
        }

        let missing: Vec<_> = mnemonics
            .iter()
            .zip(found)
            .filter_map(|(mnemonic, seen)| (!seen).then_some(*mnemonic))
            .collect();
        assert!(missing.is_empty(), "sample did not emit {missing:?}");
    }

    #[test]
    fn cvt_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_cvt: false,
            emit_narrow_cvt: false,
            emit_scalar_16bit: false,
            ..GenConfig::default()
        };

        for seed in 0..1024 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            for mnemonic in CVT_MNEMONICS {
                assert!(
                    !has_mnemonic(&ptx, mnemonic),
                    "seed {seed:x} emitted {mnemonic}"
                );
            }
        }
    }

    #[test]
    fn predicated_cvt_generation_is_reachable() {
        let cfg = coverage_heavy_config();
        let mut found = vec![false; CVT_MNEMONICS.len()];

        for seed in 0..8192 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            for op in ptx.lines().filter_map(predicated_mnemonic) {
                for (i, mnemonic) in CVT_MNEMONICS.iter().enumerate() {
                    found[i] |= op == *mnemonic;
                }
            }
            if found.iter().all(|seen| *seen) {
                break;
            }
        }

        let missing: Vec<_> = CVT_MNEMONICS
            .iter()
            .zip(found)
            .filter_map(|(mnemonic, seen)| (!seen).then_some(*mnemonic))
            .collect();
        assert!(
            missing.is_empty(),
            "sample did not emit predicated {missing:?}"
        );
    }

    #[test]
    fn predicated_cvt_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_predicated_cvt: false,
            emit_predicated_narrow_cvt: false,
            emit_predicated_scalar_16bit: false,
            ..GenConfig::default()
        };

        for seed in 0..512 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            assert!(
                !has_predicated_cvt(&ptx),
                "seed {seed:x} emitted predicated cvt"
            );
        }
    }

    #[test]
    fn narrow_cvt_generation_is_reachable() {
        let cfg = GenConfig {
            emit_wide_cvt: false,
            emit_szext: false,
            emit_predicated_narrow_cvt: false,
            ..coverage_heavy_config()
        };
        assert_mnemonic_coverage(&cfg, 32768, 4096, NARROW_CVT_MNEMONICS);
    }

    #[test]
    fn narrow_cvt_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_narrow_cvt: false,
            emit_predicated_narrow_cvt: false,
            emit_subword_wide: false,
            emit_scalar_16bit: false,
            ..GenConfig::default()
        };

        for seed in 0..2048 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            for mnemonic in NARROW_CVT_MNEMONICS {
                assert!(
                    !has_mnemonic(&ptx, mnemonic),
                    "seed {seed:x} emitted {mnemonic}"
                );
            }
            assert!(
                !has_predicated_narrow_cvt(&ptx),
                "seed {seed:x} emitted predicated narrow cvt"
            );
        }
    }

    #[test]
    fn signed_narrow_cvt_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_signed_narrow_cvt: false,
            emit_signed_subword_wide: false,
            emit_signed_scalar_16bit: false,
            ..GenConfig::default()
        };

        for seed in 0..2048 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            for mnemonic in SIGNED_NARROW_CVT_MNEMONICS {
                assert!(
                    !has_mnemonic(&ptx, mnemonic),
                    "seed {seed:x} emitted {mnemonic}"
                );
            }
        }
    }

    #[test]
    fn predicated_narrow_cvt_generation_is_reachable() {
        let cfg = GenConfig {
            emit_wide_cvt: false,
            emit_szext: false,
            ..coverage_heavy_config()
        };

        for seed in 0..8192 {
            let bytes = bytes_from_seed(seed, 32768);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            if has_predicated_narrow_cvt(&ptx) {
                return;
            }
        }

        panic!("sample did not emit predicated narrow cvt");
    }

    #[test]
    fn predicated_narrow_cvt_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_predicated_narrow_cvt: false,
            ..GenConfig::default()
        };

        for seed in 0..2048 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            assert!(
                !has_predicated_narrow_cvt(&ptx),
                "seed {seed:x} emitted predicated narrow cvt"
            );
        }
    }

    #[test]
    fn wide_cvt_generation_is_reachable() {
        let cfg = GenConfig {
            emit_narrow_cvt: false,
            emit_szext: false,
            emit_predicated_wide_cvt: false,
            ..coverage_heavy_config()
        };
        assert_mnemonic_coverage(&cfg, 32768, 4096, WIDE_CVT_MNEMONICS);
    }

    #[test]
    fn wide_cvt_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_wide_cvt: false,
            emit_predicated_wide_cvt: false,
            ..GenConfig::default()
        };

        for seed in 0..2048 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            for mnemonic in WIDE_CVT_MNEMONICS {
                assert!(
                    !has_mnemonic(&ptx, mnemonic),
                    "seed {seed:x} emitted {mnemonic}"
                );
            }
            assert!(
                !has_predicated_wide_cvt(&ptx),
                "seed {seed:x} emitted predicated wide cvt"
            );
        }
    }

    #[test]
    fn signed_wide_cvt_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_signed_wide_cvt: false,
            ..GenConfig::default()
        };

        for seed in 0..2048 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            for mnemonic in SIGNED_WIDE_CVT_MNEMONICS {
                assert!(
                    !has_mnemonic(&ptx, mnemonic),
                    "seed {seed:x} emitted {mnemonic}"
                );
            }
        }
    }

    #[test]
    fn predicated_wide_cvt_generation_is_reachable() {
        let cfg = GenConfig {
            emit_narrow_cvt: false,
            emit_szext: false,
            ..coverage_heavy_config()
        };

        for seed in 0..8192 {
            let bytes = bytes_from_seed(seed, 32768);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            if has_predicated_wide_cvt(&ptx) {
                return;
            }
        }

        panic!("sample did not emit predicated wide cvt");
    }

    #[test]
    fn predicated_wide_cvt_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_predicated_wide_cvt: false,
            ..GenConfig::default()
        };

        for seed in 0..2048 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            assert!(
                !has_predicated_wide_cvt(&ptx),
                "seed {seed:x} emitted predicated wide cvt"
            );
        }
    }

    #[test]
    fn szext_generation_is_reachable() {
        let cfg = GenConfig {
            emit_predicated_szext: false,
            ..coverage_heavy_config()
        };
        assert_mnemonic_coverage(&cfg, 32768, 4096, SZEXT_MNEMONICS);
    }

    #[test]
    fn szext_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_szext: false,
            ..coverage_heavy_config()
        };

        for seed in 0..1024 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            for mnemonic in SZEXT_MNEMONICS {
                assert!(!ptx.contains(mnemonic), "seed {seed:x} emitted {mnemonic}");
            }
        }
    }

    #[test]
    fn signed_szext_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_signed_szext: false,
            ..coverage_heavy_config()
        };

        for seed in 0..1024 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            for mnemonic in SIGNED_SZEXT_MNEMONICS {
                assert!(!ptx.contains(mnemonic), "seed {seed:x} emitted {mnemonic}");
            }
        }
    }

    #[test]
    fn predicated_szext_generation_is_reachable() {
        let cfg = coverage_heavy_config();
        let mut found = vec![false; SZEXT_MNEMONICS.len()];

        for seed in 0..8192 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            for op in ptx.lines().filter_map(predicated_mnemonic) {
                for (i, mnemonic) in SZEXT_MNEMONICS.iter().enumerate() {
                    found[i] |= op == *mnemonic;
                }
            }
            if found.iter().all(|seen| *seen) {
                break;
            }
        }

        let missing: Vec<_> = SZEXT_MNEMONICS
            .iter()
            .zip(found)
            .filter_map(|(mnemonic, seen)| (!seen).then_some(*mnemonic))
            .collect();
        assert!(
            missing.is_empty(),
            "sample did not emit predicated {missing:?}"
        );
    }

    #[test]
    fn predicated_szext_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_predicated_szext: false,
            ..coverage_heavy_config()
        };

        for seed in 0..1024 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            assert!(
                !has_predicated_szext(&ptx),
                "seed {seed:x} emitted predicated szext"
            );
        }
    }

    #[test]
    fn fns_generation_is_reachable() {
        let cfg = GenConfig {
            emit_bfind: false,
            emit_addc: false,
            emit_subc: false,
            emit_mad24: false,
            emit_mul24: false,
            emit_predicated_fns: false,
            ..coverage_heavy_config()
        };
        assert_mnemonic_coverage(&cfg, 32768, 4096, FNS_MNEMONICS);
    }

    #[test]
    fn fns_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_fns: false,
            ..coverage_heavy_config()
        };

        for seed in 0..1024 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            for mnemonic in FNS_MNEMONICS {
                assert!(!ptx.contains(mnemonic), "seed {seed:x} emitted {mnemonic}");
            }
        }
    }

    #[test]
    fn predicated_fns_generation_is_reachable() {
        let cfg = GenConfig {
            emit_bfind: false,
            emit_addc: false,
            emit_subc: false,
            emit_mad24: false,
            emit_mul24: false,
            ..coverage_heavy_config()
        };

        for seed in 0..4096 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            if has_predicated_fns(&ptx) {
                return;
            }
        }

        panic!("sample did not emit predicated fns.b32");
    }

    #[test]
    fn predicated_fns_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_predicated_fns: false,
            emit_predicated_reg_fns: false,
            ..coverage_heavy_config()
        };

        for seed in 0..1024 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            assert!(
                !has_predicated_fns(&ptx),
                "seed {seed:x} emitted predicated fns"
            );
        }
    }

    #[test]
    fn register_fns_generation_is_reachable() {
        let cfg = GenConfig {
            emit_bfind: false,
            emit_addc: false,
            emit_subc: false,
            emit_mad24: false,
            emit_mul24: false,
            emit_predicated_reg_fns: false,
            ..coverage_heavy_config()
        };

        for seed in 0..4096 {
            let bytes = bytes_from_seed(seed, 32768);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            if has_register_fns_param(&ptx) {
                return;
            }
        }

        panic!("sample did not emit fns.b32 with register base/offset");
    }

    #[test]
    fn register_fns_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_reg_fns: false,
            ..coverage_heavy_config()
        };

        for seed in 0..1024 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            assert!(
                !has_register_fns_param(&ptx),
                "seed {seed:x} emitted fns.b32 with register base/offset"
            );
        }
    }

    #[test]
    fn predicated_register_fns_generation_is_reachable() {
        let cfg = GenConfig {
            emit_bfind: false,
            emit_addc: false,
            emit_subc: false,
            emit_mad24: false,
            emit_mul24: false,
            ..coverage_heavy_config()
        };

        for seed in 0..8192 {
            let bytes = bytes_from_seed(seed, 32768);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            if has_predicated_register_fns_param(&ptx) {
                return;
            }
        }

        panic!("sample did not emit predicated fns.b32 with register base/offset");
    }

    #[test]
    fn predicated_register_fns_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_predicated_reg_fns: false,
            ..coverage_heavy_config()
        };

        for seed in 0..1024 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            assert!(
                !has_predicated_register_fns_param(&ptx),
                "seed {seed:x} emitted predicated fns.b32 with register base/offset"
            );
        }
    }

    #[test]
    fn bfind_generation_is_reachable() {
        let cfg = coverage_heavy_config();
        assert_mnemonic_coverage(&cfg, 32768, 4096, BFIND_MNEMONICS);
    }

    #[test]
    fn signed_bfind_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_signed_bfind: false,
            ..GenConfig::default()
        };

        for seed in 0..1024 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            for mnemonic in SIGNED_BFIND_MNEMONICS {
                assert!(
                    !has_mnemonic(&ptx, mnemonic),
                    "seed {seed:x} emitted {mnemonic}"
                );
            }
        }
    }

    #[test]
    fn wide_bfind_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_wide_bfind: false,
            ..GenConfig::default()
        };

        for seed in 0..1024 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            for mnemonic in WIDE_BFIND_MNEMONICS {
                assert!(
                    !has_mnemonic(&ptx, mnemonic),
                    "seed {seed:x} emitted {mnemonic}"
                );
            }
            assert!(
                !has_predicated_wide_bfind(&ptx),
                "seed {seed:x} emitted predicated wide bfind"
            );
        }
    }

    #[test]
    fn signed_wide_bfind_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_signed_wide_bfind: false,
            ..GenConfig::default()
        };

        for seed in 0..1024 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            for mnemonic in SIGNED_WIDE_BFIND_MNEMONICS {
                assert!(
                    !has_mnemonic(&ptx, mnemonic),
                    "seed {seed:x} emitted {mnemonic}"
                );
            }
        }
    }

    #[test]
    fn predicated_bfind_generation_is_reachable() {
        let cfg = coverage_heavy_config();
        let mut found = vec![false; BFIND_MNEMONICS.len()];

        for seed in 0..4096 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            for op in ptx.lines().filter_map(predicated_mnemonic) {
                for (i, mnemonic) in BFIND_MNEMONICS.iter().enumerate() {
                    found[i] |= op == *mnemonic;
                }
            }
            if found.iter().all(|seen| *seen) {
                break;
            }
        }

        let missing: Vec<_> = BFIND_MNEMONICS
            .iter()
            .zip(found)
            .filter_map(|(mnemonic, seen)| (!seen).then_some(*mnemonic))
            .collect();
        assert!(
            missing.is_empty(),
            "sample did not emit predicated {missing:?}"
        );
    }

    #[test]
    fn predicated_bfind_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_predicated_bfind: false,
            ..GenConfig::default()
        };

        for seed in 0..512 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            assert!(
                !has_predicated_bfind(&ptx),
                "seed {seed:x} emitted predicated bfind"
            );
        }
    }

    #[test]
    fn predicated_wide_bfind_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_predicated_wide_bfind: false,
            ..GenConfig::default()
        };

        for seed in 0..1024 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            assert!(
                !has_predicated_wide_bfind(&ptx),
                "seed {seed:x} emitted predicated wide bfind"
            );
        }
    }

    #[test]
    fn divrem_generation_is_reachable_and_nonzero() {
        let mnemonics = ["div.u32", "rem.u32", "div.s32", "rem.s32"];
        let mut found = [false; 4];

        for seed in 0..8192 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes(&bytes).unwrap();
            for line in ptx.lines() {
                for (i, mnemonic) in mnemonics.iter().enumerate() {
                    found[i] |= line.trim_start().starts_with(mnemonic);
                }
                if line.contains("div.u32") || line.contains("rem.u32") {
                    assert!(
                        !line.trim_end().ends_with(", 0;"),
                        "seed {seed:x} emitted {line}"
                    );
                }
                if line.contains("div.s32") || line.contains("rem.s32") {
                    assert!(
                        !line.trim_end().ends_with(", 0;") && !line.trim_end().ends_with(", 1;"),
                        "seed {seed:x} emitted {line}"
                    );
                }
            }
            if found.iter().all(|seen| *seen) {
                break;
            }
        }

        let missing: Vec<_> = mnemonics
            .iter()
            .zip(found)
            .filter_map(|(mnemonic, seen)| (!seen).then_some(*mnemonic))
            .collect();
        assert!(missing.is_empty(), "sample did not emit {missing:?}");
    }

    #[test]
    fn predicated_divrem_generation_is_reachable() {
        let cfg = GenConfig {
            emit_wide_int: false,
            emit_mul_wide: false,
            emit_predicated_reg_divrem: false,
            ..coverage_heavy_config()
        };
        let mut found = vec![false; DIVREM_MNEMONICS.len()];

        for seed in 0..4096 {
            let bytes = bytes_from_seed(seed, 32768);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            for op in ptx.lines().filter_map(predicated_mnemonic) {
                for (i, mnemonic) in DIVREM_MNEMONICS.iter().enumerate() {
                    found[i] |= op == *mnemonic;
                }
            }
            if found.iter().all(|seen| *seen) {
                break;
            }
        }

        let missing: Vec<_> = DIVREM_MNEMONICS
            .iter()
            .zip(found)
            .filter_map(|(mnemonic, seen)| (!seen).then_some(*mnemonic))
            .collect();
        assert!(
            missing.is_empty(),
            "sample did not emit predicated {missing:?}"
        );
    }

    #[test]
    fn predicated_divrem_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_wide_int: false,
            emit_mul_wide: false,
            emit_predicated_divrem: false,
            emit_predicated_reg_divrem: false,
            ..GenConfig::default()
        };

        for seed in 0..1024 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            assert!(
                !has_predicated_divrem(&ptx),
                "seed {seed:x} emitted predicated div/rem"
            );
        }
    }

    #[test]
    fn reg_divrem_generation_is_reachable() {
        let cfg = GenConfig {
            emit_wide_int: false,
            emit_mul_wide: false,
            emit_wide_shifts: false,
            ..coverage_heavy_config()
        };
        let mut saw_reg_divrem = false;

        for seed in 0..2048 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            saw_reg_divrem |= has_reg_divrem(&ptx);
            if saw_reg_divrem {
                break;
            }
        }

        assert!(
            saw_reg_divrem,
            "no seed in sample emitted register-divisor div/rem"
        );
    }

    #[test]
    fn reg_divrem_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_wide_int: false,
            emit_mul_wide: false,
            emit_wide_shifts: false,
            emit_reg_divrem: false,
            ..GenConfig::default()
        };

        for seed in 0..1024 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            assert!(
                !has_reg_divrem(&ptx),
                "seed {seed:x} emitted register-divisor div/rem"
            );
        }
    }

    #[test]
    fn predicated_reg_divrem_generation_is_reachable() {
        let cfg = GenConfig {
            emit_wide_int: false,
            emit_mul_wide: false,
            emit_wide_shifts: false,
            emit_predicated_divrem: false,
            ..coverage_heavy_config()
        };
        let mut saw_predicated_reg_divrem = false;

        for seed in 0..4096 {
            let bytes = bytes_from_seed(seed, 32768);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            saw_predicated_reg_divrem |= has_predicated_reg_divrem(&ptx);
            if saw_predicated_reg_divrem {
                break;
            }
        }

        assert!(
            saw_predicated_reg_divrem,
            "no seed in sample emitted predicated register-divisor div/rem"
        );
    }

    #[test]
    fn predicated_reg_divrem_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_wide_int: false,
            emit_mul_wide: false,
            emit_wide_shifts: false,
            emit_predicated_divrem: false,
            emit_predicated_reg_divrem: false,
            ..GenConfig::default()
        };

        for seed in 0..1024 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            assert!(
                !has_predicated_reg_divrem(&ptx),
                "seed {seed:x} emitted predicated register-divisor div/rem"
            );
        }
    }

    #[test]
    fn signed_divrem_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_signed_divrem: false,
            ..GenConfig::default()
        };

        for seed in 0..1024 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            for mnemonic in ["div.s32", "rem.s32"] {
                assert!(!ptx.contains(mnemonic), "seed {seed:x} emitted {mnemonic}");
            }
        }
    }

    #[test]
    fn sad_generation_is_reachable() {
        let mnemonics = ["sad.u32", "sad.s32"];
        let mut found = [false; 2];

        for seed in 0..2048 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes(&bytes).unwrap();
            for (i, mnemonic) in mnemonics.iter().enumerate() {
                found[i] |= has_mnemonic(&ptx, mnemonic);
            }
            if found.iter().all(|seen| *seen) {
                break;
            }
        }

        let missing: Vec<_> = mnemonics
            .iter()
            .zip(found)
            .filter_map(|(mnemonic, seen)| (!seen).then_some(*mnemonic))
            .collect();
        assert!(missing.is_empty(), "sample did not emit {missing:?}");
    }

    #[test]
    fn sad_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_sad: false,
            ..GenConfig::default()
        };

        for seed in 0..1024 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            for mnemonic in SAD_MNEMONICS {
                assert!(
                    !has_mnemonic(&ptx, mnemonic),
                    "seed {seed:x} emitted {mnemonic}"
                );
            }
        }
    }

    #[test]
    fn predicated_sad_generation_is_reachable() {
        let cfg = coverage_heavy_config();
        let mut found = vec![false; SAD_MNEMONICS.len()];

        for seed in 0..4096 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            for op in ptx.lines().filter_map(predicated_mnemonic) {
                for (i, mnemonic) in SAD_MNEMONICS.iter().enumerate() {
                    found[i] |= op == *mnemonic;
                }
            }
            if found.iter().all(|seen| *seen) {
                break;
            }
        }

        let missing: Vec<_> = SAD_MNEMONICS
            .iter()
            .zip(found)
            .filter_map(|(mnemonic, seen)| (!seen).then_some(*mnemonic))
            .collect();
        assert!(
            missing.is_empty(),
            "sample did not emit predicated {missing:?}"
        );
    }

    #[test]
    fn predicated_sad_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_predicated_sad: false,
            ..GenConfig::default()
        };

        for seed in 0..512 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            assert!(
                !has_predicated_sad(&ptx),
                "seed {seed:x} emitted predicated sad"
            );
        }
    }

    #[test]
    fn slct_generation_is_reachable() {
        let mnemonics = ["slct.u32.s32", "slct.s32.s32", "slct.b32.s32"];
        let mut found = [false; 3];

        for seed in 0..4096 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes(&bytes).unwrap();
            for (i, mnemonic) in mnemonics.iter().enumerate() {
                found[i] |= has_mnemonic(&ptx, mnemonic);
            }
            if found.iter().all(|seen| *seen) {
                break;
            }
        }

        let missing: Vec<_> = mnemonics
            .iter()
            .zip(found)
            .filter_map(|(mnemonic, seen)| (!seen).then_some(*mnemonic))
            .collect();
        assert!(missing.is_empty(), "sample did not emit {missing:?}");
    }

    #[test]
    fn slct_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_slct: false,
            ..GenConfig::default()
        };

        for seed in 0..1024 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            assert!(
                !has_slct_mnemonic(&ptx),
                "seed {seed:x} emitted slct instruction"
            );
        }
    }

    #[test]
    fn f32_slct_generation_is_reachable() {
        let cfg = GenConfig {
            control_flow: ControlFlowMode::Arbitrary,
            min_blocks: 1,
            max_blocks: 1,
            min_insts_per_block: 1024,
            max_insts_per_block: 1024,
            n_working_regs: 96,
            max_immediate: u32::MAX,
            emit_structured_loops: false,
            emit_arbitrary_loops: false,
            emit_slct: true,
            emit_s32_slct: true,
            emit_f32_slct: true,
            emit_wide_slct: false,
            emit_f64_slct: false,
            emit_predicated_slct: false,
            ..GenConfig::default()
        };
        assert_mnemonic_coverage(&cfg, 32768, 2048, LEGACY_F32_SLCT_MNEMONICS);
    }

    #[test]
    fn f32_slct_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_f32_slct: false,
            ..GenConfig::default()
        };

        for seed in 0..2048 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            assert!(
                !has_f32_slct_mnemonic(&ptx),
                "seed {seed:x} emitted f32 slct"
            );
        }
    }

    #[test]
    fn wide_slct_generation_is_reachable() {
        let cfg = GenConfig {
            control_flow: ControlFlowMode::Arbitrary,
            min_blocks: 1,
            max_blocks: 1,
            min_insts_per_block: 1024,
            max_insts_per_block: 1024,
            n_working_regs: 96,
            max_immediate: u32::MAX,
            emit_structured_loops: false,
            emit_arbitrary_loops: false,
            emit_slct: true,
            emit_s32_slct: true,
            emit_f32_slct: true,
            emit_wide_slct: true,
            emit_f64_slct: false,
            emit_predicated_slct: false,
            ..GenConfig::default()
        };
        assert_mnemonic_coverage(&cfg, 32768, 2048, WIDE_SLCT_MNEMONICS);
    }

    #[test]
    fn wide_slct_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_wide_slct: false,
            ..GenConfig::default()
        };

        for seed in 0..2048 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            assert!(
                !has_wide_slct_mnemonic(&ptx),
                "seed {seed:x} emitted wide slct"
            );
        }
    }

    #[test]
    fn f64_slct_generation_is_reachable() {
        let cfg = GenConfig {
            control_flow: ControlFlowMode::Arbitrary,
            min_blocks: 1,
            max_blocks: 1,
            min_insts_per_block: 1024,
            max_insts_per_block: 1024,
            n_working_regs: 96,
            max_immediate: u32::MAX,
            emit_structured_loops: false,
            emit_arbitrary_loops: false,
            emit_slct: true,
            emit_s32_slct: true,
            emit_f32_slct: true,
            emit_wide_slct: false,
            emit_f64_slct: true,
            emit_predicated_slct: false,
            ..GenConfig::default()
        };
        assert_mnemonic_coverage(&cfg, 8192, 2048, F64_SLCT_MNEMONICS);
    }

    #[test]
    fn f64_slct_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_f64_slct: false,
            ..GenConfig::default()
        };

        for seed in 0..2048 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            assert!(
                !has_f64_slct_mnemonic(&ptx),
                "seed {seed:x} emitted f64 slct"
            );
        }
    }

    #[test]
    fn predicated_slct_generation_is_reachable() {
        let cfg = coverage_heavy_config();
        let mut found = vec![false; SLCT_MNEMONICS.len()];

        for seed in 0..16384 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            for op in ptx.lines().filter_map(predicated_mnemonic) {
                for (i, mnemonic) in SLCT_MNEMONICS.iter().enumerate() {
                    found[i] |= op == *mnemonic;
                }
            }
            if found.iter().all(|seen| *seen) {
                break;
            }
        }

        let missing: Vec<_> = SLCT_MNEMONICS
            .iter()
            .zip(found)
            .filter_map(|(mnemonic, seen)| (!seen).then_some(*mnemonic))
            .collect();
        assert!(
            missing.is_empty(),
            "sample did not emit predicated {missing:?}"
        );
    }

    #[test]
    fn predicated_slct_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_predicated_slct: false,
            ..GenConfig::default()
        };

        for seed in 0..512 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            assert!(
                !has_predicated_slct(&ptx),
                "seed {seed:x} emitted predicated slct"
            );
        }
    }

    #[test]
    fn s32_slct_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_s32_slct: false,
            ..GenConfig::default()
        };

        for seed in 0..2048 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            assert!(
                !has_s32_slct_mnemonic(&ptx),
                "seed {seed:x} emitted slct.s32.*"
            );
        }
    }

    #[test]
    fn dp4a_generation_is_reachable() {
        let mnemonics = [
            "dp4a.u32.u32",
            "dp4a.u32.s32",
            "dp4a.s32.u32",
            "dp4a.s32.s32",
        ];
        let mut found = [false; 4];

        for seed in 0..8192 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes(&bytes).unwrap();
            for (i, mnemonic) in mnemonics.iter().enumerate() {
                found[i] |= has_mnemonic(&ptx, mnemonic);
            }
            if found.iter().all(|seen| *seen) {
                break;
            }
        }

        let missing: Vec<_> = mnemonics
            .iter()
            .zip(found)
            .filter_map(|(mnemonic, seen)| (!seen).then_some(*mnemonic))
            .collect();
        assert!(missing.is_empty(), "sample did not emit {missing:?}");
    }

    #[test]
    fn dp4a_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_dp4a: false,
            ..GenConfig::default()
        };

        for seed in 0..1024 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            for mnemonic in DP4A_MNEMONICS {
                assert!(
                    !has_mnemonic(&ptx, mnemonic),
                    "seed {seed:x} emitted {mnemonic}"
                );
            }
        }
    }

    #[test]
    fn dp2a_generation_is_reachable() {
        let mnemonics = [
            "dp2a.lo.u32.u32",
            "dp2a.hi.u32.u32",
            "dp2a.lo.u32.s32",
            "dp2a.hi.u32.s32",
            "dp2a.lo.s32.u32",
            "dp2a.hi.s32.u32",
            "dp2a.lo.s32.s32",
            "dp2a.hi.s32.s32",
        ];
        let mut found = [false; 8];

        for seed in 0..16384 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes(&bytes).unwrap();
            for (i, mnemonic) in mnemonics.iter().enumerate() {
                found[i] |= has_mnemonic(&ptx, mnemonic);
            }
            if found.iter().all(|seen| *seen) {
                break;
            }
        }

        let missing: Vec<_> = mnemonics
            .iter()
            .zip(found)
            .filter_map(|(mnemonic, seen)| (!seen).then_some(*mnemonic))
            .collect();
        assert!(missing.is_empty(), "sample did not emit {missing:?}");
    }

    #[test]
    fn dp2a_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_dp2a: false,
            ..GenConfig::default()
        };

        for seed in 0..512 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            for mnemonic in [
                "dp2a.lo.u32.u32",
                "dp2a.hi.u32.u32",
                "dp2a.lo.u32.s32",
                "dp2a.hi.u32.s32",
                "dp2a.lo.s32.u32",
                "dp2a.hi.s32.u32",
                "dp2a.lo.s32.s32",
                "dp2a.hi.s32.s32",
            ] {
                assert!(!ptx.contains(mnemonic), "seed {seed:x} emitted {mnemonic}");
            }
        }
    }

    #[test]
    fn predicated_dp_generation_is_reachable() {
        let cfg = dot_video_focused_config();
        let mut saw_dp4a = false;
        let mut saw_dp2a = false;

        for seed in 0..2048 {
            let bytes = bytes_from_seed(seed, 32768);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            for op in ptx.lines().filter_map(predicated_mnemonic) {
                saw_dp4a |= DP4A_MNEMONICS.contains(&op);
                saw_dp2a |= DP2A_MNEMONICS.contains(&op);
            }
            if saw_dp4a && saw_dp2a {
                break;
            }
        }

        assert!(saw_dp4a, "sample did not emit predicated dp4a");
        assert!(saw_dp2a, "sample did not emit predicated dp2a");
    }

    #[test]
    fn predicated_dp_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_predicated_dp: false,
            ..GenConfig::default()
        };

        for seed in 0..1024 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            assert!(
                !has_predicated_dp(&ptx),
                "seed {seed:x} emitted predicated dot product"
            );
        }
    }

    #[test]
    fn video_generation_is_reachable() {
        let cfg = GenConfig {
            emit_signed_video: false,
            emit_video_sat: false,
            ..GenConfig::default()
        };
        let mnemonics = [
            "vadd2.u32.u32.u32",
            "vsub2.u32.u32.u32",
            "vavrg2.u32.u32.u32",
            "vavrg2.u32.u32.u32.add",
            "vabsdiff2.u32.u32.u32.add",
            "vmin2.u32.u32.u32",
            "vmin2.u32.u32.u32.add",
            "vmax2.u32.u32.u32",
            "vmax2.u32.u32.u32.add",
            "vadd4.u32.u32.u32",
            "vsub4.u32.u32.u32",
            "vavrg4.u32.u32.u32",
            "vavrg4.u32.u32.u32.add",
            "vabsdiff4.u32.u32.u32.add",
            "vmin4.u32.u32.u32",
            "vmin4.u32.u32.u32.add",
            "vmax4.u32.u32.u32",
            "vmax4.u32.u32.u32.add",
        ];
        let mut found = [false; 18];

        for seed in 0..32768 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            for (i, mnemonic) in mnemonics.iter().enumerate() {
                found[i] |= has_mnemonic(&ptx, mnemonic);
            }
            if found.iter().all(|seen| *seen) {
                break;
            }
        }

        let missing: Vec<_> = mnemonics
            .iter()
            .zip(found)
            .filter_map(|(mnemonic, seen)| (!seen).then_some(*mnemonic))
            .collect();
        assert!(missing.is_empty(), "sample did not emit {missing:?}");
    }

    #[test]
    fn video_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_video: false,
            ..GenConfig::default()
        };

        for seed in 0..512 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            assert!(
                !has_video_mnemonic(&ptx),
                "seed {seed:x} emitted video instruction"
            );
        }
    }

    #[test]
    fn signed_video_generation_is_reachable() {
        let cfg = GenConfig {
            emit_video: true,
            emit_signed_video: true,
            emit_video_sat: false,
            emit_vsub4: false,
            ..dot_video_focused_config()
        };
        let mut saw_signed_video = false;

        for seed in 0..2048 {
            let bytes = bytes_from_seed(seed, 32768);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            saw_signed_video |= has_signed_video_mnemonic(&ptx);
            if saw_signed_video {
                break;
            }
        }

        assert!(saw_signed_video, "no seed in sample emitted signed video");
    }

    #[test]
    fn signed_video_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_signed_video: false,
            ..GenConfig::default()
        };

        for seed in 0..1024 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            assert!(
                !has_signed_video_mnemonic(&ptx),
                "seed {seed:x} emitted signed video"
            );
        }
    }

    #[test]
    fn video_sat_generation_is_reachable() {
        let cfg = GenConfig {
            emit_video: true,
            emit_signed_video: false,
            emit_video_sat: true,
            emit_vsub4: false,
            ..dot_video_focused_config()
        };
        let mut saw_video_sat = false;

        for seed in 0..2048 {
            let bytes = bytes_from_seed(seed, 32768);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            saw_video_sat |= has_video_sat_mnemonic(&ptx);
            if saw_video_sat {
                break;
            }
        }

        assert!(saw_video_sat, "no seed in sample emitted video .sat");
    }

    #[test]
    fn video_sat_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_video_sat: false,
            ..GenConfig::default()
        };

        for seed in 0..1024 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            assert!(
                !has_video_sat_mnemonic(&ptx),
                "seed {seed:x} emitted video .sat"
            );
        }
    }

    #[test]
    fn predicated_video_generation_is_reachable() {
        let cfg = dot_video_focused_config();
        let mut saw_predicated_video = false;

        for seed in 0..2048 {
            let bytes = bytes_from_seed(seed, 32768);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            saw_predicated_video |= has_predicated_video(&ptx);
            if saw_predicated_video {
                break;
            }
        }

        assert!(
            saw_predicated_video,
            "no seed in sample emitted predicated video"
        );
    }

    #[test]
    fn predicated_video_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_predicated_video: false,
            ..GenConfig::default()
        };

        for seed in 0..1024 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            assert!(
                !has_predicated_video(&ptx),
                "seed {seed:x} emitted predicated video"
            );
        }
    }

    #[test]
    fn vsub4_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_vsub4: false,
            ..GenConfig::default()
        };

        for seed in 0..2048 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            assert!(!has_vsub4_mnemonic(&ptx), "seed {seed:x} emitted vsub4");
        }
    }

    #[test]
    fn set_generation_is_reachable() {
        let mut saw_set = false;
        for seed in 0..2048 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes(&bytes).unwrap();
            if ptx.contains("set.eq.u32")
                || ptx.contains("set.ne.u32")
                || ptx.contains("set.lt.u32")
                || ptx.contains("set.le.u32")
                || ptx.contains("set.gt.u32")
                || ptx.contains("set.ge.u32")
            {
                saw_set = true;
                break;
            }
        }
        assert!(saw_set, "no seed in sample emitted set");
    }

    #[test]
    fn predicated_set_generation_is_reachable() {
        let cfg = coverage_heavy_config();
        let mut saw_predicated_set = false;

        for seed in 0..1024 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            saw_predicated_set |= has_predicated_set(&ptx);
            if saw_predicated_set {
                break;
            }
        }

        assert!(
            saw_predicated_set,
            "no seed in sample emitted predicated set"
        );
    }

    #[test]
    fn predicated_set_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_predicated_set: false,
            ..GenConfig::default()
        };

        for seed in 0..512 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            assert!(
                !has_predicated_set(&ptx),
                "seed {seed:x} emitted predicated set"
            );
        }
    }

    #[test]
    fn negated_predicate_generation_is_reachable() {
        let cfg = coverage_heavy_config();
        let mut saw_negated_predicate = false;

        for seed in 0..1024 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            saw_negated_predicate |= has_negated_predicate(&ptx);
            if saw_negated_predicate {
                break;
            }
        }

        assert!(
            saw_negated_predicate,
            "no seed in sample emitted a negated instruction predicate"
        );
    }

    #[test]
    fn negated_predicate_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_negated_predicates: false,
            ..GenConfig::default()
        };

        for seed in 0..512 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            assert!(
                !has_negated_predicate(&ptx),
                "seed {seed:x} emitted a negated instruction predicate"
            );
        }
    }

    #[test]
    fn predicated_alu_generation_is_reachable() {
        let cfg = coverage_heavy_config();
        let mut saw_predicated_alu = false;

        for seed in 0..1024 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            saw_predicated_alu |= has_predicated_alu(&ptx);
            if saw_predicated_alu {
                break;
            }
        }

        assert!(
            saw_predicated_alu,
            "no seed in sample emitted predicated ALU"
        );
    }

    #[test]
    fn predicated_alu_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_predicated_alu: false,
            ..GenConfig::default()
        };

        for seed in 0..512 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            assert!(
                !has_predicated_alu(&ptx),
                "seed {seed:x} emitted predicated ALU"
            );
        }
    }

    #[test]
    fn setp_bool_generation_is_reachable() {
        let cfg = coverage_heavy_config();
        let mut saw = [false; 3];

        for seed in 0..4096 {
            let bytes = bytes_from_seed(seed, 32768);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            for suffix in ptx.lines().filter_map(setp_bool_suffix) {
                match suffix {
                    "and" => saw[0] = true,
                    "or" => saw[1] = true,
                    "xor" => saw[2] = true,
                    _ => {}
                }
            }
            if saw.iter().all(|seen| *seen) {
                break;
            }
        }

        assert!(
            saw.iter().all(|seen| *seen),
            "sample emitted setp bool ops {saw:?}"
        );
    }

    #[test]
    fn setp_bool_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_setp_bool: false,
            emit_wide_setp_bool: false,
            ..GenConfig::default()
        };

        for seed in 0..1024 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            assert!(
                !has_setp_bool(&ptx),
                "seed {seed:x} emitted setp bool combiner"
            );
        }
    }

    #[test]
    fn setp_dual_generation_is_reachable() {
        let cfg = coverage_heavy_config();
        let mut saw_setp_dual = false;

        for seed in 0..2048 {
            let bytes = bytes_from_seed(seed, 32768);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            saw_setp_dual |= has_setp_dual(&ptx);
            if saw_setp_dual {
                break;
            }
        }

        assert!(
            saw_setp_dual,
            "no seed in sample emitted setp dual destination"
        );
    }

    #[test]
    fn setp_dual_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_setp_dual: false,
            ..GenConfig::default()
        };

        for seed in 0..1024 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            assert!(
                !has_setp_dual(&ptx),
                "seed {seed:x} emitted setp dual destination"
            );
        }
    }

    #[test]
    fn pred_logic_generation_is_reachable() {
        let cfg = coverage_heavy_config();
        assert_mnemonic_coverage(&cfg, 32768, 4096, PRED_LOGIC_MNEMONICS);
    }

    #[test]
    fn pred_logic_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_pred_logic: false,
            ..GenConfig::default()
        };

        for seed in 0..1024 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            assert!(
                !has_pred_logic(&ptx),
                "seed {seed:x} emitted predicate logic"
            );
        }
    }

    #[test]
    fn selp_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_selp: false,
            ..GenConfig::default()
        };

        for seed in 0..512 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            assert!(!ptx.contains("selp.b32"), "seed {seed:x} emitted selp.b32");
            assert!(!ptx.contains("selp.s32"), "seed {seed:x} emitted selp.s32");
        }
    }

    #[test]
    fn typed_selp_generation_is_reachable() {
        let cfg = GenConfig {
            control_flow: ControlFlowMode::Arbitrary,
            min_blocks: 1,
            max_blocks: 1,
            min_insts_per_block: 1024,
            max_insts_per_block: 1024,
            n_working_regs: 96,
            max_immediate: u32::MAX,
            emit_structured_loops: false,
            emit_arbitrary_loops: false,
            emit_set: false,
            emit_selp: true,
            emit_typed_selp: true,
            emit_predicated_selp: false,
            emit_f32_compare: false,
            emit_f64_compare: false,
            emit_scalar_16bit_compare: false,
            emit_scalar_16bit_selp: false,
            ..GenConfig::default()
        };
        assert_mnemonic_coverage(&cfg, 32768, 2048, TYPED_SELP_MNEMONICS);
    }

    #[test]
    fn typed_selp_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_typed_selp: false,
            emit_f32_compare: false,
            emit_f64_compare: false,
            emit_scalar_16bit_compare: false,
            emit_scalar_16bit_selp: false,
            ..GenConfig::default()
        };

        for seed in 0..2048 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            assert!(
                !has_direct_typed_selp(&ptx),
                "seed {seed:x} emitted direct typed selp"
            );
        }
    }

    #[test]
    fn predicated_selp_generation_is_reachable() {
        let cfg = coverage_heavy_config();
        let mut saw_predicated_selp = false;

        for seed in 0..1024 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            saw_predicated_selp |= has_predicated_selp(&ptx);
            if saw_predicated_selp {
                break;
            }
        }

        assert!(
            saw_predicated_selp,
            "no seed in sample emitted predicated selp"
        );
    }

    #[test]
    fn predicated_selp_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_predicated_selp: false,
            ..GenConfig::default()
        };

        for seed in 0..512 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            assert!(
                !has_predicated_selp(&ptx),
                "seed {seed:x} emitted predicated selp"
            );
        }
    }

    #[test]
    fn set_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_set: false,
            ..GenConfig::default()
        };

        for seed in 0..512 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            for mnemonic in [
                "set.eq.u32",
                "set.ne.u32",
                "set.lt.u32",
                "set.le.u32",
                "set.gt.u32",
                "set.ge.u32",
            ] {
                assert!(!ptx.contains(mnemonic), "seed {seed:x} emitted {mnemonic}");
            }
        }
    }

    #[test]
    fn funnel_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_funnel: false,
            ..GenConfig::default()
        };

        for seed in 0..256 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            for mnemonic in FUNNEL_MNEMONICS {
                assert!(!ptx.contains(mnemonic), "seed {seed:x} emitted {mnemonic}");
            }
        }
    }

    #[test]
    fn funnel_clamp_generation_is_reachable() {
        let cfg = coverage_heavy_config();
        assert_mnemonic_coverage(&cfg, 32768, 4096, FUNNEL_CLAMP_MNEMONICS);
    }

    #[test]
    fn funnel_clamp_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_funnel_clamp: false,
            ..GenConfig::default()
        };

        for seed in 0..1024 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            for mnemonic in FUNNEL_CLAMP_MNEMONICS {
                assert!(
                    !has_mnemonic(&ptx, mnemonic),
                    "seed {seed:x} emitted {mnemonic}"
                );
            }
        }
    }

    #[test]
    fn predicated_funnel_generation_is_reachable() {
        let cfg = coverage_heavy_config();
        let mut saw_predicated_funnel = false;

        for seed in 0..2048 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            saw_predicated_funnel |= has_predicated_funnel(&ptx);
            if saw_predicated_funnel {
                break;
            }
        }

        assert!(
            saw_predicated_funnel,
            "no seed in sample emitted predicated funnel shift"
        );
    }

    #[test]
    fn predicated_funnel_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_predicated_funnel: false,
            ..GenConfig::default()
        };

        for seed in 0..512 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            assert!(
                !has_predicated_funnel(&ptx),
                "seed {seed:x} emitted predicated funnel shift"
            );
        }
    }

    #[test]
    fn register_funnel_generation_is_reachable() {
        let cfg = coverage_heavy_config();
        let mut saw_register_funnel = false;

        for seed in 0..2048 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            saw_register_funnel |= has_register_funnel(&ptx);
            if saw_register_funnel {
                break;
            }
        }

        assert!(
            saw_register_funnel,
            "no seed in sample emitted register-count funnel shift"
        );
    }

    #[test]
    fn register_funnel_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_reg_funnel: false,
            ..GenConfig::default()
        };

        for seed in 0..512 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            assert!(
                !has_register_funnel(&ptx),
                "seed {seed:x} emitted register-count funnel shift"
            );
        }
    }
}
