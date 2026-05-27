cmake_minimum_required(VERSION 3.20)

project(FuzzXLLVMSPIRVCrashFuzzer LANGUAGES C CXX)

find_package(LLVM REQUIRED CONFIG)

message(STATUS "Using LLVM ${LLVM_PACKAGE_VERSION} from ${LLVM_DIR}")

list(APPEND CMAKE_MODULE_PATH "${LLVM_CMAKE_DIR}")
include(HandleLLVMOptions)

set(FUZZER_SANITIZERS "fuzzer" CACHE STRING
    "Comma-separated sanitizer list for the fuzzer binary")

add_executable(llvm_spirv_crash_fuzzer llvm_spirv_crash_fuzzer.cpp)
target_compile_features(llvm_spirv_crash_fuzzer PRIVATE cxx_std_17)
target_include_directories(llvm_spirv_crash_fuzzer PRIVATE
    ${LLVM_INCLUDE_DIRS}
)
target_compile_definitions(llvm_spirv_crash_fuzzer PRIVATE
    ${LLVM_DEFINITIONS}
)

if(CMAKE_CXX_COMPILER_ID MATCHES "Clang")
    target_compile_options(llvm_spirv_crash_fuzzer PRIVATE
        -fno-omit-frame-pointer
        -fsanitize=${FUZZER_SANITIZERS}
    )
    target_link_options(llvm_spirv_crash_fuzzer PRIVATE
        -fsanitize=${FUZZER_SANITIZERS}
    )
endif()

if(TARGET LLVM)
    target_link_libraries(llvm_spirv_crash_fuzzer PRIVATE LLVM)
else()
    llvm_map_components_to_libnames(LLVM_LIBS
        Analysis
        BitReader
        BitWriter
        CodeGen
        Core
        IRReader
        MC
        Passes
        SPIRVCodeGen
        SPIRVDesc
        SPIRVInfo
        Support
        Target
        TargetParser
        TransformUtils
    )
    target_link_libraries(llvm_spirv_crash_fuzzer PRIVATE ${LLVM_LIBS})
endif()
