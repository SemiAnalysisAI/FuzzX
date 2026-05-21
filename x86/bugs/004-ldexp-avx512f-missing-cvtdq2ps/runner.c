#include <stdint.h>
#include <stdio.h>
#include <math.h>

typedef float v4f __attribute__((vector_size(16)));
typedef int   v4i __attribute__((vector_size(16)));

v4f ldexp_v4f32(v4f x, v4i e);

int main(void) {
    v4f x = { 1.0f, 2.0f, 4.0f, 8.0f };
    v4i e = { 1, 2, 3, 4 };
    v4f r = ldexp_v4f32(x, e);
    float exp_arr[4] = { 2.0f, 8.0f, 32.0f, 128.0f };
    int bad = 0;
    for (int i = 0; i < 4; i++) {
        printf("lane %d: got %g  expected %g\n", i, r[i], exp_arr[i]);
        if (r[i] != exp_arr[i]) bad = 1;
    }
    if (bad) { puts("FAIL — ldexp produced wrong value (LowerFLDEXP non-VLX bug)"); return 1; }
    puts("OK");
    return 0;
}
