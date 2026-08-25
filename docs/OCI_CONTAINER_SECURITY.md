# OCI container security boundary

- Status: F2d-d bounded capture/evidence supervisor implemented; no concrete runtime adapter or worker activation
- Backend claim: `oci-container-v1`
- Scope: untrusted CPU-only candidate and oracle processes

This document freezes the security boundary before Cairn invokes a Docker-compatible runtime. The
current code validates identities, lifecycle and bounded terminal observations, and OCI environment
bytes, renders a canonical create argv without a shell, and reconciles lifecycle/capture operations
through generic typed runtime capabilities. It has no concrete Docker-compatible adapter and does
not activate a container executor, so it must not yet be treated as an operational isolation claim.

## Threat model

The subject may control every byte in its input bundle, executable, arguments, environment values,
stdout, stderr, declared outputs, and the contents of its selected image. It may attempt filesystem
escape, symlink and special-file substitution, fork or memory exhaustion, network access, runtime
API access, credential discovery, output forgery, container-name collision, and execution replay.

The trusted computing base includes the Cairn worker/supervisor, its immutable configuration, the
configured container runtime and daemon, the host kernel, canonical identity/schema code, worker
CAS/journal, and evidence ingestion code. `oci-container-v1` does not defend against a compromised
kernel, runtime, daemon, worker binary, or operator account.

The subject must not be able to observe or modify:

- worker TLS private keys, certificates, enrollment/rotation state, or configuration;
- worker/controller SQLite databases, content stores, transfer staging, or trusted evidence;
- the runtime socket or runtime state directory;
- unrelated host files, processes, IPC objects, devices, or network interfaces;
- another attempt's input, writable workspace, outputs, temporary files, or container lifecycle.

F2d admits no accelerator, GPU, NPU, generic device, privileged, host-network, host-PID, or arbitrary
mount path. Device-aware containers are a separate F2e design and cannot be enabled by weakening
this policy.

## Frozen typed contract

`cairn-execution` now owns provider-neutral types for:

- `OciImageDigest`: exactly `sha256:<64 lowercase hex>`; mutable tags are unrepresentable;
- `ContainerName`: derived only as `cairn-attempt-<AttemptId UUID>`;
- `RuntimeContainerId`: one full 64-character lowercase runtime identity, never a short prefix;
- `ContainerPhase`: `absent`, `created`, `running`, or `exited`;
- `ContainerMountRole`: input, work, output, or temporary;
- `ContainerSandboxPolicy::cpu-untrusted-v1`: a code-owned policy identity;
- `ContainerBinding`: exact attempt, job, contract, input, environment, and policy identities;
- `ContainerInspection`: a tagged state in which present phases cannot omit runtime identity or
  binding, exited state cannot omit its typed exit code, and the absent phase cannot invent them;
- `ContainerExitObservation`: an exact exited identity requiring name, full runtime ID, complete
  binding, and typed exit code rather than permitting a nonterminal phase;
- `ContainerWaitPolicy` and `ContainerWaitOutcome`: total runtime-relative timeout plus independent
  stream bounds, producing either the exact exit or one typed stop requirement without resetting
  the deadline on recovery;
- `BoundedContainerBytes`, `ContainerOutputObservation`, and `ContainerStream`: bounded retained
  prefixes, explicit overrun, regular-file classification, and independently drained streams;
- `ContainerTerminalEvidence`: runtime-owned exact exit, observed image, program identity, elapsed
  time, and optional forced-stop reason outside candidate-writable mounts;
- `ResolvedContainerImage`: exact local immutable image identity plus a typed observation of
  image-declared volumes, which the CPU policy rejects before create;
- `OciExecutionEnvironmentV1`: strict canonical JSON containing one image digest and a canonical
  environment-variable set;
- `ContainerRuntime`: the read-only resolution/inspection port returning typed observations rather
  than Docker/Podman output;
- `ContainerLifecycleRuntime`: the minimal create/start capability parameterized by the
  backend-owned launch-plan type;
- `ContainerCaptureRuntime`: bounded wait, exact-ID stop, independent stream capture, declared-path
  capture, and terminal evidence. It intentionally exposes no remove/cleanup operation. Definite
  mutation errors prove no effect, while ambiguous errors may have applied and can be decided only
  by later exact inspection.

The OCI environment bytes occupy the existing typed `ExecutionEnvironmentArtifact` content domain.
The job's backend determines which strict decoder is entitled to interpret those bytes. There is no
fallback from OCI format to local-process format.

## Fixed launch policy

`cairn-worker` now derives one `ContainerLaunchPlan` from the exact canonical contract,
worker-verified material identities and bytes, a backend-owned absolute state root, and strong
positive resource ceilings. The plan is data only: it contains no runtime executable and grants no
create, start, stop, or delete authority. Its Docker-compatible renderer emits one argv vector and
never constructs a shell command.

The fixed argv requires a read-only image root, numeric non-root subject `65532:65532`, denied
network, all capabilities dropped, `no-new-privileges`, private cgroup/PID/IPC namespaces, bounded
PIDs/CPU/memory-and-swap, and no health check or restart policy. The only host bind is the exact
attempt input directory mounted read-only at `/cairn/input`. `/cairn/work`, `/cairn/output`, and
`/tmp` are separate size-bounded tmpfs mounts owned by the subject; output and temporary mounts are
also `noexec`. The working directory is exactly `/cairn/work`, declared outputs must remain below
`/cairn/output`, and the contract program replaces any image-defined entrypoint. The image is
addressed only by its immutable digest with pulling disabled.

The plan rejects non-OCI backends, enabled network, every accelerator/device request, an
insufficient configured CPU/memory/work/output ceiling, reserved input paths, a non-executable
program, identity mismatch, and unsafe state-root syntax. Worker credentials, journal, CAS,
SQLite, runtime socket, runtime state, unrelated host paths, arbitrary mounts, devices, and
privilege switches cannot be supplied as plan fields.

Operator configuration in later slices may select a trusted runtime executable and backend-owned
state root and may set documented numeric ceilings. It cannot add mounts, capabilities, devices,
namespace sharing, or network access to `cpu-untrusted-v1`. Numeric ceilings used for one accepted
job cannot be disabled and must satisfy the contract minima. Because a Docker CLI flag cannot force
a daemon into a private user namespace, activation must later preflight a rootless runtime or
daemon-level user-namespace remapping; the plan never emits `--userns=host`. Image-declared volumes
are now rejected before create. Activation must later stage the read-only input tree with
permissions readable by the remapped non-root subject without making worker-owned state visible,
and concrete runtime preflight must reject any observed policy weakening.

## Recovery boundary

One `AttemptId` has one deterministic `ContainerName`. Before Cairn reattaches to a present
container, its full runtime ID and every immutable `ContainerBinding` field must match. A name
collision with different labels is hostile/conflicting state: Cairn does not start, delete, rename,
or reuse it.

The intended lifecycle is:

```text
Absent → Created → Running → Exited
```

`start_container_supervision` is the sole initial entry; `recover_container_supervision` is the
reconciliation-only entry after uncertainty or restart. Both inspect the deterministic name before
mutation and after every successful create/start. Initial runtime/preflight or definitive create
failure is `NotStarted` only while a second inspection still proves the name absent. Recovery never
returns `NotStarted`.

An unknown create/start response is immediately `Ambiguous`; a later recovery inspects rather than
reconstructing job authority. Absent recovery may create the same deterministic name. `Created`
recovery may reissue start against the same full runtime ID, which converges on one runtime subject;
it never starts an `Exited` container. Running recovery waits for the same ID, and exited recovery
is purely observational. A definitive create race is re-inspected before classification, so a
matching winner is reused and a conflicting winner fails closed.

Bounded wait evaluates the contract timeout from the runtime-observed original start, not from each
API call. Deadline or independent stream exhaustion requests stop only for the matching full ID;
success, rejection, and ambiguous response are all followed by exact inspection. Capture begins
only from `Exited`, verifies terminal evidence against the immutable plan, returns no more than each
stream/output bound, and marks missing, symlink/special/directory, or over-limit declared output as
an integrity violation. Runtime image, program, elapsed time, exit, fixed policy, termination reason,
and full ID are trusted only because the adapter obtains them outside writable mounts.

There is no cleanup method in the lifecycle/capture ports. A future integration may mint separate
cleanup authority only after the exact terminal worker observation is durable. Cleanup failure must
retain evidence and cannot authorize re-execution.

## Acceptance boundary

F2d-d ordinary tests cover the exact golden create argv, policy-downgrade rejection, lifecycle
races, completion during disconnect, exited replay, stop ambiguity before/after effect, deadline
preservation across recovery, independent bounded streams, missing/non-regular/oversized output,
runtime-image conflict, evidence bounds, and the absence of cleanup from pre-publication ports.
F2d is not complete until a concrete adapter and opt-in real-host tests prove
filesystem/network/device/resource isolation on both release architectures. Until then,
`cairn-worker` must not advertise or activate `oci-container-v1`.
