// Standalone ptxas -O2 miscompile reproducer.
//
// This file embeds a reduced PTX kernel, assembles it twice with ptxas
// (-O0 and -O2), launches one thread through the CUDA Driver API, and compares
// its output against a scalar PTX trace.
//
// Build, typical x86 CUDA install:
//   g++ -std=c++17 -O2 repro_ptxas_signed_unsigned_ifconvert_o2.cpp \
//     -I/usr/local/cuda/include -L/usr/local/cuda/lib64/stubs -lcuda \
//     -o repro_ptxas_signed_unsigned_ifconvert_o2
//
// Build, CUDA SBSA install like this machine:
//   g++ -std=c++17 -O2 repro_ptxas_signed_unsigned_ifconvert_o2.cpp \
//     -I/usr/local/cuda/targets/sbsa-linux/include \
//     -L/usr/local/cuda/targets/sbsa-linux/lib/stubs -lcuda \
//     -o repro_ptxas_signed_unsigned_ifconvert_o2
//
// Run:
//   ./repro_ptxas_signed_unsigned_ifconvert_o2 [sm_XX]
//
// Optional:
//   PTXAS=/path/to/ptxas ./repro_ptxas_signed_unsigned_ifconvert_o2 sm_103
//
// Correct scalar behavior:
//   The launch uses n = 32 and one input word x = 0xe4ca6123. Interpreted as
//   signed s32, x is negative, so the outer signed compare `x <= 32` is true.
//   Interpreted as unsigned u32, x is much larger than 32, so the inner
//   unsigned compare `32 >= x` is false. The kernel must therefore take the
//   inner_else path:
//
//     r0 = 32 | 345 = 0x00000179
//     r3 remains 32
//
//   The correct output tuple is:
//
//     { r0, tid, x, r3 } = { 0x179, 0, 0xe4ca6123, 0x20 }
//
// Observed bug with CUDA 13.0 ptxas V13.0.88 on sm_103 and with CUDA 13.2
// Update 1 ptxas V13.2.78 on sm_103:
//   -O0 matches the scalar trace.
//   -O2 stores r0 = 0x20 and r3 = x * 8 = 0x26530918, as if the inner unsigned
//   compare were true after the outer signed compare.
//
// SASS root-cause summary:
//   At -O2, ptxas if-converts the nested branches and removes the inner
//   unsigned test. The optimized SASS effectively has only:
//
//     ISETP.GT.AND P0, PT, x, 32, PT ;  // signed x > 32
//     @P0  SHF.L.U32 r0, x, 0xb, RZ ;   // outer_else: x << 11
//     @!P0 SHF.L.U32 r3, x, 0x3, RZ ;   // inner_then: x * 8
//
//   For x = 0xe4ca6123, signed `x > 32` is false, but unsigned `32 >= x` is
//   also false. The PTX requires the inner_else `or.b32`; the optimized SASS
//   incorrectly assumes the inner_then path.

#include <cuda.h>

#include <array>
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
constexpr uint32_t INPUT_X = 0xe4ca6123u;
constexpr int N_OUTPUTS = 4;
constexpr int OUTPUT_BYTES = N_OUTPUTS * 4;

static const char* kPtx = R"PTX(
.version 8.8
.target sm_103
.address_size 64

.visible .entry fuzz_kernel(
    .param .u64 in_ptr,
    .param .u64 out_ptr,
    .param .u32 in_n
)
{
    .reg .pred  %p<2>;
    .reg .b32   %r<4>;
    .reg .b64   %rd<8>;

    ld.param.u64    %rd0, [in_ptr];
    ld.param.u64    %rd1, [out_ptr];
    ld.param.u32    %r0, [in_n];
    mov.u32         %r1, %tid.x;
    cvta.to.global.u64 %rd2, %rd0;
    mul.wide.u32    %rd3, %r1, 4;
    add.s64         %rd2, %rd2, %rd3;
    ld.global.u32   %r2, [%rd2];
    mov.u32         %r3, %r0;

    setp.le.s32   %p0, %r2, %r0;
    @%p0 bra   outer_then;
    bra             outer_else;
outer_then:
    setp.ge.u32   %p1, %r3, %r2;
    @%p1 bra   inner_then;
    bra             inner_else;
inner_then:
    mul.lo.u32    %r3, %r2, 8;
    bra             outer_done;
inner_else:
    or.b32        %r0, %r0, 345;
    bra             outer_done;
outer_else:
    shl.b32       %r0, %r2, 11;
    bra             outer_done;
outer_done:
    cvta.to.global.u64 %rd4, %rd1;
    mul.wide.u32    %rd5, %r1, 16;
    add.s64         %rd4, %rd4, %rd5;
    st.global.u32   [%rd4 + 0], %r0;
    st.global.u32   [%rd4 + 4], %r1;
    st.global.u32   [%rd4 + 8], %r2;
    st.global.u32   [%rd4 + 12], %r3;
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
    TempDir dir("/tmp/ptxas_signed_unsigned_ifconvert_repro.XXXXXX");
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

static std::array<uint32_t, N_OUTPUTS> expected_output() {
    return {KERNEL_N | 345u, 0u, INPUT_X, KERNEL_N};
}

static std::array<uint32_t, N_OUTPUTS> run_kernel(const std::vector<char>& cubin) {
    CUmodule module = nullptr;
    CUfunction fn = nullptr;
    CUdeviceptr d_in = 0;
    CUdeviceptr d_out = 0;
    std::array<uint32_t, N_OUTPUTS> out = {};

    check(cuModuleLoadData(&module, cubin.data()), "cuModuleLoadData");
    check(cuModuleGetFunction(&fn, module, "fuzz_kernel"), "cuModuleGetFunction");
    check(cuMemAlloc(&d_in, sizeof(INPUT_X)), "cuMemAlloc input");
    check(cuMemAlloc(&d_out, OUTPUT_BYTES), "cuMemAlloc output");
    check(cuMemcpyHtoD(d_in, &INPUT_X, sizeof(INPUT_X)), "cuMemcpyHtoD input");
    check(cuMemsetD8(d_out, 0xa5, OUTPUT_BYTES), "cuMemsetD8 output");

    uint32_t n = KERNEL_N;
    void* params[] = { &d_in, &d_out, &n };
    check(cuLaunchKernel(fn, 1, 1, 1, 1, 1, 1, 0, nullptr, params, nullptr),
          "cuLaunchKernel");
    check(cuCtxSynchronize(), "cuCtxSynchronize");
    check(cuMemcpyDtoH(out.data(), d_out, OUTPUT_BYTES), "cuMemcpyDtoH output");

    cuMemFree(d_out);
    cuMemFree(d_in);
    cuModuleUnload(module);
    return out;
}

static int report(const char* label, const std::array<uint32_t, N_OUTPUTS>& got) {
    const auto expect = expected_output();
    int bad = 0;
    for (int i = 0; i < N_OUTPUTS; ++i) {
        bool ok = got[i] == expect[i];
        std::printf("%s out[%d]: got 0x%08x expected 0x%08x%s\n",
                    label, i, got[i], expect[i], ok ? "" : "  MISMATCH");
        bad += ok ? 0 : 1;
    }
    return bad;
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
        std::cout << "Input x:     0x" << std::hex << INPUT_X << std::dec << "\n\n";

        auto cubin_o0 = compile_ptx(ptxas, arch, "-O0");
        auto cubin_o2 = compile_ptx(ptxas, arch, "-O2");

        check(cuInit(0), "cuInit");
        CUdevice dev = 0;
        CUcontext ctx = nullptr;
        check(cuDeviceGet(&dev, 0), "cuDeviceGet");
        create_context(&ctx, dev);

        auto out_o0 = run_kernel(cubin_o0);
        auto out_o2 = run_kernel(cubin_o2);

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
