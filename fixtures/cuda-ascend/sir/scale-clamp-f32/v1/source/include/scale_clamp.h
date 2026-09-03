#pragma once

#include <cuda_runtime_api.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

cudaError_t launch_scale_clamp_f32(
    const float* input,
    uint32_t count,
    float scale,
    float lower,
    float upper,
    float* output,
    cudaStream_t stream);

#ifdef __cplusplus
}
#endif
