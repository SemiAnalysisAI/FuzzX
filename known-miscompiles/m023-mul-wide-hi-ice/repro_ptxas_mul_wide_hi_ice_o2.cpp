// Standalone ptxas -O2 internal-compiler-error reproducer.
//
// This file embeds a small PTX kernel, assembles it with ptxas -O0 and -O2,
// runs the -O0 cubin through the CUDA Driver API to confirm the scalar result,
// and reports the optimized ptxas crash. It does not use an input buffer; the
// only kernel parameters are the output pointer and a u32 n.
//
// Build, typical x86 CUDA install:
//   g++ -std=c++17 -O2 repro_ptxas_mul_wide_hi_ice_o2.cpp \
//     -I/usr/local/cuda/include -L/usr/local/cuda/lib64/stubs -lcuda \
//     -o repro_ptxas_mul_wide_hi_ice_o2
//
// Build, CUDA SBSA install like this machine:
//   g++ -std=c++17 -O2 repro_ptxas_mul_wide_hi_ice_o2.cpp \
//     -I/usr/local/cuda/targets/sbsa-linux/include \
//     -L/usr/local/cuda/targets/sbsa-linux/lib/stubs -lcuda \
//     -o repro_ptxas_mul_wide_hi_ice_o2
//
// Run:
//   ./repro_ptxas_mul_wide_hi_ice_o2 [sm_XX]
//
// Optional:
//   PTXAS=/path/to/ptxas ./repro_ptxas_mul_wide_hi_ice_o2 sm_103
//
// The program returns 1 when the ptxas bug is reproduced: -O0 compiles and
// matches the scalar PTX trace, but -O2 crashes in ptxas.

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
constexpr uint32_t EXPECTED = 0xffffffa5u;
constexpr int OUTPUT_BYTES = 4;

static const char* kPtx = R"PTX(
.version 8.8
.target sm_103
.address_size 64

.entry fuzz_kernel(.param .u64 out_ptr, .param .u32 n)
{
    .reg .pred %p<1>;
    .reg .b32  %r<8>;
    .reg .b64  %rd<3>;

    ld.param.u64 %rd1, [out_ptr];
    ld.param.u32 %r0, [n];
    mov.u32      %r1, %tid.x;

    mul.wide.s32 %rd2, %r0, 0xffffffff;
    mov.b64      {%r4, %r5}, %rd2;
    mul.hi.s32   %r4, %r0, %r4;
    setp.ge.u32  %p0, %r4, %r1;
    selp.b32     %r7, 0xffffffa5, 0, %p0;

    st.global.u32 [%rd1], %r7;
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
        unlink((path + "/ptxas.log").c_str());
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

static std::string read_text(const std::string& path) {
    std::ifstream f(path);
    if (!f) {
        return "";
    }
    return std::string(std::istreambuf_iterator<char>(f),
                       std::istreambuf_iterator<char>());
}

struct CompileResult {
    bool ok = false;
    int status = 0;
    std::vector<char> cubin;
    std::string log;
};

static CompileResult compile_ptx(const std::string& ptxas,
                                 const std::string& arch,
                                 const char* opt) {
    TempDir dir("/tmp/ptxas_mul_wide_hi_ice.XXXXXX");
    const std::string ptx_path = dir.path + "/in.ptx";
    const std::string cubin_path = dir.path + "/out.cubin";
    const std::string log_path = dir.path + "/ptxas.log";
    write_text(ptx_path, kPtx);

    std::fflush(nullptr);
    pid_t pid = fork();
    if (pid < 0) {
        throw std::runtime_error(std::string("fork failed: ") + std::strerror(errno));
    }
    if (pid == 0) {
        FILE* err_log = std::freopen(log_path.c_str(), "w", stderr);
        FILE* out_log = std::freopen(log_path.c_str(), "a", stdout);
        (void)err_log;
        (void)out_log;
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

    CompileResult r;
    r.status = status;
    r.log = read_text(log_path);
    r.ok = WIFEXITED(status) && WEXITSTATUS(status) == 0;
    if (r.ok) {
        r.cubin = read_binary(cubin_path);
    }
    return r;
}

static void create_context(CUcontext* ctx, CUdevice dev) {
#if CUDA_VERSION >= 13000
    check(cuCtxCreate(ctx, nullptr, 0, dev), "cuCtxCreate");
#else
    check(cuCtxCreate(ctx, 0, dev), "cuCtxCreate");
#endif
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
        std::printf("Scalar expected -O0 output: 0x%08x\n\n", EXPECTED);

        auto o0 = compile_ptx(ptxas, arch, "-O0");
        auto o2 = compile_ptx(ptxas, arch, "-O2");

        if (!o0.ok) {
            std::cout << "-O0 ptxas unexpectedly failed, status " << o0.status << "\n";
            std::cout << o0.log << "\n";
            return 2;
        }

        check(cuInit(0), "cuInit");
        CUdevice dev = 0;
        CUcontext ctx = nullptr;
        check(cuDeviceGet(&dev, 0), "cuDeviceGet");
        create_context(&ctx, dev);

        uint32_t out_o0 = run_kernel(o0.cubin);
        cuCtxDestroy(ctx);

        std::printf("-O0 run: got 0x%08x expected 0x%08x%s\n",
                    out_o0, EXPECTED, out_o0 == EXPECTED ? "" : "  MISMATCH");
        if (out_o0 != EXPECTED) {
            return 2;
        }

        if (!o2.ok) {
            std::cout << "\nREPRODUCED: -O0 compiles and runs correctly, but -O2 ptxas fails.\n";
            std::cout << "-O2 ptxas status: " << o2.status << "\n";
            if (!o2.log.empty()) {
                std::cout << "-O2 ptxas output:\n" << o2.log << "\n";
            }
            return 1;
        }

        std::cout << "\nNot reproduced: -O2 ptxas compiled successfully on this setup.\n";
        return 0;
    } catch (const std::exception& e) {
        std::cerr << "error: " << e.what() << "\n";
        return 2;
    }
}
