#!/usr/bin/env bash
# Scan the repository for secrets with gitleaks.
#
#   scripts/scan-secrets.sh          # full git history (what CI runs)
#   scripts/scan-secrets.sh staged   # staged changes only (pre-commit hook)
#
# Install gitleaks first: https://github.com/gitleaks/gitleaks#installing
set -euo pipefail
cd "$(dirname "$0")/.."

if ! command -v gitleaks >/dev/null 2>&1; then
  echo "gitleaks not found. Install it from https://github.com/gitleaks/gitleaks" >&2
  exit 1
fi

mode="${1:-history}"
case "$mode" in
  staged)
    echo "== scanning staged changes =="
    gitleaks protect --staged --redact --verbose --config .gitleaks.toml
    ;;
  history)
    echo "== scanning full git history =="
    gitleaks detect --source . --redact --verbose --config .gitleaks.toml
    echo "== scanning working tree (build output excluded) =="
    tmp="$(mktemp -d)"
    trap 'rm -rf "$tmp"' EXIT
    rsync -a --exclude node_modules --exclude target --exclude dist --exclude data \
      --exclude src-tauri/binaries --exclude .git --exclude .env ./ "$tmp"/
    gitleaks detect --source "$tmp" --no-git --redact --verbose --config .gitleaks.toml
    ;;
  *)
    echo "usage: $0 [history|staged]" >&2
    exit 2
    ;;
esac
