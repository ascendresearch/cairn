# scale-clamp-f32 evaluator task

The caller wants an asynchronous CUDA operation that multiplies every input value by a scalar and
then confines the result to a caller-provided closed interval, writing one output value per input
value.

The task owner has not declared what the operation should do when an input value is not a number,
nor what should happen if the caller passes an interval whose lower bound exceeds its upper bound.
Input and output do not overlap. The implementation and ABI below are the only artifacts offered
to the runtime SIR actor.
