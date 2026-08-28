# D-040 deterministic Intent qualification controls

This public bundle freezes the qualification contract and independently authored controls for the
ten mechanism slots required by D-040. It consumes the exact DEV-001 public bundle and its redacted
private-review receipt. It does not contain a mechanism implementation, qualification receipt,
`MigrationIntentContract`, or admitted outcome.

The ten control suites define expected behavior before DEV-100, DEV-102, DEV-103, and DEV-104 write
the corresponding implementation subjects. Those later slices must bind exact source, dependency,
toolchain, calibration environment, scope, and limitations and must produce their own independent
qualification receipts before Gate use.

Private wrong-binding and redaction canaries live only in the restricted qualification-control
store. This public tree records only their categories, review state, and eventually a redacted
control-review receipt identity. Control-review authority is distinct from mechanism-qualification
authority.

No recorded workflow, host calibration, CUDA build/run, Ascend build/NPU run, profiler, model, or
model-integration lane is executed by DEV-002.
