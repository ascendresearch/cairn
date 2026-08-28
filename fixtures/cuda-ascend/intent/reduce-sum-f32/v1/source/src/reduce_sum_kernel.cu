#include <cuda_runtime.h>
#include <stdint.h>

extern "C" __global__ void cairn_reduce_sum_f32_kernel(
    const float* input,
    float* output,
    uint32_t element_count) {
    __shared__ float partials[256];
    const uint32_t lane = threadIdx.x;
    partials[lane] = lane < element_count ? input[lane] : 0.0F;
    __syncthreads();

    for (uint32_t stride = 128; stride > 0; stride >>= 1) {
        if (lane < stride) {
            partials[lane] += partials[lane + stride];
        }
        __syncthreads();
    }

    if (lane == 0) {
        output[0] = partials[0];
    }
}
