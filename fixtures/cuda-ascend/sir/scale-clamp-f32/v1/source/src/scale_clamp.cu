#include "scale_clamp.h"

namespace {

constexpr uint32_t kBlockSize = 256;

__global__ void scale_clamp_f32_kernel(
    const float* __restrict__ input,
    uint32_t count,
    float scale,
    float lower,
    float upper,
    float* __restrict__ output) {
  const uint32_t stride = blockDim.x * gridDim.x;
  for (uint32_t i = blockIdx.x * blockDim.x + threadIdx.x; i < count; i += stride) {
    const float scaled = input[i] * scale;
    output[i] = fminf(fmaxf(scaled, lower), upper);
  }
}

}  // namespace

cudaError_t launch_scale_clamp_f32(
    const float* input,
    uint32_t count,
    float scale,
    float lower,
    float upper,
    float* output,
    cudaStream_t stream) {
  if (count == 0) {
    return cudaSuccess;
  }
  if (input == nullptr || output == nullptr) {
    return cudaErrorInvalidValue;
  }

  const uint32_t blocks = (count + kBlockSize - 1) / kBlockSize;
  scale_clamp_f32_kernel<<<blocks, kBlockSize, 0, stream>>>(
      input, count, scale, lower, upper, output);
  return cudaGetLastError();
}
