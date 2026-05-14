// Standalone ptxas -O2 miscompile reproducer.
//
// This file embeds a 27-line PTX kernel, assembles it twice with ptxas
// (-O0 and -O2), runs both cubins through the CUDA Driver API, and compares
// their output against the scalar PTX trace.  It does not use an input buffer;
// the only kernel parameter is the output pointer.
//
// Build, typical x86 CUDA install:
//   g++ -std=c++17 -O2 repro_ptxas_lop3_o2.cpp \
//     -I/usr/local/cuda/include -L/usr/local/cuda/lib64/stubs -lcuda \
//     -o repro_ptxas_lop3_o2
//
// Build, CUDA SBSA install like this machine:
//   g++ -std=c++17 -O2 repro_ptxas_lop3_o2.cpp \
//     -I/usr/local/cuda/targets/sbsa-linux/include \
//     -L/usr/local/cuda/targets/sbsa-linux/lib/stubs -lcuda \
//     -o repro_ptxas_lop3_o2
//
// Run:
//   ./repro_ptxas_lop3_o2 [sm_XX]
//
// The program returns 1 when the ptxas bug is reproduced: -O0 matches the
// scalar trace, but -O2 does not.
//
// Optional:
//   PTXAS=/path/to/ptxas ./repro_ptxas_lop3_o2 sm_103
//
// Correct scalar behavior for the embedded PTX:
//   The kernel launches 32 threads and stores one u32 per thread at out[tid].
//   Every thread executes the same value computation:
//
//     r3 = 1
//     r7 = 0
//     p1 = (r3 < 15)      // true
//     r0 = p1 ? r3 : r7   // 1
//     r7 = lop3(26, r0, r0, 0x65)
//     r0 = r7 ^ 26
//
//   With PTX lop3 truth-table indexing, lop3(26, 1, 1, 0x65) is 0xffffffe4,
//   so the final xor computes 0xfffffffe.  Therefore every thread must store
//   0xfffffffe.
//
// Observed bug with CUDA 13.0 ptxas V13.0.88 on sm_103 and with CUDA 13.2
// Update 1 ptxas V13.2.78 on sm_103:
//   -O0 output: every thread stores 0xfffffffe, which matches the scalar trace.
//   -O2 output: every thread stores 0xffffffff, which is wrong.
//
// SASS root-cause summary:
//   -O0 keeps the value computation as SEL + LOP3 + XOR-as-LOP3.
//   -O2 folds the value computation to:
//
//     HFMA2 R0, -RZ, RZ, 0, 5.9604644775390625e-08 ;
//     LOP3.LUT R5, R0, 0x1a, RZ, 0x95, !PT ;
//
//   That folded sequence computes 0xffffffff, not the required 0xfffffffe.
//   This appears to be an incorrect selp/lop3/xor boolean fold or truth-table
//   rewrite, not the uniform loop-predicate bug from the other testcase.

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

constexpr int N_THREADS = 32;
constexpr int OUTPUT_BYTES = N_THREADS * 4;

static const char* kPtx = R"PTX(
.version 8.8
.target sm_103
.address_size 64

.entry fuzz_kernel(.param .u64 out_ptr)
{
    .reg .pred  %p<2>;
    .reg .b32   %r<9>;
    .reg .b64   %rd<6>;

    ld.param.u64    %rd1, [out_ptr];
    mov.u32         %r8, %tid.x;
    mov.u32         %r3, 1;
    mov.u32         %r7, 0;

label:
    setp.lt.s32   %p1, %r3, 15;
    selp.b32      %r0, %r3, %r7, %p1;
    lop3.b32      %r7, 26, %r0, %r0, 0x65;
    xor.b32       %r0, %r7, 26;

    cvta.to.global.u64 %rd4, %rd1;
    mul.wide.u32    %rd5, %r8, 4;
    add.s64         %rd4, %rd4, %rd5;
    st.global.u32   [%rd4], %r0;
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
    TempDir dir("/tmp/ptxas_lop3_repro.XXXXXX");
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

static uint32_t lop3(uint32_t a, uint32_t b, uint32_t c, uint8_t imm) {
    uint32_t out = 0;
    for (uint32_t bit = 0; bit < 32; ++bit) {
        uint32_t idx = (((a >> bit) & 1) << 2)
                     | (((b >> bit) & 1) << 1)
                     | (((c >> bit) & 1) << 0);
        out |= (((imm >> idx) & 1) << bit);
    }
    return out;
}

static uint32_t expected_value() {
    uint32_t r3 = 1;
    uint32_t r7 = 0;
    bool p1 = static_cast<int32_t>(r3) < 15;
    uint32_t r0 = p1 ? r3 : r7;
    r7 = lop3(26, r0, r0, 0x65);
    r0 = r7 ^ 26;
    return r0;
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

    void* params[] = { &d_out };
    check(cuLaunchKernel(fn, 1, 1, 1, N_THREADS, 1, 1, 0, nullptr, params, nullptr),
          "cuLaunchKernel");
    check(cuCtxSynchronize(), "cuCtxSynchronize");
    check(cuMemcpyDtoH(out.data(), d_out, OUTPUT_BYTES), "cuMemcpyDtoH output");

    cuMemFree(d_out);
    cuModuleUnload(module);
    return out;
}

static int report(const char* label, const std::vector<uint32_t>& out) {
    int mismatches = 0;
    uint32_t expect = expected_value();
    std::cout << "=== " << label << " ===\n";
    for (uint32_t tid = 0; tid < N_THREADS; ++tid) {
        uint32_t got = out[tid];
        bool ok = got == expect;
        mismatches += ok ? 0 : 1;
        std::printf("tid %2u: out got 0x%08x expected 0x%08x%s\n",
                    tid, got, expect, ok ? "" : "  MISMATCH");
    }
    std::cout << label << " mismatches: " << mismatches << "\n\n";
    return mismatches;
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
        std::printf("Scalar expected value: 0x%08x\n\n", expected_value());

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
            std::cout << "REPRODUCED: -O0 matches the scalar PTX trace, but -O2 is wrong.\n";
            return 1;
        }
        if (bad_o0 != 0) {
            std::cout << "Unexpected: -O0 did not match the scalar PTX trace.\n";
            return 2;
        }
        std::cout << "Not reproduced: -O2 matched the scalar PTX trace on this setup.\n";
        return 0;
    } catch (const std::exception& e) {
        std::cerr << "error: " << e.what() << "\n";
        return 2;
    }
}
