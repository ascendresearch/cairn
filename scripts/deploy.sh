#!/usr/bin/env bash
set -euo pipefail

# Places one verified release bundle on one host and puts it under systemd.
#
# The bundle is the unit of deployment because it is the only artefact whose provenance has been
# checked: `scripts/build-release.sh` verifies each binary's machine, link mode, libc coupling and
# dynamic dependencies, records them in BUILD-METADATA.json, and the release workflow rebuilds and
# compares byte for byte. Copying a loose binary out of `target/` skips all of that, and doing so
# once already put a build worker on a host whose libc could not load it.
#
# Versions are immutable directories and `current` is a symlink, so a rollback is a symlink flip
# rather than another transfer, and the unit file never has to change to move between versions.

usage() {
  cat >&2 <<'USAGE'
usage: scripts/deploy.sh <bundle.tar.gz> <controller|worker> <instance>

Required environment:
  CAIRN_DEPLOY_HOST     "local", or an ssh destination such as root@host
  CAIRN_DEPLOY_PREFIX   absolute directory on the target that will hold versions/ and current
  CAIRN_DEPLOY_CONFIG   absolute path to the process configuration on the target
  CAIRN_DEPLOY_WORKDIR  absolute working directory for the unit

Optional environment:
  CAIRN_DEPLOY_SCOPE    system | user            (default: system)
  CAIRN_DEPLOY_STATE    absolute state directory (controller only; the unit is granted write
                        access to exactly this path and the rest of the filesystem stays read-only)
USAGE
  exit 2
}

for tool in tar sha256sum sed basename dirname; do
  command -v "$tool" >/dev/null 2>&1 || {
    echo "deploy cannot run: $tool is unavailable" >&2
    exit 2
  }
done

(($# == 3)) || usage
readonly BUNDLE="$1"
readonly ROLE="$2"
readonly INSTANCE="$3"

readonly HOST="${CAIRN_DEPLOY_HOST:?CAIRN_DEPLOY_HOST is required}"
readonly PREFIX="${CAIRN_DEPLOY_PREFIX:?CAIRN_DEPLOY_PREFIX is required}"
readonly CONFIG="${CAIRN_DEPLOY_CONFIG:?CAIRN_DEPLOY_CONFIG is required}"
readonly WORKDIR="${CAIRN_DEPLOY_WORKDIR:?CAIRN_DEPLOY_WORKDIR is required}"
readonly SCOPE="${CAIRN_DEPLOY_SCOPE:-system}"
readonly STATE="${CAIRN_DEPLOY_STATE:-}"

case "$ROLE" in
  controller)
    binary="cairn-server"
    template="deploy/systemd/cairn-controller.service.template"
    unit="cairn-controller-$INSTANCE.service"
    [[ -n "$STATE" ]] || { echo "CAIRN_DEPLOY_STATE is required for the controller role" >&2; exit 2; }
    ;;
  worker)
    binary="cairn-worker"
    template="deploy/systemd/cairn-worker.service.template"
    unit="cairn-worker-$INSTANCE.service"
    ;;
  *) usage ;;
esac
case "$SCOPE" in
  system) wanted_by="multi-user.target" ;;
  user) wanted_by="default.target" ;;
  *) echo "CAIRN_DEPLOY_SCOPE must be system or user, observed: $SCOPE" >&2; exit 2 ;;
esac
for path in "$PREFIX" "$CONFIG" "$WORKDIR"; do
  [[ "$path" == /* ]] || { echo "deploy paths must be absolute, observed: $path" >&2; exit 2; }
done
[[ -f "$BUNDLE" && -f "$BUNDLE.sha256" ]] || {
  echo "bundle or its checksum is missing: $BUNDLE" >&2
  exit 2
}
[[ -f "$template" ]] || { echo "missing unit template: $template" >&2; exit 2; }

# The name carries the commit the bundle was built from, and that is what names the version
# directory, so a host always reports which source produced what it is running.
bundle_name="$(basename "$BUNDLE")"
version="${bundle_name#cairn-}"
version="${version%%-*}"
[[ -n "$version" ]] || { echo "cannot derive a version from bundle name: $bundle_name" >&2; exit 2; }

(cd "$(dirname "$BUNDLE")" && sha256sum --check --strict "$bundle_name.sha256") >/dev/null

rendered="$(sed \
  -e "s|@INSTANCE@|$INSTANCE|g" \
  -e "s|@BINARY@|$PREFIX/current/bin/$binary|g" \
  -e "s|@CONFIG@|$CONFIG|g" \
  -e "s|@WORKING_DIRECTORY@|$WORKDIR|g" \
  -e "s|@STATE_DIRECTORY@|$STATE|g" \
  -e "s|@WANTED_BY@|$wanted_by|g" \
  "$template")"

on_target() {
  if [[ "$HOST" == "local" ]]; then
    bash -s
  else
    ssh -o BatchMode=yes "$HOST" bash -s
  fi
}

if [[ "$HOST" != "local" ]]; then
  scp -q -o BatchMode=yes "$BUNDLE" "$HOST:/tmp/$bundle_name"
  remote_bundle="/tmp/$bundle_name"
else
  remote_bundle="$(cd "$(dirname "$BUNDLE")" && pwd -P)/$bundle_name"
fi

on_target <<REMOTE
set -euo pipefail
for tool in tar sha256sum systemctl install ln mv; do
  command -v "\$tool" >/dev/null 2>&1 || { echo "target cannot deploy: \$tool is unavailable" >&2; exit 2; }
done

scope="$SCOPE"
unit="$unit"
if [[ "\$scope" == user ]]; then
  export XDG_RUNTIME_DIR="\${XDG_RUNTIME_DIR:-/run/user/\$(id -u)}"
  systemctl_cmd=(systemctl --user)
  unit_dir="\$HOME/.config/systemd/user"
else
  systemctl_cmd=(systemctl)
  unit_dir="/etc/systemd/system"
fi

# Two processes sharing one worker identity and one durable state directory is a corruption, not a
# race worth resolving automatically. Refuse instead, and name the process the operator has to stop.
if ! "\${systemctl_cmd[@]}" is-active --quiet "\$unit" 2>/dev/null; then
  existing="\$(pgrep -f -- "$CONFIG" || true)"
  if [[ -n "\$existing" ]]; then
    echo "an unmanaged process is already running with this configuration: \$existing" >&2
    echo "stop it before placing $CONFIG under systemd" >&2
    exit 1
  fi
fi

version_dir="$PREFIX/versions/$version"
mkdir -p "\$version_dir" "$PREFIX"
tar -xzf "$remote_bundle" -C "\$version_dir"
(cd "\$version_dir" && sha256sum --check --strict --quiet SHA256SUMS)
chmod 0755 "\$version_dir/bin/$binary"

# ln -sfn on an existing symlink is not atomic; a create-then-rename is, so a concurrent start can
# never observe the prefix without a current version.
ln -sfn "versions/$version" "$PREFIX/.current.staged"
mv -T "$PREFIX/.current.staged" "$PREFIX/current"

mkdir -p "\$unit_dir"
cat > "\$unit_dir/\$unit" <<'UNIT'
$rendered
UNIT
"\${systemctl_cmd[@]}" daemon-reload
"\${systemctl_cmd[@]}" enable "\$unit"
"\${systemctl_cmd[@]}" restart "\$unit"

for _ in \$(seq 1 30); do
  "\${systemctl_cmd[@]}" is-active --quiet "\$unit" && break
  sleep 1
done
if ! "\${systemctl_cmd[@]}" is-active --quiet "\$unit"; then
  echo "unit did not become active: \$unit" >&2
  "\${systemctl_cmd[@]}" status "\$unit" --no-pager --lines=20 >&2 || true
  exit 1
fi
echo "active \$unit running $version from $PREFIX/current"
if [[ "$HOST" != local ]]; then
  rm -f "$remote_bundle"
fi
REMOTE
