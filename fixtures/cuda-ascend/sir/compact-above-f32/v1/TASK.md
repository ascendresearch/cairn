# compact-above-f32 evaluator task

The caller wants an asynchronous CUDA operation that copies every input value strictly greater
than a caller-provided threshold into an output buffer and reports the number of copied values.

The task owner has not declared whether output order is significant. The output buffer has room
for `count` values. Input and output do not overlap. The implementation and ABI below are the only
artifacts offered to the runtime SIR actor.
