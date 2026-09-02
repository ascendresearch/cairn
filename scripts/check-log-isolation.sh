#!/usr/bin/env bash
set -euo pipefail

# A check that cannot fail is not a check. Every tool this gate depends on must be present, or the
# gate reports its own absence instead of passing in silence.
for tool in grep awk find; do
  command -v "$tool" >/dev/null 2>&1 || {
    echo "log isolation gate cannot run: $tool is unavailable" >&2
    exit 2
  }
done

status=0

# Cairn V1 uses leaf events, not spans that can accidentally become owners of business work.
if grep -rnE \
  'tracing::(span|trace_span|debug_span|info_span|warn_span|error_span)!|#\[(tracing::)?instrument|\.instrument\(' \
  --include='*.rs' crates; then
  echo "logging isolation violation: tracing spans/instrumentation are not allowed in Cairn V1 business crates" >&2
  status=1
fi

sources=$(find crates -name '*.rs' -type f)
if [[ -z "$sources" ]]; then
  echo "log isolation gate cannot run: no Rust sources were found under crates" >&2
  exit 2
fi

# Rustfmt keeps one tracing event invocation as a line-oriented block. Reject control flow and
# operations with obvious state/external effects inside that block. Immutable typed getters and
# bounded scalar projections remain subject to ordinary review.
awk '
  /tracing::(trace|debug|info|warn|error)!\(/ { inside = 1 }
  inside && /\?[[:space:]]*,/ {
    print FILENAME ":" FNR ": fallible ? operator inside tracing event: " $0 > "/dev/stderr"
    status = 1
  }
  inside && /\.await/ {
    print FILENAME ":" FNR ": await inside tracing event: " $0 > "/dev/stderr"
    status = 1
  }
  inside && /(execute|dispatch|invoke|recover|append|put|write|send|recv|sleep|take|replace|insert|remove|ok_or|map_err|unwrap|expect)[[:space:]]*\(/ {
    print FILENAME ":" FNR ": stateful or fallible call inside tracing event: " $0 > "/dev/stderr"
    status = 1
  }
  inside && /^[[:space:]]*\)[,;][[:space:]]*$/ { inside = 0 }
  END { exit status }
' $sources || status=1

# Nothing may open a file under the log tree.
#
# `log/` carries stable identities, counts, states and failure classes, and 10.5 forbids it from
# holding any place a diagnostic body could land. Source, prompts, model bodies, stdout, stderr,
# hidden content and credentials are readable only from the store under an explicit authorization,
# and a file sink under `log/` is how that boundary gets crossed by accident rather than by
# decision. Every process writes to its supervisor's journal today and no code names this tree at
# all, so the check is that the count stays zero: adding a sink means editing this gate, which is
# the review the invariant is asking for.
log_tree_uses=$(grep -rn 'RuntimeTree::Log' --include='*.rs' crates \
  | grep -v '^crates/cairn-layout/' \
  | grep -c . || true)
if [[ "$log_tree_uses" -ne 0 ]]; then
  grep -rn 'RuntimeTree::Log' --include='*.rs' crates | grep -v '^crates/cairn-layout/' >&2
  echo "the log tree has $log_tree_uses consumer(s); a file under log/ must be a reviewed decision, not a diff" >&2
  status=1
fi

exit "$status"
