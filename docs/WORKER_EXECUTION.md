# Docker worker execution

- Status: F2 implemented and measured on a real Docker daemon
- Backend: `docker-v1`
- Scope: trusted private infrastructure running operator-submitted migration jobs

`cairn-worker join` creates an enrolled but disabled worker. Enrollment proves identity; an
operator explicitly enables execution after Docker and the required immutable image are present.
Cairn assumes the operator is responsible for the code and images submitted to that private
environment. The Docker adapter provides repeatable packaging, configurable resource bounds, and
recoverable process state. It is not a hostile multi-tenant security product or a malware scanner.

## Activation

Change the generated worker configuration coherently:

```json
{
  "availability": {
    "active_attempts": [],
    "available_slots": 1,
    "draining": false,
    "health": "ready"
  },
  "execution": {
    "accelerator": {
      "kind": "none"
    },
    "command": "/usr/bin/docker",
    "logical_cpu_limit": null,
    "memory_byte_limit": null,
    "mode": "docker",
    "pids_limit": null,
    "poll_interval_ms": 10,
    "state_directory": "state/docker",
    "writable_byte_limit": null
  },
  "profile": {
    "backends": ["docker-v1"],
    "max_concurrency": 1
  }
}
```

The fragment shows only changed fields. Retain the other generated fields. Each resource limit is
independently optional; `null` disables it. The worker currently accepts one concurrent attempt so
that its durable journal and availability claim remain simple.

`execution.accelerator` is mandatory whenever Docker execution is enabled. It is a closed V1 local
policy, not job-controlled Docker argv:

- `{"kind":"none"}` exposes no accelerator;
- `{"kind":"nvidia","device_index":0}` derives exactly `--gpus device=0`;
- `{"kind":"ascend","device_index":3}` maps only `/dev/davinci3`,
  `/dev/davinci_manager`, and `/dev/hisi_hdc`, mounts the fixed Ascend driver directory read-only,
  adds only `DAC_OVERRIDE`, and fixes the container-visible device index to
  `ASCEND_RT_VISIBLE_DEVICES=0`.

Vendor-specific device indices are distinct bounded types. Ascend activation fails before the
worker connects if the fixed driver path or any required character device is absent. The selected
host device and the in-container index are deliberately different concepts; jobs cannot override
the worker-derived container-visible index. Each terminal execution receipt records the exact
accelerator policy observation beside the immutable image and container identities.

The job environment is strict canonical JSON containing a full local Docker image ID of the form
`sha256:<64 lowercase hex digits>` and sorted environment variables. Mutable tags are rejected.
The input bundle contains explicit directories and regular files only. The command is argv, never a
shell string.

## Execution and recovery

For each `AttemptId`, the worker uses one deterministic container name and one state directory. It
persists the worker `started` fact before invoking Docker. On worker restart it reconstructs every
locally started, non-terminal attempt from SQLite and reconciles that same container:

- absent: materialize verified input and create it;
- created: start it;
- running: wait using the original Docker start time;
- exited: capture the existing result without rerunning it.

The container receives a read-only root and input bind, an operator-owned output bind, temporary
work directories, no network, a numeric non-root user, dropped capabilities, and
`no-new-privileges`. CPU, memory, PID, and writable-work limits are included only when configured.
These defaults reduce accidental interference but do not change the trusted-environment assumption.

Stdout, stderr, and declared output files are checked against the job's configurable capture
bounds. The terminal observation and outbox message are committed to SQLite before the worker
removes the container and attempt directory. A cleanup failure never causes another execution.

After terminal reconciliation, orchestration releases the exact scheduler reservation through
`cairn-server reservation release <controller.json> <reservation-id> <command-id>`. The command
reconstructs assignment state and fails closed for live, accepted, running, or in-doubt work; it is
not a direct scheduler-ledger edit.

## Real Hello World gate

Use any already-pulled image whose full ID is available locally:

```bash
docker image inspect --format '{{.Id}}' postgres:16-alpine
scripts/docker-hello-smoke.sh sha256:<64-hex-image-id>
```

The smoke test runs an executable from the content-addressed input bundle, checks `hello world` on
stdout and a declared output artifact, then asks the executor for the same exited attempt again and
requires a byte-identical terminal capture. Ordinary CI leaves this host-dependent test ignored.

## Real managed GPU worker gate

With the controller running and an enrolled ready GB10 worker, run:

```bash
scripts/real-gpu-worker-smoke.sh controller.json sha256:<64-hex-GPU-image-id>
```

The gate archives a content-addressed executable and Docker environment, schedules through the live
registry and worker-control protocol, executes `nvidia-smi` in the remote device-bound container,
then reconstructs the terminal receipt from controller SQLite/CAS. It requires stdout `NVIDIA GB10`
and trusted evidence `docker:accelerator:nvidia:0`, then releases the exact terminal reservation.
The test is ignored in ordinary CI and is safe to run consecutively; two immediate passes are the
repeatability gate for reservation cleanup and control-message quiescence.

## Real CUDA reduction gate

The source-side product gate uses Alloyport's original `cuda-reduction-v1` intake rather than a
prepared Cairn fixture:

```bash
scripts/real-cuda-reduction-smoke.sh \
  controller.json \
  sha256:<64-hex-GPU-image-id> \
  /path/to/alloyport/fixtures/migrations/cuda-reduction-v1/input
```

The gate accepts exactly the fixture's CMake file, public header, two CUDA sources, and reference
driver. Those bytes, the fixed offline build runner, immutable image ID, `sm_121` build adaptation,
placement requirements, and command are all content-addressed. The managed worker must advertise
the matching NVIDIA architecture, vendor, and device index. It configures and builds with CUDA 13,
runs all nine release cases, and requires the exact deterministic checksum plus trusted device-0
evidence from the terminal receipt. Two consecutive live passes are recorded for the GB10.

Docker mounts `/cairn/work` as an explicitly executable writable tmpfs because compiler jobs must
run the products they just linked. `/tmp` remains explicitly `noexec`; both mounts retain the
configured byte ceiling, non-root ownership, `nosuid`, and `nodev`.

## Real no-device Ascend build gate

The shared Ascend host runs a separate `npu-build` worker identity. Its V1 profile advertises only
the build role and exact `ascend`, `9.1.0-beta.1`, and `dav-3510` toolchain capabilities. Its Docker
policy is `accelerator: none`; it has no device nodes or driver mount and remains independent of the
device worker, which is unavailable and draining while the shared cards are occupied.

Run the toolchain gate with:

```bash
scripts/real-ascend-build-smoke.sh \
  controller.json \
  sha256:<64-hex-Ascend-build-image-id> \
  /path/to/alloyport/fixtures/ascend-add-v1
```

The content-addressed job copies the frozen `add_custom.cpp` and tiling header into bounded work
storage, selects CMake's ASC language with `--npu-arch=dav-3510`, and requires `bisheng` compilation
plus ASC static-library linkage. The receipt must report the exact success line and trusted
`docker:accelerator:none` evidence. This proves the real target toolchain without claiming target
device correctness or compiling a generated reduction candidate.

## F2 boundary

F2 is complete when a worker can receive verified materials, durably start a real Docker job,
publish its bounded result, and recover the same attempt after restart. Service managers,
dynamic shared-device selection, richer network policies, and stronger container isolation are
separate product slices justified by actual migration needs.
