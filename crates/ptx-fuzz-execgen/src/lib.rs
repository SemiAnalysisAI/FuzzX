//! Generates PTX kernels for differential testing of ptxas.
//!
//! Kernel ABI matches `ptx_fuzz_exec`:
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
//!   * Integer ops only. No div (skips divisor-zero UB). No shifts by
//!     register (skips shift-amount UB). No FP.

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
        }
    }
}

/// Generate a PTX kernel from an `Unstructured` byte source.
pub fn generate(u: &mut Unstructured, cfg: &GenConfig) -> Result<String> {
    Generator::new(cfg).build(u)
}

/// Convenience: build an `Unstructured` from raw bytes and generate.
pub fn generate_from_bytes(bytes: &[u8]) -> Result<String> {
    let mut u = Unstructured::new(bytes);
    generate(&mut u, &GenConfig::default())
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

#[derive(Clone, Copy)]
enum BinOp { Add, Sub, Mul, MulHi, And, Or, Xor, Min, Max }

impl BinOp {
    fn mnemonic(self) -> &'static str {
        match self {
            BinOp::Add => "add.u32",
            BinOp::Sub => "sub.u32",
            BinOp::Mul => "mul.lo.u32",
            BinOp::MulHi => "mul.hi.u32",
            BinOp::And => "and.b32",
            BinOp::Or  => "or.b32",
            BinOp::Xor => "xor.b32",
            BinOp::Min => "min.u32",
            BinOp::Max => "max.u32",
        }
    }
}

#[derive(Clone, Copy)]
enum ShiftOp { Shl, Shr }

impl ShiftOp {
    fn mnemonic(self) -> &'static str {
        match self {
            ShiftOp::Shl => "shl.b32",
            ShiftOp::Shr => "shr.u32",
        }
    }
}

#[derive(Clone, Copy)]
enum UnaryOp { Not, Popc, Clz, Brev }

impl UnaryOp {
    fn mnemonic(self) -> &'static str {
        match self {
            UnaryOp::Not  => "not.b32",
            UnaryOp::Popc => "popc.b32",
            UnaryOp::Clz  => "clz.b32",
            UnaryOp::Brev => "brev.b32",
        }
    }
}

#[derive(Clone, Copy)]
enum CmpOp { Eq, Ne, Lt, Le, Gt, Ge }

impl CmpOp {
    fn mnemonic(self) -> &'static str {
        match self {
            CmpOp::Eq => "setp.eq.u32",
            CmpOp::Ne => "setp.ne.u32",
            CmpOp::Lt => "setp.lt.u32",
            CmpOp::Le => "setp.le.u32",
            CmpOp::Gt => "setp.gt.u32",
            CmpOp::Ge => "setp.ge.u32",
        }
    }
}

#[derive(Clone, Copy)]
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

enum Inst {
    Bin { op: BinOp, dst: u32, a: Operand, b: Operand },
    Sel { dst: u32, a: Operand, b: Operand, cmp: CmpOp, ca: Operand, cb: Operand, pred: u32 },
    /// `<op>.b32 dst, src, amount;` where amount is an immediate in 0..=31
    /// (avoids shift-amount-≥-32 UB).
    Shift { op: ShiftOp, dst: u32, src: Operand, amount: u32 },
    /// `<op>.b32 dst, src;`
    Unary { op: UnaryOp, dst: u32, src: Operand },
}

enum Term {
    Branch(usize),
    CondBranch { cmp: CmpOp, a: Operand, b: Operand, pred: u32, t: usize, f: usize },
    /// `if ctr == 0: bra fwd; else: ctr -= 1; bra back;`
    Loop { ctr: u32, pred: u32, back: usize, fwd: usize },
    Return,
}

struct Block { insts: Vec<Inst>, term: Term }

struct Generator<'a> {
    cfg: &'a GenConfig,
    n_working: u32,
    n_pred: u32,
    blocks: Vec<Block>,
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
            counters: Vec::new(),
        }
    }

    fn alloc_pred(&mut self) -> u32 {
        let p = self.n_pred;
        self.n_pred += 1;
        p
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

    fn build(mut self, u: &mut Unstructured) -> Result<String> {
        let min_blocks = self.cfg.min_blocks.max(1);
        let n_blocks = u.int_in_range(min_blocks..=self.cfg.max_blocks.max(min_blocks))?;
        for i in 0..n_blocks {
            let n_insts = u.int_in_range(
                self.cfg.min_insts_per_block..=self.cfg.max_insts_per_block.max(self.cfg.min_insts_per_block),
            )?;
            let mut insts = Vec::with_capacity(n_insts);
            for _ in 0..n_insts {
                insts.push(self.gen_inst(u)?);
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

    fn pick_dst(&mut self, u: &mut Unstructured) -> Result<u32> {
        u.int_in_range(0..=self.n_working - 1)
    }

    fn pick_operand(&mut self, u: &mut Unstructured) -> Result<Operand> {
        let pick: u8 = u.arbitrary()?;
        if pick < 192 {
            Ok(Operand::Reg(u.int_in_range(0..=self.n_working - 1)?))
        } else {
            Ok(Operand::Imm(u.int_in_range(0..=self.cfg.max_immediate)?))
        }
    }

    fn gen_inst(&mut self, u: &mut Unstructured) -> Result<Inst> {
        // ~55% bin, ~15% selp, ~15% shift (imm amount), ~15% unary
        let pick: u8 = u.arbitrary()?;
        if pick < 140 {
            Ok(Inst::Bin {
                op: pick_binop(u)?,
                dst: self.pick_dst(u)?,
                a: self.pick_operand(u)?,
                b: self.pick_operand(u)?,
            })
        } else if pick < 178 {
            Ok(Inst::Sel {
                dst: self.pick_dst(u)?,
                a: self.pick_operand(u)?,
                b: self.pick_operand(u)?,
                cmp: pick_cmp(u)?,
                ca: self.pick_operand(u)?,
                cb: self.pick_operand(u)?,
                pred: self.alloc_pred(),
            })
        } else if pick < 216 {
            Ok(Inst::Shift {
                op: pick_shift(u)?,
                dst: self.pick_dst(u)?,
                src: self.pick_operand(u)?,
                amount: u.int_in_range(0..=31)?,
            })
        } else {
            Ok(Inst::Unary {
                op: pick_unary(u)?,
                dst: self.pick_dst(u)?,
                src: self.pick_operand(u)?,
            })
        }
    }

    fn gen_terminator(&mut self, u: &mut Unstructured, i: usize, n_blocks: usize) -> Result<Term> {
        let pick: u8 = u.arbitrary()?;
        let fwd_lo = i + 1;
        let fwd_hi = n_blocks - 1;
        if pick < 102 {
            Ok(Term::Branch(u.int_in_range(fwd_lo..=fwd_hi)?))
        } else if pick < 178 {
            Ok(Term::CondBranch {
                cmp: pick_cmp(u)?,
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

    fn emit(&self) -> String {
        let mut s = String::with_capacity(4096);
        let tid_reg = self.tid_reg();
        let total_regs = (self.n_working + 1 + self.counters.len() as u32).max(1);

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

        // Epilogue: store output regs to out[tid * N_OUTPUTS * 4 ..]. Uses the
        // reserved tid reg, NOT %r1 (which the body is free to clobber).
        writeln!(s, "exit:").unwrap();
        writeln!(s, "    cvta.to.global.u64 %rd4, %rd1;").unwrap();
        writeln!(s, "    mul.wide.u32    %rd5, %r{tid_reg}, {};", N_OUTPUTS * 4).unwrap();
        writeln!(s, "    add.s64         %rd4, %rd4, %rd5;").unwrap();
        for k in 0..N_OUTPUTS {
            writeln!(s, "    st.global.u32   [%rd4 + {}], %r{k};", k * 4).unwrap();
        }
        writeln!(s, "    ret;").unwrap();
        writeln!(s, "}}").unwrap();
        s
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
            Inst::Sel { dst, a, b, cmp, ca, cb, pred } => {
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
            Inst::Shift { op, dst, src, amount } => {
                write!(s, "    {:<13} %r{dst}, ", op.mnemonic()).unwrap();
                src.emit(s);
                writeln!(s, ", {amount};").unwrap();
            }
            Inst::Unary { op, dst, src } => {
                write!(s, "    {:<13} %r{dst}, ", op.mnemonic()).unwrap();
                src.emit(s);
                writeln!(s, ";").unwrap();
            }
        }
    }

    fn emit_terminator(&self, s: &mut String, t: &Term) {
        match *t {
            Term::Branch(tgt) => {
                writeln!(s, "    bra             block_{tgt};").unwrap();
            }
            Term::CondBranch { cmp, a, b, pred, t: tt, f: ff } => {
                write!(s, "    {:<13} %p{pred}, ", cmp.mnemonic()).unwrap();
                a.emit(s);
                write!(s, ", ").unwrap();
                b.emit(s);
                writeln!(s, ";").unwrap();
                writeln!(s, "    @%p{pred} bra   block_{tt};").unwrap();
                writeln!(s, "    bra             block_{ff};").unwrap();
            }
            Term::Loop { ctr, pred, back, fwd } => {
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

fn pick_binop(u: &mut Unstructured) -> Result<BinOp> {
    let ops = [
        BinOp::Add, BinOp::Sub, BinOp::Mul, BinOp::MulHi,
        BinOp::And, BinOp::Or, BinOp::Xor, BinOp::Min, BinOp::Max,
    ];
    Ok(*u.choose(&ops)?)
}

fn pick_cmp(u: &mut Unstructured) -> Result<CmpOp> {
    let ops = [CmpOp::Eq, CmpOp::Ne, CmpOp::Lt, CmpOp::Le, CmpOp::Gt, CmpOp::Ge];
    Ok(*u.choose(&ops)?)
}

fn pick_shift(u: &mut Unstructured) -> Result<ShiftOp> {
    let ops = [ShiftOp::Shl, ShiftOp::Shr];
    Ok(*u.choose(&ops)?)
}

fn pick_unary(u: &mut Unstructured) -> Result<UnaryOp> {
    let ops = [UnaryOp::Not, UnaryOp::Popc, UnaryOp::Clz, UnaryOp::Brev];
    Ok(*u.choose(&ops)?)
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
