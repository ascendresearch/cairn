#!/usr/bin/env bash
set -euo pipefail

# End-to-end capability has been lost twice by refactors that moved ownership and left the previous
# consumers behind, while `scripts/ci.sh` stayed green because every real build and device lane is
# an explicit `--ignored` opt-in. This gate makes that class of loss visible at ordinary CI time.
#
# A check that cannot fail is not a check, so every tool this gate depends on must be present or the
# gate reports its own absence rather than passing in silence.
for tool in grep find python3; do
  command -v "$tool" >/dev/null 2>&1 || {
    echo "product path gate cannot run: $tool is unavailable" >&2
    exit 2
  }
done

status=0

# 1. Every opt-in lane a script invokes must name a test target that exists. A script calling a
#    target which was deleted is not an asset; it reads as coverage that is not there.
while IFS= read -r script; do
  target=$(grep -o -- '--test [a-z_]*' "$script" | head -1 | awk '{print $2}' || true)
  [[ -n "$target" ]] || continue
  if ! find crates -path '*/tests/*' -name "$target.rs" -type f | grep -q .; then
    echo "dangling opt-in lane: $script invokes --test $target, which does not exist" >&2
    status=1
  fi
done < <(find scripts -name '*.sh' -type f | sort)

# 2. Orphan ratchet over the domain crate's exported functions.
#
#    A product capability whose only remaining callers are tests is a capability the normal path can
#    no longer reach. The number below is a recorded baseline, not a target: it may only change
#    through a reviewed edit to this file, so both losing a consumer and adopting an orphan become
#    visible events. It is deliberately not an allowlist of names, which would grow in silence.
# 2026-09-03: 22 -> 23. `recompute_candidate_admission` lost its caller when the candidate stage
# was rebuilt around the search loop. It had no reachable caller before either: the only path to it
# ran through `observe_candidate_on_worker`, which returned `CandidateMechanismExecutionUnavailable`
# unconditionally, so admission was never reached at runtime. The reference was decoration, and the
# count now says what was already true. It regains a caller in P5, when a qualified mechanism can
# execute against a built artifact.
readonly EXPECTED_ORPHANS=23

orphans=$(python3 - <<'PY'
import pathlib
import re

lib_path = pathlib.Path("crates/cairn-migration/src/lib.rs")
lib = lib_path.read_text(encoding="utf-8")

owner = {}
for block in re.finditer(r"pub use (\w+)::\{(.*?)\};", lib, re.S):
    module = block.group(1)
    for item in block.group(2).replace("\n", " ").split(","):
        item = item.strip()
        if item and item[0].islower():
            owner[item] = f"crates/cairn-migration/src/{module}.rs"

sources = {}
for path in pathlib.Path("crates").rglob("*.rs"):
    if "/tests/" in str(path) or path == lib_path:
        continue
    sources[str(path)] = path.read_text(encoding="utf-8").split("#[cfg(test)]")[0]

for name in sorted(owner):
    defining = owner[name]
    pattern = re.compile(r"\b" + re.escape(name) + r"\b")
    if not any(
        pattern.search(body) for path, body in sources.items() if path != defining
    ):
        print(name)
PY
)

count=$(printf '%s' "$orphans" | grep -c . || true)
if [[ "$count" -ne "$EXPECTED_ORPHANS" ]]; then
  echo "exported functions with no non-test consumer outside their module: $count (recorded $EXPECTED_ORPHANS)" >&2
  printf '%s\n' "$orphans" | sed 's/^/  /' >&2
  echo "update EXPECTED_ORPHANS in scripts/check-product-path.sh only as a reviewed decision" >&2
  status=1
fi

# 3. Both the controller and the worker run synchronous SQLite and bulk staging inside `on_store`,
#    which is `tokio::task::block_in_place` and exists only on a multi-threaded runtime. On a
#    current-thread runtime the same call panics, so a flavor change would move a live failure into
#    production without any ordinary test noticing: no test covers either `main.rs`, and the two
#    integration tests that reach these paths pin their own flavor.
# cairn-migration-app joined this list on 2026-09-03: its product services now open the durable
# store inside `block_in_place` on every candidate search transition, so it carries the same
# requirement the controller and worker do.
# 4. The release manifest is the declaration and build-release.sh is its implementation, so the
#    script must read it rather than restate it. A manifest naming one set of binaries while the
#    script builds another already produced a bundle that could not deploy the host it was for.
for field in targets binaries; do
  if ! grep -q "manifest_list $field" scripts/build-release.sh; then
    echo "build-release.sh must read '$field' from release/toolchain.toml, not restate it" >&2
    status=1
  fi
done
while read -r binary; do
  [[ -n "$binary" ]] || continue
  if ! grep -qw -- "$binary" deploy/systemd/*.template scripts/deploy.sh; then
    echo "release manifest ships $binary, which no deployment installs" >&2
    status=1
  fi
done < <(sed -n 's/^binaries = \[\(.*\)\]$/\1/p' release/toolchain.toml | tr -d '"' | tr ',' '\n' | tr -d ' ')

for crate in cairn-server cairn-worker cairn-migration-app; do
  if grep -rn 'current_thread' "crates/$crate"; then
    echo "$crate must stay on a multi-threaded runtime: it calls block_in_place" >&2
    status=1
  fi
done

exit "$status"
