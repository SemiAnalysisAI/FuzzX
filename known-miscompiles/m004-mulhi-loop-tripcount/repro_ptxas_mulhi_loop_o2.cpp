// Standalone ptxas -O2 miscompile reproducer.
//
// This file embeds a 30-line PTX kernel, assembles it twice with ptxas
// (-O0 and -O2), runs both cubins through the CUDA Driver API, and compares
// their output against a scalar PTX trace. It does not use an input buffer;
// the only kernel parameters are the output pointer and a u32 n.
//
// Build, typical x86 CUDA install:
//   g++ -std=c++17 -O2 repro_ptxas_mulhi_loop_o2.cpp \
//     -I/usr/local/cuda/include -L/usr/local/cuda/lib64/stubs -lcuda \
//     -o repro_ptxas_mulhi_loop_o2
//
// Build, CUDA SBSA install like this machine:
//   g++ -std=c++17 -O2 repro_ptxas_mulhi_loop_o2.cpp \
//     -I/usr/local/cuda/targets/sbsa-linux/include \
//     -L/usr/local/cuda/targets/sbsa-linux/lib/stubs -lcuda \
//     -o repro_ptxas_mulhi_loop_o2
//
// Run:
//   ./repro_ptxas_mulhi_loop_o2 [sm_XX]
//
// The program returns 1 when the ptxas bug is reproduced: -O0 matches the
// scalar trace, but -O2 does not.
//
// Optional:
//   PTXAS=/path/to/ptxas ./repro_ptxas_mulhi_loop_o2 sm_103
//
// Correct scalar behavior for the embedded PTX:
//   The kernel launches one thread, receives n = 32, and stores one u32.
//
//     r1 = tid.x = 0
//     r2 = 4
//     loop while r2 != 0:
//       r2 = r2 - 1
//       r3 = 0xc4787a77
//       r3 = mul.hi.s32(r3, n)
//       r1 = r1 + r3
//
//   Interpreted as signed 32-bit, 0xc4787a77 is -998737289.
//   Therefore:
//
//     mul.hi.s32(0xc4787a77, 32)
//       = high_32_bits((-998737289) * 32)
//       = high_32_bits(-31959593248)
//       = -8
//       = 0xfffffff8
//
//   The loop has exactly four iterations, so the only correct output is:
//
//     0 + 4 * 0xfffffff8 = 0xffffffe0
//
// Observed bug with CUDA 13.2 Update 1 ptxas V13.2.78 on sm_103:
//   -O0 output: 0xffffffe0, which matches the scalar trace.
//   -O2 output: 0xfffffff0, which is wrong. This is as if only two of the
//   four loop-carried additions were kept.
//
// SASS root-cause summary:
//   -O0 emits a real counted loop. Each trip computes the signed high multiply
//   with IMAD.HI and then adds it into the loop-carried accumulator.
//
//   -O2 removes the loop, but the optimized cubin contains only two IMAD.HI
//   high-multiply contributions before the store. Since each contribution is
//   -8 for n = 32, the optimized code stores -16 instead of -32. The PTX loop
//   counter starts at 4 and is decremented once per iteration, so dropping two
//   recurrence updates is illegal.

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
constexpr uint32_t MULHI_A = 3296230007u;  // 0xc4787a77
constexpr int OUTPUT_BYTES = 4;

static const char* kPtx = R"PTX(
.version 8.8
.target sm_103
.address_size 64

.visible .entry fuzz_kernel(
    .param .u64 out_ptr,
    .param .u32 in_n
)
{
    .reg .pred  %p<1>;
    .reg .b32   %r<4>;
    .reg .b64   %rd<1>;

    ld.param.u64    %rd0, [out_ptr];
    ld.param.u32    %r0, [in_n];
    mov.u32         %r1, %tid.x;
    mov.u32         %r2, 4;
loop:
    setp.eq.u32   %p0, %r2, 0;
    @%p0 bra      done;
    sub.u32       %r2, %r2, 1;
    mov.u32       %r3, 3296230007;
    mul.hi.s32    %r3, %r3, %r0;
    add.u32       %r1, %r1, %r3;
    bra           loop;
done:
    cvta.to.global.u64 %rd0, %rd0;
    st.global.u32   [%rd0], %r1;
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
    TempDir dir("/tmp/ptxas_mulhi_loop_repro.XXXXXX");
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

static uint32_t mul_hi_s32(uint32_t a, uint32_t b) {
    const int64_t word = 0x100000000LL;
    int64_t prod = static_cast<int64_t>(bitcast_i32(a)) *
                   static_cast<int64_t>(bitcast_i32(b));
    int64_t hi = 0;
    if (prod >= 0) {
        hi = prod / word;
    } else {
        hi = -(((-prod) + word - 1) / word);
    }
    return static_cast<uint32_t>(static_cast<int32_t>(hi));
}

static uint32_t expected_value(uint32_t n) {
    uint32_t r1 = 0;  // tid.x for the one launched thread.
    uint32_t r2 = 4;
    while (r2 != 0) {
        r2 -= 1;
        uint32_t r3 = mul_hi_s32(MULHI_A, n);
        r1 += r3;
    }
    return r1;
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
