#!/usr/bin/env bash
set -euo pipefail

# Regenerate gh leaf command policy/report files in repo root.
# Uses Python generator script with gh from nix-shell.

nix-shell -p gh python3 --run 'python3 portal/scripts/gh-policy-gen.py'
