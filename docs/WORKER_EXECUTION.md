# Worker execution and explicit activation

`cairn-worker join` deliberately generates a schema-V8 worker with `execution.mode=disabled`, the
single advertised backend `transport-only`, and unavailable/draining/zero-slot availability.
Enrollment proves identity and pool ownership; it does not prove that a host can execute jobs.

## Local-process V1 activation

The first real adapter is `local-process-v1`. It is intended for controlled host utilities and for
exercising the durable execution loop. It is not an oracle-grade hostile-code filesystem sandbox:
the create-only workspace does not hide worker credentials, CAS paths, or the rest of the host.
Untrusted candidate/oracle workloads therefore still require a later container or hardened sandbox
adapter. Scheduler policy must keep those workload contracts off this backend.

Activation is one deliberate `worker.json` edit. These fields must change together:

```json
{
  "availability": {
    "active_attempts": [],
    "available_slots": 1,
    "draining": false,
    "health": "ready"
  },
  "execution": {
    "materialized_file_byte_limit": 536870912,
    "mode": "local_process",
    "namespace": {
      "command": "/usr/bin/unshare",
      "preflight_timeout_ms": 5000
    },
    "sandbox_directory": "sandboxes",
    "supervisor_poll_interval_ms": 10
  },
  "profile": {
    "backends": ["local-process-v1"]
  },
  "schema_version": 8
}
```

The fragment shows only changed fields; retain all other generated fields. Both byte and preflight
bounds can be set to `null` to disable them. The positive supervisor polling interval is required.
All relative paths are resolved against `worker.json`.

The worker invokes the configured, operator-trusted util-linux-compatible command with the fixed
arguments `--user --map-root-user --net --`. It runs the same path with `/bin/true` during startup
and before every workload. If user/network namespaces are unavailable, the binary is incompatible,
or preflight times out, startup/execution fails closed. `local-process-v1` admits only job contracts
whose network policy is `disabled`; `dependency-fetch` needs a separate constrained adapter.

The worker validates the execution mode, advertised backend, and availability as one configuration
invariant. Merely changing `available_slots` cannot activate execution, and merely advertising the
backend cannot make a disabled executor schedulable. V1 also requires `max_concurrency=1` and
`available_slots=1`; broader concurrency waits for a deliberately sharded worker-journal design.

## Versioned material formats

`InputBundleArtifact` bytes are canonical JSON encoding an `InputBundleV1`. V1 supports only
explicit directories and complete regular files:

```json
{"entries":[{"kind":"directory","path":"bin"},{"bytes":"IyEvYmluL3NoCg","kind":"file","mode":"executable","path":"bin/run"},{"kind":"directory","path":"work"}],"schema_version":1}
```

File bytes use canonical unpadded base64. Entries are sorted by `SandboxPath`; every parent
directory is explicit. Duplicate paths, dot/parent/absolute paths, missing parents, symlinks, and
special files are not representable. Expansion creates a fresh `<sandbox>/<AttemptId>` tree with
private permissions and `create_new` semantics for every entry. A stale or duplicate attempt tree
is retained for audit and never overwritten.

`ExecutionEnvironmentArtifact` bytes are canonical JSON encoding an `ExecutionEnvironmentV1`:

```json
{"schema_version":1,"variables":[{"name":"LANG","value":"C.UTF-8"}]}
```

Names use portable process-environment syntax and are unique/sorted. Values cannot contain NUL.
The child environment is cleared before these exact variables are installed, so worker secrets and
ambient deployment settings are not inherited accidentally.

## Supervision and evidence boundary

The adapter executes argv without a shell on a blocking supervisor task while the async control
session continues heartbeats, starts a new process group, captures stdout/stderr into separate
supervisor-owned files, enforces the contract timeout and stream bounds while polling, and kills the
process group on timeout or overflow. Declared outputs are reopened with
`symlink_metadata`, accepted only as regular files, and independently bounded. Evidence records the
environment content identity, SHA-256 of the executable before launch, backend identity, namespace
mode, and supervision mode.

The durable start fact is still committed before materialization or process spawn. A deterministic
preflight/materialization/spawn failure is reported as `NotStarted`; after a successful spawn, an
unrecoverable supervision failure is `Ambiguous`. Timeout, subject failure, capture integrity
violation, and success are terminal captures. A crash after the start fact remains in doubt and
never reconstructs a second executor token.

Executor invocation and journal publication are separate one-shot phases. The blocking task returns
an opaque observation still owning the consumed authority; the control task reloads intervening
delivery/acknowledgement facts and serially appends the terminal result. This avoids concurrent
journal writers without freezing worker liveness during a long command. The observation channel is
owned by the worker process rather than one WebSocket session, so a reconnect or credential cutover
does not discard a still-running supervisor's terminal observation.
