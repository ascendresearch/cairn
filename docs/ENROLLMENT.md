# Worker enrollment and managed identity

Cairn workers generate and retain their own private keys. Operators transfer one short-lived JSON
bundle, not a worker key/certificate set. The bootstrap listener is separate from the control
listener: bootstrap uses pinned server-authenticated TLS, while normal control remains strict mTLS.

## Controller configuration

Configure `enrollment_service` in `controller.json`. Its `issuer_certificate` must be the same CA
trusted by the control listener's `tls.client_ca`, and `issuer_private_key` must match that
certificate. Bootstrap handshake, diagnostic, wire-message, token TTL, and issued-credential
validity values are explicit configuration or command inputs.

Controller configuration schema V2 also gives every transitional static enrollment an explicit
strong `credential_id`; it must not be generated afresh at each startup. Pre-V2 development
configuration must be upgraded deliberately. Existing pre-V3 worker-registration facts require
the controlled development-state migration/rebuild gate described in the implementation plan.

The reference adapter currently reads an issuer certificate/key from files. The issuance boundary
is deliberately separate from the registry so an enterprise CA, offline signer, or SPIFFE adapter
can replace it without changing `EnrollmentId`, `WorkerId`, pool, or credential history semantics.

## Create a one-shot bundle

```bash
cairn-server enrollment create controller.json default 600000 worker.enrollment.json
```

This records an expiring offer before returning the bundle. The output path must not already exist;
on Unix it is created with mode `0600`. The controller event stream stores only the secret's
SHA-256 digest. Creating another bundle is the recovery path if the operator loses this file before
the worker submits a CSR.

## Enroll a worker

Transfer the single bundle through an operator-approved channel, then run on the worker:

```bash
cairn-worker enroll worker.enrollment.json /var/lib/cairn/identity
```

The worker creates the state directory with mode `0700` and stages `worker-key.pem` plus the exact
CSR before contacting the controller. The private key is `0600` and never crosses the network. On
success the directory contains:

- `worker-key.pem` — worker-local private key;
- `enrollment.csr.pem` — exact retry/recovery CSR;
- `worker.pem` — issued leaf and chain;
- `ca.pem` — pinned controller trust anchor from the bundle;
- `identity.json` — one-shot `EnrollmentId`, stable `WorkerId`, rotatable `CredentialId`,
  authenticated pool, and relative TLS paths.

No existing file with different bytes is overwritten. If the controller committed issuance but
the response was lost, rerunning the same command with the same state directory returns the exact
original credential. The same token with another CSR/key is rejected.

## Start the managed worker

Worker configuration selects the state directory rather than repeating identity files:

```json
{
  "identity": {
    "mode": "managed",
    "state_directory": "/var/lib/cairn/identity"
  }
}
```

After bootstrap, delete the transferred bundle according to local secret-handling policy. Do not
delete the staged CSR: it is non-secret recovery evidence bound to the local private key.

Worker configuration schema V3 adds mandatory positive `identity_poll_interval_ms`. A running
managed worker checks the atomic identity manifest at this interval. When rotation changes the
credential, it closes the old connection and reconnects with a fresh `WorkerIncarnationId`.

## Rotate a managed credential

Set `enrollment_service.rotation_overlap_ms` to a positive duration, or `null` to disable automatic
predecessor retirement. The controller freezes this choice at successor issuance. Create a bundle
for the exact current credential:

```bash
cairn-server credential rotate controller.json credential:... 600000 worker.rotation.json
```

Transfer it to the same worker state directory and rotate:

```bash
cairn-worker rotate worker.rotation.json /var/lib/cairn/identity
```

The command verifies that the bundle names the current `WorkerId` and `CredentialId`, then stages a
fresh key and exact CSR under `rotations/<EnrollmentId UUID>/`. The directory also retains the
predecessor manifest, issued certificate, and pinned CA. No old key or certificate is overwritten.
After validating lineage and public-key binding, the worker atomically replaces only
`identity.json`. Retrying after response or local-commit loss reuses the staged CSR and recovers the
same successor.

The running worker observes the manifest switch and reconnects with a new incarnation during the
overlap. After confirming the successor session, the rotation bundle may be deleted. If overlap is
`null`, the operator must explicitly revoke the predecessor after observing successor admission.

To roll back a bad successor before the frozen deadline, first revoke that successor at the
controller, then restore the local predecessor:

```bash
cairn-server credential revoke controller.json credential:<successor>
cairn-worker rollback /var/lib/cairn/identity
```

Revoking the successor inside the overlap cancels the predecessor's pending retirement as part of
the durable projection. The local command validates worker, pool, credential lineage, material, and
deadline before atomically restoring the predecessor manifest. Reversing this order can leave only
a temporary local fallback, so the controller action is deliberately first. Rollback after the
deadline fails closed. Destroy the revoked successor's rotation bundle after rollback; exact-CSR
replay deliberately returns the original (now revoked) issuance and is not a second rotation.

## Authority and recovery properties

- The controller, not the worker, assigns `WorkerId` and pool.
- An `EnrollmentId` is single-use but exact-CSR retries remain idempotent even after expiry.
- A fresh controller process reconstructs certificate fingerprint to `WorkerId`/pool authorization
  from the append-only registry stream.
- The issued certificate serial is the strong `CredentialId`; it is not the permanent worker ID.
- Static file enrollment remains available for externally provisioned identities during the
  transition, but managed enrollment is the normal open-source path.

## Revoke authority

The current managed registry supports three deliberately separate emergency actions:

```bash
cairn-server enrollment revoke controller.json enrollment:...
cairn-server credential revoke controller.json credential:...
cairn-server worker disable controller.json worker:...
```

Enrollment revocation invalidates only an unused bootstrap authority. Credential revocation
invalidates one issued credential. Worker disablement invalidates every managed credential for the
logical worker. These commands append facts; they do not edit or delete issuance history. A running
controller observes the shared SQLite authority safely and rejects reconnect before registration;
an observed live session is closed no later than the configured positive
`authority_poll_interval_ms` (or an earlier control message/outbox poll).

Application authorization is authoritative even when TLS accepts a certificate chain, so the
baseline does not require CRL or OCSP. Static file enrollments are not revocable through this
managed registry until the planned `import-static` transition. Worker disablement remains a
one-directional emergency action until the registry lifecycle phase adds re-enable. See
[`IMPLEMENTATION_PLAN.md`](IMPLEMENTATION_PLAN.md).
