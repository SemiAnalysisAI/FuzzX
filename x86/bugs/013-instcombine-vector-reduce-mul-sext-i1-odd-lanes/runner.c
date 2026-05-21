#include <stdio.h>
#include <stdint.h>
struct v3i1 { unsigned a:1; unsigned b:1; unsigned c:1; };
int8_t f(_Bool a, _Bool b, _Bool c);

int main(void){
    /* All-true input. Vector is <-1, -1, -1>, product = -1.
       Buggy InstCombine fold yields zext(and-reduce(<1,1,1>)) = 1. */
    int8_t r = f(1, 1, 1);
    printf("vector_reduce_mul(sext(<i1 1,1,1>)) = %d (expected -1)\n", r);
    if (r != -1) { puts("FAIL — odd-lane parity dropped"); return 1; }
    puts("OK");
    return 0;
}
