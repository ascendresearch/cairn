#ifndef CAIRN_REDUCE_SUM_F32_V1_H
#define CAIRN_REDUCE_SUM_F32_V1_H

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

enum CairnReduceSumF32Status {
    CAIRN_REDUCE_SUM_F32_OK = 0,
    CAIRN_REDUCE_SUM_F32_INVALID_ARGUMENT = 1,
    CAIRN_REDUCE_SUM_F32_CUDA_FAILURE = 2
};

int cairn_reduce_sum_f32(const float* input, float* output, uint32_t element_count);

#ifdef __cplusplus
}
#endif

#endif
