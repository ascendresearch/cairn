#!/usr/bin/env bash
set -euo pipefail

cargo fmt --all --check
bash scripts/check-log-isolation.sh
cargo check --workspace --locked
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-targets --all-features
cargo test --workspace --doc --all-features

status=0
for file in README.md docs/*.md; do
  while IFS= read -r link; do
    target="${link%%#*}"
    case "$target" in
      ""|http://*|https://*|mailto:*) continue ;;
    esac
    base=$(dirname "$file")
    if [[ ! -e "$base/$target" ]]; then
      echo "missing link: $file -> $link" >&2
      status=1
    fi
  done < <(sed -n 's/.*](\([^)]*\)).*/\1/p' "$file")
done

if rg -n '[[:blank:]]+$' README.md LICENSE .github config model-templates docs crates scripts release Cargo.toml rustfmt.toml rust-toolchain.toml; then
  status=1
fi

exit "$status"
