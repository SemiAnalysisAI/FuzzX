//! Compile PTX with `ptxas` and run the resulting cubin on a CUDA device.
//!
//! Generated kernels follow a fixed ABI:
//!
//! ```text
//! .visible .entry <name>(.param .u64 in, .param .u64 out, .param .u32 n)
//! ```
//!
//! `in` and `out` are device pointers to byte buffers the caller manages; `n`
//! is a single scalar (the generator uses it as an input-length / iteration
//! bound). All non-FFI errors surface as `anyhow::Error`.

use std::ffi::{c_char, c_void, CStr, CString};
use std::marker::PhantomData;
use std::path::PathBuf;
use std::process::Command;
use std::ptr;

use anyhow::{anyhow, bail, Context as _, Result};

// ===== Raw FFI =====

#[allow(non_camel_case_types)]
mod sys {
    use std::ffi::{c_char, c_int, c_uint, c_void};

    pub type CUresult = c_int;
    pub type CUdevice = c_int;
    pub type CUcontext = *mut c_void;
    pub type CUmodule = *mut c_void;
    pub type CUfunction = *mut c_void;
    pub type CUstream = *mut c_void;
    pub type CUdeviceptr = u64;

    pub const CUDA_SUCCESS: CUresult = 0;

    #[link(name = "cuda")]
    extern "C" {
        pub fn cuInit(flags: c_uint) -> CUresult;
        pub fn cuDeviceGet(device: *mut CUdevice, ordinal: c_int) -> CUresult;
        pub fn cuCtxCreate_v2(pctx: *mut CUcontext, flags: c_uint, dev: CUdevice) -> CUresult;
        pub fn cuCtxDestroy_v2(ctx: CUcontext) -> CUresult;
        pub fn cuCtxSynchronize() -> CUresult;
        pub fn cuModuleLoadData(module: *mut CUmodule, image: *const c_void) -> CUresult;
        pub fn cuModuleUnload(module: CUmodule) -> CUresult;
        pub fn cuModuleGetFunction(
            hfunc: *mut CUfunction,
            hmod: CUmodule,
            name: *const c_char,
        ) -> CUresult;
        pub fn cuMemAlloc_v2(dptr: *mut CUdeviceptr, bytesize: usize) -> CUresult;
        pub fn cuMemFree_v2(dptr: CUdeviceptr) -> CUresult;
        pub fn cuMemcpyHtoD_v2(dst: CUdeviceptr, src: *const c_void, n: usize) -> CUresult;
        pub fn cuMemcpyDtoH_v2(dst: *mut c_void, src: CUdeviceptr, n: usize) -> CUresult;
        pub fn cuMemsetD8_v2(dst: CUdeviceptr, uc: u8, n: usize) -> CUresult;
        pub fn cuLaunchKernel(
            f: CUfunction,
            grid_x: c_uint, grid_y: c_uint, grid_z: c_uint,
            block_x: c_uint, block_y: c_uint, block_z: c_uint,
            shared_mem_bytes: c_uint,
            stream: CUstream,
            kernel_params: *mut *mut c_void,
            extra: *mut *mut c_void,
        ) -> CUresult;
        pub fn cuGetErrorString(error: CUresult, p_str: *mut *const c_char) -> CUresult;
    }
}

fn check(code: sys::CUresult, op: &'static str) -> Result<()> {
    if code == sys::CUDA_SUCCESS {
        return Ok(());
    }
    let msg = unsafe {
        let mut s: *const c_char = ptr::null();
        if sys::cuGetErrorString(code, &mut s) == sys::CUDA_SUCCESS && !s.is_null() {
            CStr::from_ptr(s).to_string_lossy().into_owned()
        } else {
            format!("CUresult {code}")
        }
    };
    Err(anyhow!("CUDA call {op} failed: {msg}"))
}

// ===== ptxas =====

fn ptxas_path() -> PathBuf {
    if let Ok(p) = std::env::var("PTXAS") {
        return PathBuf::from(p);
    }
    if let Ok(home) = std::env::var("HOME") {
        let p = PathBuf::from(home).join("bin").join("ptxas");
        if p.exists() {
            return p;
        }
    }
    PathBuf::from("ptxas")
}

/// Compile PTX text to a cubin via `ptxas`.
///
/// `flags` is passed verbatim before `-o <cubin> <ptx>`. Typical use:
/// `compile(ptx, &["-arch=sm_103", "-O3"])`.
pub fn compile(ptx: &str, flags: &[&str]) -> Result<Vec<u8>> {
    let dir = tempfile::tempdir()?;
    let ptx_path = dir.path().join("in.ptx");
    let cubin_path = dir.path().join("out.cubin");
    std::fs::write(&ptx_path, ptx)?;

    let bin = ptxas_path();
    let out = Command::new(&bin)
        .args(flags)
        .arg("-o")
        .arg(&cubin_path)
        .arg(&ptx_path)
        .output()
        .with_context(|| format!("spawning ptxas at {}", bin.display()))?;
    if !out.status.success() {
        bail!(
            "ptxas {} failed ({}): {}",
            flags.join(" "),
            out.status,
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    std::fs::read(&cubin_path).with_context(|| format!("reading {}", cubin_path.display()))
}

// ===== CUDA wrapper =====

/// Owned device allocation; freed on drop.
struct DeviceBuf(sys::CUdeviceptr);

impl DeviceBuf {
    unsafe fn alloc(n: usize) -> Result<Self> {
        // cuMemAlloc rejects 0-byte requests; bump to 1 so callers don't have to.
        let mut p: sys::CUdeviceptr = 0;
        check(sys::cuMemAlloc_v2(&mut p, n.max(1)), "cuMemAlloc")?;
        Ok(Self(p))
    }
    fn ptr(&self) -> sys::CUdeviceptr {
        self.0
    }
}

impl Drop for DeviceBuf {
    fn drop(&mut self) {
        unsafe {
            let _ = sys::cuMemFree_v2(self.0);
        }
    }
}

/// Owned module handle; unloaded on drop.
struct Module(sys::CUmodule);

impl Module {
    unsafe fn load(cubin: &[u8]) -> Result<Self> {
        let mut m: sys::CUmodule = ptr::null_mut();
        check(
            sys::cuModuleLoadData(&mut m, cubin.as_ptr() as *const c_void),
            "cuModuleLoadData",
        )?;
        Ok(Self(m))
    }
    unsafe fn function(&self, name: &str) -> Result<sys::CUfunction> {
        let c = CString::new(name)?;
        let mut f: sys::CUfunction = ptr::null_mut();
        check(
            sys::cuModuleGetFunction(&mut f, self.0, c.as_ptr()),
            "cuModuleGetFunction",
        )?;
        Ok(f)
    }
}

impl Drop for Module {
    fn drop(&mut self) {
        unsafe {
            let _ = sys::cuModuleUnload(self.0);
        }
    }
}

/// A CUDA primary-ish context bound to the creating thread.
pub struct Cuda {
    ctx: sys::CUcontext,
    // CUDA contexts are per-thread; don't let this struct cross threads.
    _not_send: PhantomData<*const ()>,
}

impl Cuda {
    pub fn init(device_ordinal: i32) -> Result<Self> {
        unsafe {
            check(sys::cuInit(0), "cuInit")?;
            let mut dev: sys::CUdevice = 0;
            check(sys::cuDeviceGet(&mut dev, device_ordinal), "cuDeviceGet")?;
            let mut ctx: sys::CUcontext = ptr::null_mut();
            check(sys::cuCtxCreate_v2(&mut ctx, 0, dev), "cuCtxCreate")?;
            Ok(Self { ctx, _not_send: PhantomData })
        }
    }

    /// Load a cubin and launch its `kernel_name` entry with our fixed ABI.
    ///
    /// `input` is copied to a device buffer (the `in` param). Output of
    /// `output_len` bytes is zero-initialized on the device, written by the
    /// kernel, then copied back.
    pub fn launch(
        &self,
        cubin: &[u8],
        kernel_name: &str,
        grid: (u32, u32, u32),
        block: (u32, u32, u32),
        input: &[u8],
        output_len: usize,
        n: u32,
    ) -> Result<Vec<u8>> {
        unsafe {
            let module = Module::load(cubin)?;
            let func = module.function(kernel_name)?;

            let d_in = DeviceBuf::alloc(input.len())?;
            let d_out = DeviceBuf::alloc(output_len)?;

            if !input.is_empty() {
                check(
                    sys::cuMemcpyHtoD_v2(d_in.ptr(), input.as_ptr() as *const c_void, input.len()),
                    "cuMemcpyHtoD",
                )?;
            }
            check(
                sys::cuMemsetD8_v2(d_out.ptr(), 0, output_len.max(1)),
                "cuMemsetD8",
            )?;

            // kernelParams: array of pointers to each argument value.
            let mut arg_in = d_in.ptr();
            let mut arg_out = d_out.ptr();
            let mut arg_n = n;
            let mut params: [*mut c_void; 3] = [
                &mut arg_in as *mut _ as *mut c_void,
                &mut arg_out as *mut _ as *mut c_void,
                &mut arg_n as *mut _ as *mut c_void,
            ];

            check(
                sys::cuLaunchKernel(
                    func,
                    grid.0, grid.1, grid.2,
                    block.0, block.1, block.2,
                    0,
                    ptr::null_mut(),
                    params.as_mut_ptr(),
                    ptr::null_mut(),
                ),
                "cuLaunchKernel",
            )?;
            check(sys::cuCtxSynchronize(), "cuCtxSynchronize")?;

            let mut buf = vec![0u8; output_len];
            if output_len > 0 {
                check(
                    sys::cuMemcpyDtoH_v2(buf.as_mut_ptr() as *mut c_void, d_out.ptr(), output_len),
                    "cuMemcpyDtoH",
                )?;
            }
            Ok(buf)
        }
    }
}

impl Drop for Cuda {
    fn drop(&mut self) {
        unsafe {
            let _ = sys::cuCtxDestroy_v2(self.ctx);
        }
    }
}

// ===== Differential oracle =====

/// Outcome of running the same PTX program at two different opt levels.
pub struct DiffOutcome {
    pub o0: Result<Vec<u8>>,
    pub o3: Result<Vec<u8>>,
}

impl DiffOutcome {
    /// Both compilations succeeded *and* produced bit-identical output.
    pub fn matches(&self) -> bool {
        matches!((&self.o0, &self.o3), (Ok(a), Ok(b)) if a == b)
    }

    /// At least one of: outputs differ; one side compiled/launched but the
    /// other didn't. Both-failed is not "diverged" (probably an invalid kernel).
    pub fn diverged(&self) -> bool {
        match (&self.o0, &self.o3) {
            (Ok(a), Ok(b)) => a != b,
            (Ok(_), Err(_)) | (Err(_), Ok(_)) => true,
            (Err(_), Err(_)) => false,
        }
    }
}

/// Compile `ptx` twice (`-O0` and `-O3`), launch each on `cuda` with the
/// shared inputs, and return both outcomes for the caller to classify.
///
/// Both pipelines are run unconditionally so that asymmetric ptxas or launch
/// failures (one side OK, the other not) are still observable.
pub fn differential(
    cuda: &Cuda,
    ptx: &str,
    arch: &str,
    kernel_name: &str,
    grid: (u32, u32, u32),
    block: (u32, u32, u32),
    input: &[u8],
    output_len: usize,
    n: u32,
) -> DiffOutcome {
    let run_at = |opt: &str| -> Result<Vec<u8>> {
        let cubin = compile(ptx, &[arch, opt])?;
        cuda.launch(&cubin, kernel_name, grid, block, input, output_len, n)
    };
    DiffOutcome {
        o0: run_at("-O0"),
        o3: run_at("-O3"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diff_outcome_classification() {
        let same = DiffOutcome { o0: Ok(vec![1, 2, 3]), o3: Ok(vec![1, 2, 3]) };
        assert!(same.matches() && !same.diverged());

        let diff = DiffOutcome { o0: Ok(vec![1, 2, 3]), o3: Ok(vec![1, 2, 4]) };
        assert!(!diff.matches() && diff.diverged());

        let asym = DiffOutcome { o0: Ok(vec![1]), o3: Err(anyhow!("boom")) };
        assert!(!asym.matches() && asym.diverged());

        let both_failed = DiffOutcome { o0: Err(anyhow!("a")), o3: Err(anyhow!("b")) };
        assert!(!both_failed.matches() && !both_failed.diverged());
    }

    #[test]
    fn ptxas_compile_smoke() {
        let ptx = r#".version 8.8
.target sm_103
.address_size 64

.visible .entry noop()
{
    ret;
}
"#;
        let cubin = compile(ptx, &["-arch=sm_103", "-O3"]).expect("compile");
        assert!(cubin.len() > 100, "cubin suspiciously small: {} bytes", cubin.len());
    }
}
