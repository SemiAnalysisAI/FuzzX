// Standalone ptxas -O2 miscompile reproducer.
//
// This file embeds a small PTX kernel, assembles it twice with ptxas (-O0 and
// -O2), runs both cubins through the CUDA Driver API, and compares their output
// against the scalar PTX trace. It does not use an input buffer; the only kernel
// parameters are the output pointer and a u32 n.
//
// Build, typical x86 CUDA install:
//   g++ -std=c++17 -O2 repro_ptxas_mixed_minmax_signedness_o2.cpp \
//     -I/usr/local/cuda/include -L/usr/local/cuda/lib64/stubs -lcuda \
//     -o repro_ptxas_mixed_minmax_signedness_o2
//
// Build, CUDA SBSA install like this machine:
//   g++ -std=c++17 -O2 repro_ptxas_mixed_minmax_signedness_o2.cpp \
//     -I/usr/local/cuda/targets/sbsa-linux/include \
//     -L/usr/local/cuda/targets/sbsa-linux/lib/stubs -lcuda \
//     -o repro_ptxas_mixed_minmax_signedness_o2
//
// Run:
//   ./repro_ptxas_mixed_minmax_signedness_o2 [sm_XX]
//
// Optional:
//   PTXAS=/path/to/ptxas ./repro_ptxas_mixed_minmax_signedness_o2 sm_103
//
// The program returns 1 when the ptxas bug is reproduced: -O0 matches the
// scalar PTX trace, but -O2 does not.
//
// Correct scalar behavior for the embedded PTX:
//   The kernel launches one thread, receives n = 32, and stores one u32.
//
//     r1 = n << 26                  = 0x80000000
//     r1 = r1 | 24                  = 0x80000018
//     r2 = max.u32(0, r1)           = 0x80000018
//     r3 = min.s32(r2, 0)           = 0x80000018
//     r4 = bfi(r3, 24, pos=5, len=7)= 0x00000318
//
//   The signed min is the key point: 0x80000018 is negative as s32, so
//   min.s32(0x80000018, 0) must keep 0x80000018. The bfi then inserts the low
//   seven bits, 0x18, at bit position 5, producing 0x318.
//
// Observed bug with CUDA 13.0 ptxas V13.0.88 on sm_103 and with the latest
// NVIDIA CUDA Toolkit available on 2026-05-15, CUDA 13.2 Update 1 ptxas
// V13.2.78 (build cuda_13.2.r13.2/compiler.37668154_0), on sm_103:
//   -O0 output: 0x00000318, which matches the scalar trace.
//   -O2 output: 0x00000018, which is wrong. This is as if ptxas folded
//   min.s32(max.u32(0, x), 0) to 0, incorrectly treating the result of the
//   unsigned max as nonnegative under the later signed min.
//
// SASS root-cause summary:
//   -O0 emits the expected SHF/LOP3, VIMNMX.U32, then signed VIMNMX sequence.
//   -O2 drops the n-dependent computation entirely and stores a constant 0x18.

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

constexpr uint32_t KERNEL_N = 32;
constexpr int OUTPUT_BYTES = 4;

static const char* kPtx = R"PTX(
.version 8.8
.target sm_103
.address_size 64

.entry fuzz_kernel(.param .u64 out_ptr, .param .u32 n)
{
    .reg .b32 %r<5>;
    .reg .b64 %rd<1>;

    ld.param.u64 %rd0, [out_ptr];
    ld.param.u32 %r0, [n];

    shl.b32  %r1, %r0, 26;
    or.b32   %r1, %r1, 24;
    max.u32  %r2, 0, %r1;
    min.s32  %r3, %r2, 0;
    bfi.b32  %r4, %r3, 24, 5, 7;

    st.global.u32 [%rd0], %r4;
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
    TempDir dir("/tmp/ptxas_mixed_minmax_repro.XXXXXX");
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

static int32_t bitcast_i32(uint32_t x) {
    int32_t y = 0;
    std::memcpy(&y, &x, sizeof(y));
    return y;
}

static uint32_t min_s32(uint32_t a, uint32_t b) {
    return bitcast_i32(a) <= bitcast_i32(b) ? a : b;
}

static uint32_t bfi(uint32_t a, uint32_t b, uint32_t pos, uint32_t len) {
    const uint32_t mask = len == 32 ? 0xffffffffu : ((1u << len) - 1u) << pos;
    return (b & ~mask) | ((a << pos) & mask);
}

static uint32_t expected_value(uint32_t n) {
    uint32_t r1 = n << 26;
    r1 |= 24;
    uint32_t r2 = r1;       // max.u32(0, r1)
    uint32_t r3 = min_s32(r2, 0);
    return bfi(r3, 24, 5, 7);
}

static uint32_t run_kernel(const std::vector<char>& cubin) {
    CUmodule module = nullptr;
    CUfunction fn = nullptr;
    CUdeviceptr d_out = 0;
    uint32_t out = 0;

    check(cuModuleLoadData(&module, cubin.data()), "cuModuleLoadData");
    check(cuModuleGetFunction(&fn, module, "fuzz_kernel"), "cuModuleGetFunction");
    check(cuMemAlloc(&d_out, OUTPUT_BYTES), "cuMemAlloc output");
    check(cuMemsetD8(d_out, 0xa5, OUTPUT_BYTES), "cuMemsetD8 output");

    uint32_t n = KERNEL_N;
    void* params[] = { &d_out, &n };
    check(cuLaunchKernel(fn, 1, 1, 1, 1, 1, 1, 0, nullptr, params, nullptr),
          "cuLaunchKernel");
    check(cuCtxSynchronize(), "cuCtxSynchronize");
    check(cuMemcpyDtoH(&out, d_out, OUTPUT_BYTES), "cuMemcpyDtoH output");

    cuMemFree(d_out);
    cuModuleUnload(module);
    return out;
}

static int report(const char* label, uint32_t got) {
    uint32_t expect = expected_value(KERNEL_N);
    bool ok = got == expect;
    std::printf("%s: got 0x%08x expected 0x%08x%s\n",
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
        std::cout << "Kernel n:    " << KERNEL_N << "\n";
        std::printf("Scalar expected value: 0x%08x\n\n", expected_value(KERNEL_N));

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
