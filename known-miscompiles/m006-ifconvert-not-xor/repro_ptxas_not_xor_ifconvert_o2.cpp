// Standalone ptxas -O2 miscompile reproducer.
//
// This file embeds a 37-line PTX kernel, assembles it twice with ptxas
// (-O0 and -O2), launches five threads through the CUDA Driver API, and
// compares their output against a scalar PTX trace.
//
// Build, typical x86 CUDA install:
//   g++ -std=c++17 -O2 repro_ptxas_not_xor_ifconvert_o2.cpp \
//     -I/usr/local/cuda/include -L/usr/local/cuda/lib64/stubs -lcuda \
//     -o repro_ptxas_not_xor_ifconvert_o2
//
// Build, CUDA SBSA install like this machine:
//   g++ -std=c++17 -O2 repro_ptxas_not_xor_ifconvert_o2.cpp \
//     -I/usr/local/cuda/targets/sbsa-linux/include \
//     -L/usr/local/cuda/targets/sbsa-linux/lib/stubs -lcuda \
//     -o repro_ptxas_not_xor_ifconvert_o2
//
// Run:
//   ./repro_ptxas_not_xor_ifconvert_o2 [sm_XX]
//
// Optional:
//   PTXAS=/path/to/ptxas ./repro_ptxas_not_xor_ifconvert_o2 sm_103
//
// Correct scalar behavior:
//   The kernel launches tids 0..4 with n = 32. For tids other than 4, the
//   branch skips the else path and leaves r0 = n, so:
//
//     out = 594 * n + n = 0x00004a60
//
//   For tid 4, the else path runs:
//
//     r0 = n ^ ~tid = 32 ^ ~4 = 0xffffffdb
//     out = 594 * 32 + 0xffffffdb = 0x00004a1b
//
// Observed bug with CUDA 13.0 ptxas V13.0.88 on sm_103 and with CUDA 13.2
// Update 1 ptxas V13.2.78 on sm_103:
//   -O0 matches the scalar trace.
//   -O2 stores 0x00004a64 for tid 4, as if the else path computed
//   `n ^ tid` instead of `n ^ ~tid`.
//
// SASS root-cause summary:
//   At -O2, ptxas if-converts the branch and folds `not` + `xor` into a
//   predicated LOP3:
//
//     ISETP.NE.U32.AND P0, PT, R5, 0x4, PT ;
//     @!P0 LOP3.LUT R0, R7, 0x4, RZ, 0x3c, !PT ;
//     IMAD R5, R7, 0x252, R0 ;
//
//   For the false path, ptxas has substituted tid with the known value 4, but
//   the LOP3 truth table `0x3c` computes `n ^ 4`. The PTX source requires
//   `n ^ ~4`. The correct truth table/source expression would need to preserve
//   the complement.

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
constexpr int N_THREADS = 5;
constexpr int OUTPUT_BYTES = N_THREADS * 4;

static const char* kPtx = R"PTX(
.version 8.8
.target sm_103
.address_size 64

.visible .entry fuzz_kernel(
    .param .u64 out_ptr,
    .param .u32 n
)
{
    .reg .pred %p<1>;
    .reg .b32 %r<8>;
    .reg .b64 %rd<3>;

    ld.param.u64 %rd0, [out_ptr];
    ld.param.u32 %r0, [n];
    mov.u32 %r2, %tid.x;
    mov.u32 %r1, %r2;
    mov.u32 %r3, %r0;
    mov.u32 %r4, 4;
    mov.u32 %r5, %r2;
    mov.u32 %r7, %r0;
    setp.ne.u32 %p0, %r4, %r1;
    @%p0 bra then;
    bra else;
then:
    bra done;
else:
    not.b32 %r6, %r5;
    xor.b32 %r0, %r7, %r6;
    bra done;
done:
    mad.lo.u32 %r1, 594, %r3, %r0;
    cvta.to.global.u64 %rd0, %rd0;
    mul.wide.u32 %rd1, %r2, 4;
    add.s64 %rd2, %rd0, %rd1;
    st.global.u32 [%rd2], %r1;
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
    TempDir dir("/tmp/ptxas_not_xor_ifconvert_repro.XXXXXX");
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

static uint32_t expected_value(uint32_t tid) {
    uint32_t r0 = KERNEL_N;
    uint32_t r3 = r0;
    if (tid == 4) {
        r0 = KERNEL_N ^ ~tid;
    }
    return 594u * r3 + r0;
}

static std::vector<uint32_t> run_kernel(const std::vector<char>& cubin) {
    CUmodule module = nullptr;
    CUfunction fn = nullptr;
    CUdeviceptr d_out = 0;
    std::vector<uint32_t> out(N_THREADS, 0);

    check(cuModuleLoadData(&module, cubin.data()), "cuModuleLoadData");
    check(cuModuleGetFunction(&fn, module, "fuzz_kernel"), "cuModuleGetFunction");
    check(cuMemAlloc(&d_out, OUTPUT_BYTES), "cuMemAlloc output");
    check(cuMemsetD8(d_out, 0xa5, OUTPUT_BYTES), "cuMemsetD8 output");

    uint32_t n = KERNEL_N;
    void* params[] = { &d_out, &n };
    check(cuLaunchKernel(fn, 1, 1, 1, N_THREADS, 1, 1, 0, nullptr, params, nullptr),
          "cuLaunchKernel");
    check(cuCtxSynchronize(), "cuCtxSynchronize");
    check(cuMemcpyDtoH(out.data(), d_out, OUTPUT_BYTES), "cuMemcpyDtoH output");

    cuMemFree(d_out);
    cuModuleUnload(module);
    return out;
}

static int report(const char* label, const std::vector<uint32_t>& got) {
    int bad = 0;
    for (uint32_t tid = 0; tid < N_THREADS; ++tid) {
        uint32_t expect = expected_value(tid);
        bool ok = got[tid] == expect;
        std::printf("%s tid %u: got 0x%08x expected 0x%08x%s\n",
                    label, tid, got[tid], expect, ok ? "" : "  MISMATCH");
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
        std::cout << "Kernel n:    " << KERNEL_N << "\n\n";

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
