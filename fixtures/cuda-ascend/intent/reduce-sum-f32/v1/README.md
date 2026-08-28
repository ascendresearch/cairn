# reduce-sum-f32 Intent materialization V1

This public, MIT-licensed bundle is a Cairn-authored clean-room materialization of D-039. Its only
authoring specification is `docs/DECISIONS.md` at the exact commit recorded in `manifest.json`; no
Alloyport source bytes are imported.

The frozen source declares a one-dimensional binary32 reduction over one read-only input buffer,
one single-element output buffer, and one element count. The first domain is normal values or signed
zero, `1 <= N <= 256`, and `abs(x_i) <= 65536`. Empty input, subnormal values, non-finite values,
aliasing, and wider shapes remain outside this bundle's admitted scope.

`claims.json`, `public-corpus.json`, and `user-decision-controls.json` are proposal/control material,
not admission results. `restricted-partitions.public.json` exposes only categorical completeness and
a redacted private-review receipt identity; restricted case bytes and identities never enter Git.

The CUDA source is frozen for later exact-identity qualification. DEV-001 performs no CUDA build or
run, no Ascend build or NPU run, and no device-behavior claim.
