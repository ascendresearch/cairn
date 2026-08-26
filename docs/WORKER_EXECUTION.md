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

## Real Hello World gate

Use any already-pulled image whose full ID is available locally:

```bash
docker image inspect --format '{{.Id}}' postgres:16-alpine
scripts/docker-hello-smoke.sh sha256:<64-hex-image-id>
```

The smoke test runs an executable from the content-addressed input bundle, checks `hello world` on
stdout and a declared output artifact, then asks the executor for the same exited attempt again and
requires a byte-identical terminal capture. Ordinary CI leaves this host-dependent test ignored.

## F2 boundary

F2 is complete when a worker can receive verified materials, durably start a real Docker job,
publish its bounded result, and recover the same attempt after restart. Service managers,
multi-worker orchestration, accelerator device exposure, richer network policies, and stronger
container isolation are separate product slices justified by actual migration needs.
