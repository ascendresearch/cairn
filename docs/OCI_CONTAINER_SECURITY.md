# OCI container security boundary

- Status: F2d-a typed contract implemented; launcher and runtime mutation not implemented
- Backend claim: `oci-container-v1`
- Scope: untrusted CPU-only candidate and oracle processes

This document freezes the security boundary before Cairn constructs a Docker-compatible command.
The current code validates identities, lifecycle observations, and OCI environment bytes. It does
not yet activate a container executor and must not be treated as an isolation claim.

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
  binding and the absent phase cannot invent them;
- `OciExecutionEnvironmentV1`: strict canonical JSON containing one image digest and a canonical
  environment-variable set;
- `ContainerRuntime`: the initial read-only resolution/inspection port returning typed observations
  rather than Docker/Podman output.

The OCI environment bytes occupy the existing typed `ExecutionEnvironmentArtifact` content domain.
The job's backend determines which strict decoder is entitled to interpret those bytes. There is no
fallback from OCI format to local-process format.

## Fixed policy target

The next implementation step must produce one code-owned launch plan, not a list of operator
checkboxes. The plan will require a read-only image root, non-root subject, denied network, all
capabilities dropped, `no-new-privileges`, independent PID/mount/IPC/user/network namespaces,
bounded PIDs/CPU/memory, bounded writable work/output/tmpfs mounts, and a read-only input mount.
Worker state, host paths, runtime state, credentials, and devices are never launch-plan inputs.

Operator configuration may select a trusted runtime executable and state roots and may set or
disable documented numeric budgets where safe. It cannot add mounts, capabilities, devices,
namespace sharing, or network access to `cpu-untrusted-v1`.

## Recovery boundary

One `AttemptId` has one deterministic `ContainerName`. Before Cairn reattaches to a present
container, every immutable `ContainerBinding` field must match. A name collision with different
labels is hostile/conflicting state: Cairn does not start, delete, rename, or reuse it.

The intended lifecycle is:

```text
Absent → Created → Running → Exited
```

An unavailable runtime before creation is `NotStarted`. Uncertainty after create or start is
`Ambiguous` until inspection of the exact name, full runtime ID, and binding proves a phase. A
worker restart may recover the same container; it must never create a second subject for an already
started `AttemptId`. Cleanup becomes eligible only after terminal evidence is durable and cleanup
failure cannot authorize re-execution.

## Acceptance boundary

F2d is not complete until offline fake-runtime tests cover fixed launch arguments, phase recovery,
binding conflicts, ambiguous mutation, bounded capture, and cleanup ordering, and opt-in real-host
tests prove filesystem/network/device isolation on both release architectures. Until then,
`cairn-worker` must not advertise or activate `oci-container-v1`.
