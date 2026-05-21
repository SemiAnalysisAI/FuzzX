#include <stdio.h>
#include <stdint.h>
__int128 sub_i128(__int128 a, __int128 b);
int main(void){
    /* a = 2^64, b = 1. a-b should be 2^64 - 1 = (lo=0xff..ff, hi=0). */
    __int128 a = ((__int128)1) << 64;
    __int128 b = 1;
    __int128 r = sub_i128(a, b);
    uint64_t lo = (uint64_t)r, hi = (uint64_t)(r >> 64);
    printf("sub128(1<<64, 1) = (hi=0x%016llx lo=0x%016llx) expected (hi=0 lo=0xff..ff)\n",
           (unsigned long long)hi, (unsigned long long)lo);
    if (hi == 0 && lo == 0xFFFFFFFFFFFFFFFFULL) { puts("OK"); return 0; }
    puts("FAIL — GISel sub-i128 borrow inverted"); return 1;
}
