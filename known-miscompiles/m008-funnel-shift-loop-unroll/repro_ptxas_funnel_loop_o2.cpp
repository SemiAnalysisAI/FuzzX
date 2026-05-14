// Standalone ptxas -O2 miscompile reproducer.
//
// This file embeds a reduced PTX kernel, assembles it twice with ptxas
// (-O0 and -O2), launches one thread through the CUDA Driver API, and compares
// its output against a scalar PTX trace.
//
// Build, typical x86 CUDA install:
//   g++ -std=c++17 -O2 repro_ptxas_funnel_loop_o2.cpp \
//     -I/usr/local/cuda/include -L/usr/local/cuda/lib64/stubs -lcuda \
//     -o repro_ptxas_funnel_loop_o2
//
// Build, CUDA SBSA install like this machine:
//   g++ -std=c++17 -O2 repro_ptxas_funnel_loop_o2.cpp \
//     -I/usr/local/cuda/targets/sbsa-linux/include \
//     -L/usr/local/cuda/targets/sbsa-linux/lib/stubs -lcuda \
//     -o repro_ptxas_funnel_loop_o2
//
// Run:
//   ./repro_ptxas_funnel_loop_o2 [sm_XX]
//
// Optional:
//   PTXAS=/path/to/ptxas ./repro_ptxas_funnel_loop_o2 sm_103
//
// Correct scalar behavior:
//   The launch uses tid = 0. The loop starts with:
//
//     r0 = 32, r9 = tid = 0, r11 = 32
//
//   and executes this body six times:
//
//     r8  = 0 - r0
//     r10 = r11 & r9
//     r11 = r11 * r10 + r11
//     r0  = r10 ^ 4096
//     r3  = shf.r.wrap.b32(r8, 469, 9)
//     r9  = r3 + 4
//
//   For amount 9, `shf.r.wrap.b32(a, 469, 9)` is:
//
//     (a >> 9) | (469 << 23)
//
//   The six-iteration scalar trace for tid 0 is:
//
//     iter  r10        r11 after   r0 after    r3         r9 after
//       0   00000000   00000020    00001000    eaffffff   eb000003
//       1   00000000   00000020    00001000    eafffff8   eafffffc
//       2   00000020   00000420    00001020    eafffff8   eafffffc
//       3   00000420   00110820    00001420    eafffff7   eafffffb
//       4   00110820   14930c20    00111820    eafffff5   eafffff9
//       5   00930c20   81e61020    00931c20    eafff773   eafff777
//
//   The correct stored value is therefore 0x00931c20.
//
// Observed bug with CUDA 13.0 ptxas V13.0.88 on sm_103 and with CUDA 13.2
// Update 1 ptxas V13.2.78 on sm_103:
//   -O0 matches the scalar trace.
//   -O2 stores 0x14131c20.
//
// SASS root-cause summary:
//   At -O0, ptxas keeps the loop and emits the expected SHF.R.W.U32 for the
//   source `shf.r.wrap.b32`.
//
//   At -O2, ptxas fully unrolls the loop and rewrites the loop-carried
//   funnel-shift recurrence into LEA.HI plus LOP3 expressions. The final store
//   is produced by a collapsed expression of the form:
//
//     LEA.HI   R0, -R9, R4, 0x1d5, 0x17 ;     // computes the r9 mask
//     LOP3.LUT R5, R7, 0x1000, R0, 0x6c ;     // (R7 & R0) ^ 0x1000
//     STG.E    ..., R5
//
//   The PTX requires the final `r10` to be `0x14930c20 & 0xeafffff9 =
//   0x00930c20`, and then `r0 = r10 ^ 0x1000 = 0x00931c20`. The optimized
//   cubin instead stores `0x14131c20`, which means the mask used by the
//   collapsed final expression is not the PTX `shf.r.wrap` result for that
//   loop-carried value.
//
//   Replacing the PTX `shf.r.wrap.b32` with the equivalent explicit
//   `shr.u32` plus `or.b32 0xea800000` makes -O2 match -O0, so this is a bug
//   in optimized funnel-shift recurrence lowering, not in the scalar trace.

#include <cuda.h>

#include <cerrno>
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <fstream>
#include <iostream>
#include <sstream>
#include <stdexcept>
#include <string>
#include <sys/wait.h>
#include <unistd.h>
#include <vector>

constexpr int OUTPUT_STRIDE_BYTES = 16;

static const char* kPtx = R"PTX(
.version 8.8
.target sm_103
.address_size 64

.visible .entry fuzz_kernel(
    .param .u64 out_ptr
)
{
    .reg .pred  %p<1>;
    .reg .b32   %r<14>;
    .reg .b64   %rd<4>;

    ld.param.u64    %rd1, [out_ptr];
    mov.u32         %r0, 32;
    mov.u32         %r1, %tid.x;
    mov.u32         %r9, %r1;
    mov.u32         %r11, %r0;
    mov.u32         %r13, 6;

loop_header:
    setp.eq.u32   %p0, %r13, 0;
    @%p0 bra   loop_done;
    sub.u32         %r13, %r13, 1;
    sub.u32       %r8, 0, %r0;
    and.b32       %r10, %r11, %r9;
    mad.lo.u32    %r11, %r11, %r10, %r11;
    xor.b32       %r0, %r10, 4096;
    shf.r.wrap.b32 %r3, %r8, 469, 9;
    add.u32       %r9, %r3, 4;
    bra             loop_header;
loop_done:
    cvta.to.global.u64 %rd1, %rd1;
    mul.wide.u32    %rd2, %r1, 16;
    add.s64         %rd3, %rd1, %rd2;
    st.global.u32   [%rd3 + 0], %r0;
    ret;
}
)PTX";

static void check(CUresult r, const char* op) {
    if (r == CUDA_SUCCESS) {
        return;
    }
    const char* msg = nullptr;
    cuGetErrorString(r, &msg);
    std::ostringstream os;
    os << op << " failed: " << (msg ? msg : "unknown CUDA error");
    throw std::runtime_error(os.str());
}

struct TempDir {
    std::string path;
    explicit TempDir(const char* pattern) {
        std::vector<char> buf(pattern, pattern + std::strlen(pattern) + 1);
        char* p = mkdtemp(buf.data());
        if (!p) {
            throw std::runtime_error(std::string("mkdtemp failed: ") + std::strerror(errno));
        }
        path = p;
    }
    ~TempDir() {
        unlink((path + "/in.ptx").c_str());
        unlink((path + "/out.cubin").c_str());
        rmdir(path.c_str());
    }
};

static void write_text(const std::string& path, const char* text) {
    std::ofstream f(path);
    if (!f) {
        throw std::runtime_error("failed to open " + path);
    }
    f << text;
}

static std::vector<char> read_binary(const std::string& path) {
    std::ifstream f(path, std::ios::binary);
    if (!f) {
        throw std::runtime_error("failed to open " + path);
    }
    return std::vector<char>(std::istreambuf_iterator<char>(f),
                             std::istreambuf_iterator<char>());
}

static std::vector<char> compile_ptx(const std::string& ptxas,
                                     const std::string& arch,
                                     const char* opt) {
    TempDir dir("/tmp/ptxas_funnel_loop_repro.XXXXXX");
    const std::string ptx_path = dir.path + "/in.ptx";
    const std::string cubin_path = dir.path + "/out.cubin";
    write_text(ptx_path, kPtx);

    pid_t pid = fork();
    if (pid < 0) {
        throw std::runtime_error(std::string("fork failed: ") + std::strerror(errno));
    }
    if (pid == 0) {
        std::string arch_arg = "-arch=" + arch;
        execlp(ptxas.c_str(), ptxas.c_str(), arch_arg.c_str(), opt,
               "-o", cubin_path.c_str(), ptx_path.c_str(), static_cast<char*>(nullptr));
        std::perror("execlp ptxas");
        _exit(127);
    }

    int status = 0;
    if (waitpid(pid, &status, 0) < 0) {
        throw std::runtime_error(std::string("waitpid failed: ") + std::strerror(errno));
    }
    if (!WIFEXITED(status) || WEXITSTATUS(status) != 0) {
        std::ostringstream os;
        os << "ptxas " << opt << " failed with status " << status;
        throw std::runtime_error(os.str());
    }
    return read_binary(cubin_path);
}

static void create_context(CUcontext* ctx, CUdevice dev) {
#if CUDA_VERSION >= 13000
    check(cuCtxCreate(ctx, nullptr, 0, dev), "cuCtxCreate");
#else
    check(cuCtxCreate(ctx, 0, dev), "cuCtxCreate");
#endif
}

static uint32_t shf_r_wrap(uint32_t a, uint32_t b, uint32_t amount) {
    amount &= 31;
    if (amount == 0) {
        return a;
    }
    return static_cast<uint32_t>((a >> amount) | (b << (32 - amount)));
}

static uint32_t expected_output() {
    uint32_t r0 = 32;
    uint32_t r9 = 0;
    uint32_t r11 = r0;

    for (int i = 0; i < 6; ++i) {
        uint32_t r8 = 0u - r0;
        uint32_t r10 = r11 & r9;
        r11 = static_cast<uint32_t>(r11 * r10 + r11);
        r0 = r10 ^ 4096u;
        uint32_t r3 = shf_r_wrap(r8, 469, 9);
        r9 = r3 + 4;
    }
    return r0;
}

static uint32_t run_kernel(const std::vector<char>& cubin) {
    CUmodule module = nullptr;
    CUfunction fn = nullptr;
    CUdeviceptr d_out = 0;
    uint32_t out = 0;

    check(cuModuleLoadData(&module, cubin.data()), "cuModuleLoadData");
    check(cuModuleGetFunction(&fn, module, "fuzz_kernel"), "cuModuleGetFunction");
    check(cuMemAlloc(&d_out, OUTPUT_STRIDE_BYTES), "cuMemAlloc output");
    check(cuMemsetD8(d_out, 0xa5, OUTPUT_STRIDE_BYTES), "cuMemsetD8 output");

    void* params[] = { &d_out };
    check(cuLaunchKernel(fn, 1, 1, 1, 1, 1, 1, 0, nullptr, params, nullptr),
          "cuLaunchKernel");
    check(cuCtxSynchronize(), "cuCtxSynchronize");
    check(cuMemcpyDtoH(&out, d_out, sizeof(out)), "cuMemcpyDtoH output");

    cuMemFree(d_out);
    cuModuleUnload(module);
    return out;
}

static int report(const char* label, uint32_t got) {
    uint32_t expect = expected_output();
    bool ok = got == expect;
    std::printf("%s out[0]: got 0x%08x expected 0x%08x%s\n",
                label, got, expect, ok ? "" : "  MISMATCH");
    return ok ? 0 : 1;
}

int main(int argc, char** argv) {
    try {
        const char* env_ptxas = std::getenv("PTXAS");
        std::string ptxas = env_ptxas ? env_ptxas : "/usr/local/cuda/bin/ptxas";
        if (access(ptxas.c_str(), X_OK) != 0) {
            ptxas = "ptxas";
        }
        std::string arch = (argc >= 2) ? argv[1] : "sm_103";

        std::cout << "Using ptxas: " << ptxas << "\n";
        std::cout << "Using arch:  " << arch << "\n";
        std::cout << "Expected:    0x" << std::hex << expected_output() << std::dec << "\n\n";

        auto cubin_o0 = compile_ptx(ptxas, arch, "-O0");
        auto cubin_o2 = compile_ptx(ptxas, arch, "-O2");

        check(cuInit(0), "cuInit");
        CUdevice dev = 0;
        CUcontext ctx = nullptr;
        check(cuDeviceGet(&dev, 0), "cuDeviceGet");
        create_context(&ctx, dev);

        uint32_t out_o0 = run_kernel(cubin_o0);
        uint32_t out_o2 = run_kernel(cubin_o2);

        int bad_o0 = report("-O0", out_o0);
        int bad_o2 = report("-O2", out_o2);

        cuCtxDestroy(ctx);

        if (bad_o0 == 0 && bad_o2 != 0) {
            std::cout << "\nREPRODUCED: -O0 matches the scalar PTX trace, but -O2 is wrong.\n";
            return 1;
        }
        if (bad_o0 != 0) {
            std::cout << "\nUnexpected: -O0 did not match the scalar PTX trace.\n";
            return 2;
        }
        std::cout << "\nNot reproduced: -O2 matched the scalar PTX trace on this setup.\n";
        return 0;
    } catch (const std::exception& e) {
        std::cerr << "error: " << e.what() << "\n";
        return 2;
    }
}
