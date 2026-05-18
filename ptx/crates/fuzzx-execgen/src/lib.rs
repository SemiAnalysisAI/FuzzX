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
//!   * Each thread writes only to its own disjoint output slice; no shared
//!     memory, no atomics, no warp intrinsics, no `bar.sync`.
//!   * Integer ops only. Variable shift counts are masked or use `.wrap`
//!     semantics. Divisors are nonzero. No FP.

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
    pub emit_sub: bool,
    pub emit_mul_lo: bool,
    pub emit_signed_lo_alu: bool,
    pub emit_sat_arith: bool,
    pub emit_mulhi: bool,
    pub emit_signed_mulhi: bool,
    pub emit_mad_hi: bool,
    pub emit_signed_mad_hi: bool,
    pub emit_bitwise_binops: bool,
    pub emit_or: bool,
    pub emit_xor: bool,
    pub emit_prmt: bool,
    pub emit_predicated_prmt: bool,
    pub emit_not: bool,
    pub emit_clz: bool,
    pub emit_brev: bool,
    pub emit_cnot: bool,
    pub emit_popc: bool,
    pub emit_abs: bool,
    pub emit_signed_cmp: bool,
    pub emit_signed_divrem: bool,
    pub emit_reg_divrem: bool,
    pub emit_predicated_reg_divrem: bool,
    pub emit_funnel: bool,
    pub emit_reg_funnel: bool,
    pub emit_predicated_funnel: bool,
    pub emit_neg: bool,
    pub emit_shl: bool,
    pub emit_shr: bool,
    pub emit_signed_shr: bool,
    pub emit_reg_shifts: bool,
    pub emit_predicated_shifts: bool,
    pub emit_predicated_reg_shifts: bool,
    pub emit_bfind: bool,
    pub emit_predicated_bfind: bool,
    pub emit_bfi: bool,
    pub emit_bmsk: bool,
    pub emit_predicated_bitfield: bool,
    pub emit_mad24: bool,
    pub emit_mul24: bool,
    pub emit_predicated_24bit: bool,
    pub emit_mul_wide: bool,
    pub emit_predicated_mul_wide: bool,
    pub emit_wide_int: bool,
    pub emit_predicated_wide_int: bool,
    pub emit_wide_shifts: bool,
    pub emit_predicated_wide_shifts: bool,
    pub emit_addc: bool,
    pub emit_subc: bool,
    pub emit_predicated_carry: bool,
    pub emit_i32_boundary_immediates: bool,
    pub emit_dp2a: bool,
    pub emit_negated_predicates: bool,
    pub emit_predicated_alu: bool,
    pub emit_predicated_unary: bool,
    pub emit_predicated_cvt: bool,
    pub emit_setp_bool: bool,
    pub emit_setp_dual: bool,
    pub emit_predicated_mad: bool,
    pub emit_predicated_mad_hi: bool,
    pub emit_predicated_set: bool,
    pub emit_predicated_selp: bool,
    pub emit_predicated_divrem: bool,
    pub emit_predicated_sad: bool,
    pub emit_predicated_slct: bool,
    pub emit_predicated_dp: bool,
    pub emit_predicated_video: bool,
    pub emit_set: bool,
    pub emit_s32_slct: bool,
    pub emit_video: bool,
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
            emit_sub: true,
            emit_mul_lo: true,
            emit_signed_lo_alu: true,
            emit_sat_arith: true,
            emit_mulhi: true,
            emit_signed_mulhi: true,
            emit_mad_hi: true,
            emit_signed_mad_hi: true,
            emit_bitwise_binops: true,
            emit_or: true,
            emit_xor: true,
            emit_prmt: true,
            emit_predicated_prmt: true,
            emit_not: true,
            emit_clz: true,
            emit_brev: true,
            emit_cnot: true,
            emit_popc: true,
            emit_abs: true,
            emit_signed_cmp: true,
            emit_signed_divrem: true,
            emit_reg_divrem: true,
            emit_predicated_reg_divrem: true,
            emit_funnel: true,
            emit_reg_funnel: true,
            emit_predicated_funnel: true,
            emit_neg: true,
            emit_shl: true,
            emit_shr: true,
            emit_signed_shr: true,
            emit_reg_shifts: true,
            emit_predicated_shifts: true,
            emit_predicated_reg_shifts: true,
            emit_bfind: true,
            emit_predicated_bfind: true,
            emit_bfi: true,
            emit_bmsk: true,
            emit_predicated_bitfield: true,
            emit_mad24: true,
            emit_mul24: true,
            emit_predicated_24bit: true,
            emit_mul_wide: true,
            emit_predicated_mul_wide: true,
            emit_wide_int: true,
            emit_predicated_wide_int: true,
            emit_wide_shifts: true,
            emit_predicated_wide_shifts: true,
            emit_addc: true,
            emit_subc: true,
            emit_predicated_carry: true,
            emit_i32_boundary_immediates: true,
            emit_dp2a: true,
            emit_negated_predicates: true,
            emit_predicated_alu: true,
            emit_predicated_unary: true,
            emit_predicated_cvt: true,
            emit_setp_bool: true,
            emit_setp_dual: true,
            emit_predicated_mad: true,
            emit_predicated_mad_hi: true,
            emit_predicated_set: true,
            emit_predicated_selp: true,
            emit_predicated_divrem: true,
            emit_predicated_sad: true,
            emit_predicated_slct: true,
            emit_predicated_dp: true,
            emit_predicated_video: true,
            emit_set: true,
            emit_s32_slct: true,
            emit_video: true,
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
enum BfindOp {
    Position,
    ShiftAmount,
}

impl BfindOp {
    fn mnemonic(self) -> &'static str {
        match self {
            BfindOp::Position => "bfind.u32",
            BfindOp::ShiftAmount => "bfind.shiftamt.u32",
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
enum WideIntOp {
    AddU64,
    SubU64,
    MulLoU64,
    AddS64,
    SubS64,
    MulLoS64,
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
            WideIntOp::AddS64 => "add.s64",
            WideIntOp::SubS64 => "sub.s64",
            WideIntOp::MulLoS64 => "mul.lo.s64",
            WideIntOp::AndB64 => "and.b64",
            WideIntOp::OrB64 => "or.b64",
            WideIntOp::XorB64 => "xor.b64",
        }
    }

    fn cvt_mnemonic(self) -> &'static str {
        match self {
            WideIntOp::AddS64 | WideIntOp::SubS64 | WideIntOp::MulLoS64 => "cvt.s64.s32",
            WideIntOp::AddU64
            | WideIntOp::SubU64
            | WideIntOp::MulLoU64
            | WideIntOp::AndB64
            | WideIntOp::OrB64
            | WideIntOp::XorB64 => "cvt.u64.u32",
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
enum AddCarryOp {
    Add,
    Sub,
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
}

impl SlctOp {
    fn mnemonic(self) -> &'static str {
        match self {
            SlctOp::U32S32 => "slct.u32.s32",
            SlctOp::S32S32 => "slct.s32.s32",
            SlctOp::B32S32 => "slct.b32.s32",
        }
    }
}

#[derive(Clone, Copy)]
enum VideoOp {
    Add2,
    Sub2,
    Avrg2,
    Avrg2Add,
    AbsDiff2Add,
    Min2,
    Min2Add,
    Max2,
    Max2Add,
    Add4,
    Sub4,
    Avrg4,
    Avrg4Add,
    AbsDiff4Add,
    Min4,
    Min4Add,
    Max4,
    Max4Add,
}

impl VideoOp {
    fn mnemonic(self) -> &'static str {
        match self {
            VideoOp::Add2 => "vadd2.u32.u32.u32",
            VideoOp::Sub2 => "vsub2.u32.u32.u32",
            VideoOp::Avrg2 => "vavrg2.u32.u32.u32",
            VideoOp::Avrg2Add => "vavrg2.u32.u32.u32.add",
            VideoOp::AbsDiff2Add => "vabsdiff2.u32.u32.u32.add",
            VideoOp::Min2 => "vmin2.u32.u32.u32",
            VideoOp::Min2Add => "vmin2.u32.u32.u32.add",
            VideoOp::Max2 => "vmax2.u32.u32.u32",
            VideoOp::Max2Add => "vmax2.u32.u32.u32.add",
            VideoOp::Add4 => "vadd4.u32.u32.u32",
            VideoOp::Sub4 => "vsub4.u32.u32.u32",
            VideoOp::Avrg4 => "vavrg4.u32.u32.u32",
            VideoOp::Avrg4Add => "vavrg4.u32.u32.u32.add",
            VideoOp::AbsDiff4Add => "vabsdiff4.u32.u32.u32.add",
            VideoOp::Min4 => "vmin4.u32.u32.u32",
            VideoOp::Min4Add => "vmin4.u32.u32.u32.add",
            VideoOp::Max4 => "vmax4.u32.u32.u32",
            VideoOp::Max4Add => "vmax4.u32.u32.u32.add",
        }
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

#[derive(Clone, Copy)]
enum FunnelDir {
    Left,
    Right,
}

impl FunnelDir {
    fn mnemonic(self) -> &'static str {
        match self {
            // .wrap mode masks the shift amount to 5 bits — safe for any input.
            FunnelDir::Left => "shf.l.wrap.b32",
            FunnelDir::Right => "shf.r.wrap.b32",
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
    Sel {
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
    /// `mul.wide.{u32,s32}` through a scratch b64 register, low 32 bits kept.
    MulWide {
        op: MulWideOp,
        dst: u32,
        a: Operand,
        b: Operand,
    },
    /// `setp.<cmp> pred, ca, cb; @pred mul.wide.* ...; @pred mov.b64 ...;`.
    PredicatedMulWide {
        op: MulWideOp,
        dst: u32,
        a: Operand,
        b: Operand,
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
    /// 64-bit shift through scratch b64 registers, low 32 bits kept.
    WideShift {
        op: WideShiftOp,
        dst: u32,
        src: Operand,
        amount: u32,
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
        dst: u32,
        a: Operand,
        b: Operand,
        ctrl: u32,
    },
    /// `setp.<cmp> pred, ca, cb; @pred prmt.b32 dst, a, b, ctrl;`.
    PredicatedPrmt {
        dst: u32,
        a: Operand,
        b: Operand,
        ctrl: u32,
        cmp: CmpOp,
        ca: Operand,
        cb: Operand,
        pred: u32,
    },
    /// `shf.{l,r}.wrap.b32 dst, a, b, amount;` — funnel shift. `.wrap`
    /// masks `amount` to 5 bits → safe for any input.
    Funnel {
        dir: FunnelDir,
        dst: u32,
        a: Operand,
        b: Operand,
        amount: u32,
    },
    /// `shf.{l,r}.wrap.b32 dst, a, b, amount_reg;`.
    RegFunnel {
        dir: FunnelDir,
        dst: u32,
        a: Operand,
        b: Operand,
        amount: Operand,
    },
    /// `setp.<cmp> pred, ca, cb; @pred shf.{l,r}.wrap.b32 dst, a, b, amount;`.
    PredicatedFunnel {
        dir: FunnelDir,
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
    /// position `pos`. PTX masks pos/len mod 32 → safe.
    Bfe {
        op: BfeOp,
        dst: u32,
        src: Operand,
        pos: u32,
        len: u32,
    },
    /// `setp.<cmp> pred, ca, cb; @pred bfe.{u32,s32} dst, src, pos, len;`.
    PredicatedBfe {
        op: BfeOp,
        dst: u32,
        src: Operand,
        pos: u32,
        len: u32,
        cmp: CmpOp,
        ca: Operand,
        cb: Operand,
        pred: u32,
    },
    /// `bfi.b32 dst, src, base, pos, len;` — insert low `len` bits of `src`
    /// into `base` starting at `pos`. PTX masks pos/len mod 32 → safe.
    Bfi {
        dst: u32,
        src: Operand,
        base: Operand,
        pos: u32,
        len: u32,
    },
    /// `setp.<cmp> pred, ca, cb; @pred bfi.b32 dst, src, base, pos, len;`.
    PredicatedBfi {
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
    /// `bmsk.clamp.b32 dst, pos, len;` — create a clamped bit mask.
    Bmsk { dst: u32, pos: u32, len: u32 },
    /// `setp.<cmp> pred, ca, cb; @pred bmsk.clamp.b32 dst, pos, len;`.
    PredicatedBmsk {
        dst: u32,
        pos: u32,
        len: u32,
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
            if self.cfg.emit_setp_dual && self.cfg.emit_predicated_alu && u.arbitrary::<bool>()? {
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
            if !self.can_emit_unary() {
                return self.pick_mad_or_add(u);
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
                if self.cfg.emit_predicated_prmt && u.arbitrary::<bool>()? {
                    Ok(Inst::PredicatedPrmt {
                        dst: self.pick_dst(u)?,
                        a: self.pick_operand(u)?,
                        b: self.pick_operand(u)?,
                        ctrl: u.int_in_range(0..=0xFFFF)?,
                        cmp: pick_cmp(u, self.cfg.emit_signed_cmp)?,
                        ca: self.pick_guard_operand(u)?,
                        cb: self.pick_guard_operand(u)?,
                        pred: self.alloc_inst_pred(u)?,
                    })
                } else {
                    Ok(Inst::Prmt {
                        dst: self.pick_dst(u)?,
                        a: self.pick_operand(u)?,
                        b: self.pick_operand(u)?,
                        ctrl: u.int_in_range(0..=0xFFFF)?,
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
                if self.cfg.emit_predicated_funnel && u.arbitrary::<bool>()? {
                    Ok(Inst::PredicatedFunnel {
                        dir,
                        dst: self.pick_dst(u)?,
                        a: self.pick_operand(u)?,
                        b: self.pick_operand(u)?,
                        amount: if self.cfg.emit_reg_funnel && u.arbitrary::<bool>()? {
                            self.pick_reg_operand(u)?
                        } else {
                            Operand::Imm(u.int_in_range(0..=31)?)
                        },
                        cmp: pick_cmp(u, self.cfg.emit_signed_cmp)?,
                        ca: self.pick_guard_operand(u)?,
                        cb: self.pick_guard_operand(u)?,
                        pred: self.alloc_inst_pred(u)?,
                    })
                } else if self.cfg.emit_reg_funnel && u.arbitrary::<bool>()? {
                    Ok(Inst::RegFunnel {
                        dir,
                        dst: self.pick_dst(u)?,
                        a: self.pick_operand(u)?,
                        b: self.pick_operand(u)?,
                        amount: self.pick_reg_operand(u)?,
                    })
                } else {
                    Ok(Inst::Funnel {
                        dir,
                        dst: self.pick_dst(u)?,
                        a: self.pick_operand(u)?,
                        b: self.pick_operand(u)?,
                        amount: u.int_in_range(0..=31)?,
                    })
                }
            } else {
                self.pick_mad_or_add(u)
            }
        } else if pick < 235 {
            if self.cfg.emit_predicated_bitfield && u.arbitrary::<bool>()? {
                if self.cfg.emit_bmsk && u.arbitrary::<bool>()? {
                    Ok(Inst::PredicatedBmsk {
                        dst: self.pick_dst(u)?,
                        pos: u.int_in_range(0..=31)?,
                        len: u.int_in_range(0..=31)?,
                        cmp: pick_cmp(u, self.cfg.emit_signed_cmp)?,
                        ca: self.pick_guard_operand(u)?,
                        cb: self.pick_guard_operand(u)?,
                        pred: self.alloc_inst_pred(u)?,
                    })
                } else {
                    Ok(Inst::PredicatedBfe {
                        op: pick_bfe(u)?,
                        dst: self.pick_dst(u)?,
                        src: self.pick_operand(u)?,
                        pos: u.int_in_range(0..=31)?,
                        len: u.int_in_range(0..=31)?,
                        cmp: pick_cmp(u, self.cfg.emit_signed_cmp)?,
                        ca: self.pick_guard_operand(u)?,
                        cb: self.pick_guard_operand(u)?,
                        pred: self.alloc_inst_pred(u)?,
                    })
                }
            } else if self.cfg.emit_bmsk && u.arbitrary::<bool>()? {
                Ok(Inst::Bmsk {
                    dst: self.pick_dst(u)?,
                    pos: u.int_in_range(0..=31)?,
                    len: u.int_in_range(0..=31)?,
                })
            } else {
                Ok(Inst::Bfe {
                    op: pick_bfe(u)?,
                    dst: self.pick_dst(u)?,
                    src: self.pick_operand(u)?,
                    pos: u.int_in_range(0..=31)?,
                    len: u.int_in_range(0..=31)?,
                })
            }
        } else if pick < 245 {
            if self.cfg.emit_bfi {
                if self.cfg.emit_predicated_bitfield && u.arbitrary::<bool>()? {
                    Ok(Inst::PredicatedBfi {
                        dst: self.pick_dst(u)?,
                        src: self.pick_operand(u)?,
                        base: self.pick_operand(u)?,
                        pos: u.int_in_range(0..=31)?,
                        len: u.int_in_range(0..=31)?,
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
                        pos: u.int_in_range(0..=31)?,
                        len: u.int_in_range(0..=31)?,
                    })
                }
            } else {
                self.pick_mad_or_add(u)
            }
        } else if pick < 248 {
            if self.cfg.emit_predicated_cvt && u.arbitrary::<bool>()? {
                Ok(Inst::PredicatedCvt {
                    op: pick_cvt(u)?,
                    dst: self.pick_dst(u)?,
                    src: self.pick_cvt_operand(u)?,
                    cmp: pick_cmp(u, self.cfg.emit_signed_cmp)?,
                    ca: self.pick_guard_operand(u)?,
                    cb: self.pick_guard_operand(u)?,
                    pred: self.alloc_inst_pred(u)?,
                })
            } else {
                Ok(Inst::Cvt {
                    op: pick_cvt(u)?,
                    dst: self.pick_dst(u)?,
                    src: self.pick_cvt_operand(u)?,
                })
            }
        } else if pick < 251 {
            if (self.cfg.emit_addc || self.cfg.emit_subc) && u.arbitrary::<bool>()? {
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
            } else if self.cfg.emit_bfind {
                if self.cfg.emit_predicated_bfind && u.arbitrary::<bool>()? {
                    Ok(Inst::PredicatedBfind {
                        op: pick_bfind(u)?,
                        dst: self.pick_dst(u)?,
                        src: self.pick_operand(u)?,
                        cmp: pick_cmp(u, self.cfg.emit_signed_cmp)?,
                        ca: self.pick_guard_operand(u)?,
                        cb: self.pick_guard_operand(u)?,
                        pred: self.alloc_inst_pred(u)?,
                    })
                } else {
                    Ok(Inst::Bfind {
                        op: pick_bfind(u)?,
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
            let wide_pick: u8 = u.int_in_range(0..=3)?;
            if self.cfg.emit_wide_int && wide_pick == 0 {
                let op = pick_wide_int(u)?;
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
            } else if self.cfg.emit_mul_wide && wide_pick <= 1 {
                let op = pick_mul_wide(u)?;
                if self.cfg.emit_predicated_mul_wide && u.arbitrary::<bool>()? {
                    Ok(Inst::PredicatedMulWide {
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
                    Ok(Inst::MulWide {
                        op,
                        dst: self.pick_dst(u)?,
                        a: self.pick_operand(u)?,
                        b: self.pick_operand(u)?,
                    })
                }
            } else if self.cfg.emit_wide_shifts && wide_pick == 2 {
                let op = pick_wide_shift(u)?;
                if self.cfg.emit_predicated_wide_shifts && u.arbitrary::<bool>()? {
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
                let op = pick_video(u, self.cfg.emit_vsub4)?;
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
            } else {
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
            }
        } else if pick < 255 {
            let op = pick_slct(u, self.cfg.emit_s32_slct)?;
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
        } else if self.cfg.emit_dp2a && u.arbitrary::<bool>()? {
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
            Inst::Set { dst, .. } | Inst::PredicatedSet { dst, .. } => {
                self.remember_set_write(*dst);
            }
            Inst::Prmt { dst, .. } | Inst::PredicatedPrmt { dst, .. } => {
                self.remember_prmt_write(*dst);
            }
            Inst::AddCarry { dst_lo, dst_hi, .. }
            | Inst::PredicatedAddCarry { dst_lo, dst_hi, .. } => {
                self.forget_tracked_write(*dst_lo);
                self.forget_tracked_write(*dst_hi);
            }
            Inst::Bin { dst, .. }
            | Inst::Sel { dst, .. }
            | Inst::PredicatedSel { dst, .. }
            | Inst::PredicatedBin { dst, .. }
            | Inst::SetpBoolBin { dst, .. }
            | Inst::SetpDualBin { dst, .. }
            | Inst::PredicatedShift { dst, .. }
            | Inst::Shift { dst, .. }
            | Inst::RegShift { dst, .. }
            | Inst::PredicatedRegShift { dst, .. }
            | Inst::Unary { dst, .. }
            | Inst::PredicatedUnary { dst, .. }
            | Inst::Cvt { dst, .. }
            | Inst::PredicatedCvt { dst, .. }
            | Inst::Bfind { dst, .. }
            | Inst::PredicatedBfind { dst, .. }
            | Inst::DivRem { dst, .. }
            | Inst::RegDivRem { dst, .. }
            | Inst::PredicatedDivRem { dst, .. }
            | Inst::PredicatedRegDivRem { dst, .. }
            | Inst::Mad24 { dst, .. }
            | Inst::PredicatedMad24 { dst, .. }
            | Inst::Mul24 { dst, .. }
            | Inst::PredicatedMul24 { dst, .. }
            | Inst::MulWide { dst, .. }
            | Inst::PredicatedMulWide { dst, .. }
            | Inst::WideInt { dst, .. }
            | Inst::PredicatedWideInt { dst, .. }
            | Inst::WideShift { dst, .. }
            | Inst::PredicatedWideShift { dst, .. }
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
            | Inst::Bfi { dst, .. }
            | Inst::PredicatedBfi { dst, .. }
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
        writeln!(s, "    .reg .b32   %r<{total_regs}>;").unwrap();
        writeln!(s, "    .reg .b64   %rd<8>;").unwrap();
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

    fn emit_inst(&self, s: &mut String, inst: &Inst) {
        match *inst {
            Inst::Bin { op, dst, a, b } => {
                write!(s, "    {:<13} %r{dst}, ", op.mnemonic()).unwrap();
                a.emit(s);
                write!(s, ", ").unwrap();
                b.emit(s);
                writeln!(s, ";").unwrap();
            }
            Inst::Sel {
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
                write!(s, "    selp.b32      %r{dst}, ").unwrap();
                a.emit(s);
                write!(s, ", ").unwrap();
                b.emit(s);
                writeln!(s, ", %p{pred};").unwrap();
            }
            Inst::PredicatedSel {
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
                write!(s, "    {} selp.b32 %r{dst}, ", pred_guard(guard_pred)).unwrap();
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
            Inst::Bfind { op, dst, src } => {
                write!(s, "    {:<13} %r{dst}, ", op.mnemonic()).unwrap();
                src.emit(s);
                writeln!(s, ";").unwrap();
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
                write!(s, "    {} {:<8} %r{dst}, ", pred_guard(pred), op.mnemonic()).unwrap();
                src.emit(s);
                writeln!(s, ";").unwrap();
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
            Inst::MulWide { op, dst, a, b } => {
                let scratch_hi = self.wide_scratch_hi_reg();
                write!(s, "    {:<13} %rd6, ", op.mnemonic()).unwrap();
                a.emit(s);
                write!(s, ", ").unwrap();
                b.emit(s);
                writeln!(s, ";").unwrap();
                writeln!(s, "    mov.b64       {{%r{dst}, %r{scratch_hi}}}, %rd6;").unwrap();
            }
            Inst::PredicatedMulWide {
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
                write!(s, "    {} {:<8} %rd6, ", pred_guard(pred), op.mnemonic()).unwrap();
                a.emit(s);
                write!(s, ", ").unwrap();
                b.emit(s);
                writeln!(s, ";").unwrap();
                writeln!(
                    s,
                    "    {} mov.b64 {{%r{dst}, %r{scratch_hi}}}, %rd6;",
                    pred_guard(pred)
                )
                .unwrap();
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
            Inst::AddCarry {
                op,
                dst_lo,
                dst_hi,
                a,
                b,
                c,
                d,
            } => {
                let (first, second) = match op {
                    AddCarryOp::Add => ("add.cc.u32", "addc.u32"),
                    AddCarryOp::Sub => ("sub.cc.u32", "subc.u32"),
                };
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
                let (first, second) = match op {
                    AddCarryOp::Add => ("add.cc.u32", "addc.u32"),
                    AddCarryOp::Sub => ("sub.cc.u32", "subc.u32"),
                };
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
                write!(s, "    {:<13} %r{dst}, ", op.mnemonic()).unwrap();
                a.emit(s);
                write!(s, ", ").unwrap();
                b.emit(s);
                write!(s, ", ").unwrap();
                c.emit(s);
                writeln!(s, ";").unwrap();
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
                write!(s, "    {} {:<8} %r{dst}, ", pred_guard(pred), op.mnemonic()).unwrap();
                a.emit(s);
                write!(s, ", ").unwrap();
                b.emit(s);
                write!(s, ", ").unwrap();
                c.emit(s);
                writeln!(s, ";").unwrap();
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
            Inst::Prmt { dst, a, b, ctrl } => {
                write!(s, "    prmt.b32      %r{dst}, ").unwrap();
                a.emit(s);
                write!(s, ", ").unwrap();
                b.emit(s);
                writeln!(s, ", 0x{ctrl:x};").unwrap();
            }
            Inst::PredicatedPrmt {
                dst,
                a,
                b,
                ctrl,
                cmp,
                ca,
                cb,
                pred,
            } => {
                self.emit_inst_predicate_setup(s, cmp, ca, cb, pred);
                write!(s, "    {} prmt.b32 %r{dst}, ", pred_guard(pred)).unwrap();
                a.emit(s);
                write!(s, ", ").unwrap();
                b.emit(s);
                writeln!(s, ", 0x{ctrl:x};").unwrap();
            }
            Inst::Funnel {
                dir,
                dst,
                a,
                b,
                amount,
            } => {
                write!(s, "    {:<13} %r{dst}, ", dir.mnemonic()).unwrap();
                a.emit(s);
                write!(s, ", ").unwrap();
                b.emit(s);
                writeln!(s, ", {amount};").unwrap();
            }
            Inst::RegFunnel {
                dir,
                dst,
                a,
                b,
                amount,
            } => {
                write!(s, "    {:<13} %r{dst}, ", dir.mnemonic()).unwrap();
                a.emit(s);
                write!(s, ", ").unwrap();
                b.emit(s);
                write!(s, ", ").unwrap();
                amount.emit(s);
                writeln!(s, ";").unwrap();
            }
            Inst::PredicatedFunnel {
                dir,
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
                    "    {} {:<8} %r{dst}, ",
                    pred_guard(pred),
                    dir.mnemonic()
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
                writeln!(s, ", {pos}, {len};").unwrap();
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
                writeln!(s, ", {pos}, {len};").unwrap();
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
                writeln!(s, ", {pos}, {len};").unwrap();
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
                writeln!(s, ", {pos}, {len};").unwrap();
            }
            Inst::Bmsk { dst, pos, len } => {
                writeln!(s, "    bmsk.clamp.b32 %r{dst}, {pos}, {len};").unwrap();
            }
            Inst::PredicatedBmsk {
                dst,
                pos,
                len,
                cmp,
                ca,
                cb,
                pred,
            } => {
                self.emit_inst_predicate_setup(s, cmp, ca, cb, pred);
                writeln!(
                    s,
                    "    {} bmsk.clamp.b32 %r{dst}, {pos}, {len};",
                    pred_guard(pred)
                )
                .unwrap();
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

fn pick_predicate_bool_op(u: &mut Unstructured) -> Result<PredicateBoolOp> {
    let ops = [
        PredicateBoolOp::And,
        PredicateBoolOp::Or,
        PredicateBoolOp::Xor,
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

fn pick_bfind(u: &mut Unstructured) -> Result<BfindOp> {
    let ops = [BfindOp::Position, BfindOp::ShiftAmount];
    Ok(*u.choose(&ops)?)
}

fn pick_bfe(u: &mut Unstructured) -> Result<BfeOp> {
    let ops = [BfeOp::U32, BfeOp::S32];
    Ok(*u.choose(&ops)?)
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

fn pick_wide_int(u: &mut Unstructured) -> Result<WideIntOp> {
    let ops = [
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
    Ok(*u.choose(&ops)?)
}

fn pick_wide_shift(u: &mut Unstructured) -> Result<WideShiftOp> {
    let ops = [
        WideShiftOp::ShlB64,
        WideShiftOp::ShrU64,
        WideShiftOp::ShrS64,
    ];
    Ok(*u.choose(&ops)?)
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

fn pick_slct(u: &mut Unstructured, emit_s32_slct: bool) -> Result<SlctOp> {
    let ops: &[SlctOp] = if emit_s32_slct {
        &[SlctOp::U32S32, SlctOp::S32S32, SlctOp::B32S32]
    } else {
        &[SlctOp::U32S32, SlctOp::B32S32]
    };
    Ok(*u.choose(&ops)?)
}

fn pick_video(u: &mut Unstructured, emit_vsub4: bool) -> Result<VideoOp> {
    const OPS_WITH_VSUB4: &[VideoOp] = &[
        VideoOp::Add2,
        VideoOp::Sub2,
        VideoOp::Avrg2,
        VideoOp::Avrg2Add,
        VideoOp::AbsDiff2Add,
        VideoOp::Min2,
        VideoOp::Min2Add,
        VideoOp::Max2,
        VideoOp::Max2Add,
        VideoOp::Add4,
        VideoOp::Sub4,
        VideoOp::Avrg4,
        VideoOp::Avrg4Add,
        VideoOp::AbsDiff4Add,
        VideoOp::Min4,
        VideoOp::Min4Add,
        VideoOp::Max4,
        VideoOp::Max4Add,
    ];
    const OPS_WITHOUT_VSUB4: &[VideoOp] = &[
        VideoOp::Add2,
        VideoOp::Sub2,
        VideoOp::Avrg2,
        VideoOp::Avrg2Add,
        VideoOp::AbsDiff2Add,
        VideoOp::Min2,
        VideoOp::Min2Add,
        VideoOp::Max2,
        VideoOp::Max2Add,
        VideoOp::Add4,
        VideoOp::Avrg4,
        VideoOp::Avrg4Add,
        VideoOp::AbsDiff4Add,
        VideoOp::Min4,
        VideoOp::Min4Add,
        VideoOp::Max4,
        VideoOp::Max4Add,
    ];
    let ops = if emit_vsub4 {
        OPS_WITH_VSUB4
    } else {
        OPS_WITHOUT_VSUB4
    };
    Ok(*u.choose(ops)?)
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
    const MAD_LO_MNEMONICS: &[&str] = &["mad.lo.u32", "mad.lo.s32"];
    const MAD_HI_MNEMONICS: &[&str] = &["mad.hi.u32", "mad.hi.s32"];
    const POST_KNOWN_BIN_MNEMONICS: &[&str] = &["add.u32", "sub.u32", "and.b32"];
    const SHIFT_MNEMONICS: &[&str] = &["shl.b32", "shr.u32", "shr.s32"];
    const UNARY_MNEMONICS: &[&str] = &[
        "not.b32", "cnot.b32", "popc.b32", "clz.b32", "brev.b32", "abs.s32", "neg.s32",
    ];
    const POST_KNOWN_UNARY_MNEMONICS: &[&str] = &["popc.b32", "clz.b32"];
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
    const BFIND_MNEMONICS: &[&str] = &["bfind.u32", "bfind.shiftamt.u32"];
    const BFE_MNEMONICS: &[&str] = &["bfe.u32", "bfe.s32"];
    const BITFIELD_MNEMONICS: &[&str] = &["bfe.u32", "bfe.s32", "bfi.b32", "bmsk.clamp.b32"];
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
    const WIDE_INT_MNEMONICS: &[&str] = &[
        "add.u64",
        "sub.u64",
        "mul.lo.u64",
        "add.s64",
        "sub.s64",
        "mul.lo.s64",
        "and.b64",
        "or.b64",
        "xor.b64",
    ];
    const WIDE_SHIFT_MNEMONICS: &[&str] = &["shl.b64", "shr.u64", "shr.s64"];
    const CARRY_MNEMONICS: &[&str] = &["add.cc.u32", "addc.u32", "sub.cc.u32", "subc.u32"];
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
    const FUNNEL_MNEMONICS: &[&str] = &["shf.l.wrap.b32", "shf.r.wrap.b32"];
    const SAD_MNEMONICS: &[&str] = &["sad.u32", "sad.s32"];
    const SLCT_MNEMONICS: &[&str] = &["slct.u32.s32", "slct.s32.s32", "slct.b32.s32"];
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
            SHIFT_MNEMONICS,
            UNARY_MNEMONICS,
            CVT_MNEMONICS,
            BFIND_MNEMONICS,
            BFE_MNEMONICS,
            DIVREM_MNEMONICS,
            MUL_WIDE_MNEMONICS,
            WIDE_INT_MNEMONICS,
            WIDE_SHIFT_MNEMONICS,
            CARRY_MNEMONICS,
            UNSIGNED_SETP_MNEMONICS,
            SIGNED_SETP_MNEMONICS,
            SET_MNEMONICS,
            FUNNEL_MNEMONICS,
            SAD_MNEMONICS,
            SLCT_MNEMONICS,
            DP4A_MNEMONICS,
            DP2A_MNEMONICS,
            VIDEO_MNEMONICS,
        ] {
            mnemonics.extend_from_slice(group);
        }
        mnemonics.extend_from_slice(&[
            "selp.b32",
            "lop3.b32",
            "prmt.b32",
            "bmsk.clamp.b32",
            "bfi.b32",
        ]);
        mnemonics
    }

    fn post_known_bug_suppression_mnemonics() -> Vec<&'static str> {
        let mut mnemonics = Vec::new();
        for group in [
            POST_KNOWN_BIN_MNEMONICS,
            POST_KNOWN_UNARY_MNEMONICS,
            CVT_MNEMONICS,
            BFE_MNEMONICS,
            DIVREM_MNEMONICS,
            MAD_HI_MNEMONICS,
            MAD24_MNEMONICS,
            MUL24_MNEMONICS,
            MUL_WIDE_MNEMONICS,
            WIDE_INT_MNEMONICS,
            WIDE_SHIFT_MNEMONICS,
            UNSIGNED_SETP_MNEMONICS,
            SAD_MNEMONICS,
            POST_KNOWN_SLCT_MNEMONICS,
            DP4A_MNEMONICS,
            DP2A_MNEMONICS,
            POST_KNOWN_VIDEO_MNEMONICS,
        ] {
            mnemonics.extend_from_slice(group);
        }
        mnemonics.push("bmsk.clamp.b32");
        mnemonics
    }

    fn has_mnemonic(ptx: &str, mnemonic: &str) -> bool {
        ptx.lines()
            .filter_map(|line| line.trim_start().split_whitespace().next())
            .any(|token| token == mnemonic)
    }

    fn predicated_mnemonic(line: &str) -> Option<&str> {
        let mut tokens = line.trim_start().split_whitespace();
        let pred = tokens.next()?;
        let op = tokens.next()?;
        (pred.starts_with("@%p") || pred.starts_with("@!%p")).then_some(op)
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
        ptx.lines()
            .filter_map(predicated_mnemonic)
            .any(|op| BIN_MNEMONICS.contains(&op))
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

    fn has_predicated_shift(ptx: &str) -> bool {
        ptx.lines()
            .filter_map(predicated_mnemonic)
            .any(|op| SHIFT_MNEMONICS.contains(&op))
    }

    fn has_predicated_unary(ptx: &str) -> bool {
        ptx.lines()
            .filter_map(predicated_mnemonic)
            .any(|op| UNARY_MNEMONICS.contains(&op))
    }

    fn has_predicated_cvt(ptx: &str) -> bool {
        ptx.lines()
            .filter_map(predicated_mnemonic)
            .any(|op| CVT_MNEMONICS.contains(&op))
    }

    fn has_predicated_bfind(ptx: &str) -> bool {
        ptx.lines()
            .filter_map(predicated_mnemonic)
            .any(|op| BFIND_MNEMONICS.contains(&op))
    }

    fn has_predicated_mad(ptx: &str) -> bool {
        ptx.lines()
            .filter_map(predicated_mnemonic)
            .any(|op| MAD_LO_MNEMONICS.contains(&op))
    }

    fn has_predicated_set(ptx: &str) -> bool {
        ptx.lines()
            .filter_map(predicated_mnemonic)
            .any(|op| SET_MNEMONICS.contains(&op))
    }

    fn has_predicated_selp(ptx: &str) -> bool {
        ptx.lines()
            .filter_map(predicated_mnemonic)
            .any(|op| op == "selp.b32")
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

    fn has_predicated_wide_int(ptx: &str) -> bool {
        ptx.lines()
            .filter_map(predicated_mnemonic)
            .any(|op| WIDE_INT_MNEMONICS.contains(&op))
    }

    fn has_predicated_wide_shift(ptx: &str) -> bool {
        ptx.lines()
            .filter_map(predicated_mnemonic)
            .any(|op| WIDE_SHIFT_MNEMONICS.contains(&op))
    }

    fn has_predicated_carry(ptx: &str) -> bool {
        ptx.lines()
            .filter_map(predicated_mnemonic)
            .any(|op| CARRY_MNEMONICS.contains(&op))
    }

    fn has_predicated_sad(ptx: &str) -> bool {
        ptx.lines()
            .filter_map(predicated_mnemonic)
            .any(|op| SAD_MNEMONICS.contains(&op))
    }

    fn has_predicated_slct(ptx: &str) -> bool {
        ptx.lines()
            .filter_map(predicated_mnemonic)
            .any(|op| SLCT_MNEMONICS.contains(&op))
    }

    fn has_predicated_dp(ptx: &str) -> bool {
        ptx.lines()
            .filter_map(predicated_mnemonic)
            .any(|op| DP4A_MNEMONICS.contains(&op) || DP2A_MNEMONICS.contains(&op))
    }

    fn has_predicated_video(ptx: &str) -> bool {
        ptx.lines()
            .filter_map(predicated_mnemonic)
            .any(|op| VIDEO_MNEMONICS.contains(&op))
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
        (op == "prmt.b32")
            .then(|| tokens.next())
            .flatten()
            .map(|token| token.trim_end_matches(','))
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
            let seen: HashSet<_> = ptx
                .lines()
                .filter_map(|line| line.trim_start().split_whitespace().next())
                .collect();
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

    fn coverage_heavy_config() -> GenConfig {
        GenConfig {
            min_blocks: 16,
            max_blocks: 24,
            min_insts_per_block: 16,
            max_insts_per_block: 24,
            n_working_regs: 24,
            max_immediate: 65536,
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
            emit_mulhi: false,
            emit_or: false,
            emit_xor: false,
            emit_prmt: false,
            emit_not: false,
            emit_brev: false,
            emit_cnot: false,
            emit_abs: false,
            emit_signed_cmp: false,
            emit_funnel: false,
            emit_neg: false,
            emit_shl: false,
            emit_shr: false,
            emit_signed_shr: false,
            emit_bfind: false,
            emit_bfi: false,
            emit_addc: false,
            emit_subc: false,
            emit_i32_boundary_immediates: false,
            emit_set: false,
            emit_s32_slct: false,
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
            emit_video: true,
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

        assert_mnemonic_coverage(&cfg, 32768, 2048, &mnemonics);
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

        assert_mnemonic_coverage(&cfg, 32768, 2048, &mnemonics);
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
            for mnemonic in ["bfind.u32", "bfind.shiftamt.u32"] {
                assert!(!ptx.contains(mnemonic), "seed {seed:x} emitted {mnemonic}");
            }
        }
    }

    #[test]
    fn bmsk_generation_is_reachable() {
        let mut saw_bmsk = false;
        for seed in 0..2048 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes(&bytes).unwrap();
            if ptx.contains("bmsk.clamp.b32") {
                saw_bmsk = true;
                break;
            }
        }
        assert!(saw_bmsk, "no seed in sample emitted bmsk.clamp.b32");
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
            assert!(
                !ptx.contains("bmsk.clamp.b32"),
                "seed {seed:x} emitted bmsk.clamp.b32"
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
    fn wide_int_generation_is_reachable() {
        let mnemonics = [
            "add.u64",
            "sub.u64",
            "mul.lo.u64",
            "add.s64",
            "sub.s64",
            "mul.lo.s64",
            "and.b64",
            "or.b64",
            "xor.b64",
        ];
        let mut found = [false; 9];

        for seed in 0..32768 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes(&bytes).unwrap();
            for (i, mnemonic) in mnemonics.iter().enumerate() {
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

        let missing: Vec<_> = mnemonics
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
                "sub.s64",
                "mul.lo.s64",
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
    fn predicated_wide_shift_generation_is_reachable() {
        let cfg = GenConfig {
            emit_mul_wide: false,
            emit_wide_int: false,
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
    fn bfind_generation_is_reachable() {
        let mut saw_bfind = false;
        for seed in 0..1024 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes(&bytes).unwrap();
            if ptx.contains("bfind.") {
                saw_bfind = true;
                break;
            }
        }
        assert!(saw_bfind, "no seed in sample emitted bfind");
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
                !ptx.contains("slct.s32.s32"),
                "seed {seed:x} emitted slct.s32.s32"
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
    fn video_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_video: false,
            ..GenConfig::default()
        };

        for seed in 0..512 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            for mnemonic in [
                "vadd4.u32.u32.u32",
                "vsub4.u32.u32.u32",
                "vabsdiff4.u32.u32.u32.add",
                "vadd2.u32.u32.u32",
                "vsub2.u32.u32.u32",
                "vavrg2.u32.u32.u32",
                "vavrg2.u32.u32.u32.add",
                "vabsdiff2.u32.u32.u32.add",
                "vmin2.u32.u32.u32",
                "vmin2.u32.u32.u32.add",
                "vmax2.u32.u32.u32",
                "vmax2.u32.u32.u32.add",
                "vavrg4.u32.u32.u32",
                "vavrg4.u32.u32.u32.add",
                "vmin4.u32.u32.u32",
                "vmin4.u32.u32.u32.add",
                "vmax4.u32.u32.u32",
                "vmax4.u32.u32.u32.add",
            ] {
                assert!(!ptx.contains(mnemonic), "seed {seed:x} emitted {mnemonic}");
            }
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
            assert!(
                !has_mnemonic(&ptx, "vsub4.u32.u32.u32"),
                "seed {seed:x} emitted vsub4"
            );
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
    fn selp_generation_can_be_disabled() {
        let cfg = GenConfig {
            emit_selp: false,
            ..GenConfig::default()
        };

        for seed in 0..512 {
            let bytes = bytes_from_seed(seed, 4096);
            let ptx = generate_from_bytes_with_config(&bytes, &cfg).unwrap();
            assert!(!ptx.contains("selp.b32"), "seed {seed:x} emitted selp.b32");
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
            for mnemonic in ["shf.l.wrap.b32", "shf.r.wrap.b32"] {
                assert!(!ptx.contains(mnemonic), "seed {seed:x} emitted {mnemonic}");
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
