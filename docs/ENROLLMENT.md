# Worker enrollment and managed identity

Cairn workers generate and retain their own private keys. Operators transfer one short-lived JSON
bundle, not a worker key/certificate set. The bootstrap listener is separate from the control
listener: bootstrap uses pinned server-authenticated TLS, while normal control remains strict mTLS.

## Controller configuration

Configure `enrollment_service` in `controller.json`. Its `issuer_certificate` must be the same CA
trusted by the control listener's `tls.client_ca`, and `issuer_private_key` must match that
certificate. Bootstrap handshake, diagnostic, wire-message, token TTL, and issued-credential
validity values are explicit configuration or command inputs.

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

## Authority and recovery properties

- The controller, not the worker, assigns `WorkerId` and pool.
- An `EnrollmentId` is single-use but exact-CSR retries remain idempotent even after expiry.
- A fresh controller process reconstructs certificate fingerprint to `WorkerId`/pool authorization
  from the append-only registry stream.
- The issued certificate serial is the strong `CredentialId`; it is not the permanent worker ID.
- Static file enrollment remains available for externally provisioned identities during the
  transition, but managed enrollment is the normal open-source path.

Credential rotation, revocation, offline CSR exchange, and external issuer adapters are later
lifecycle slices. They will reuse the same stable worker and credential identity separation.
