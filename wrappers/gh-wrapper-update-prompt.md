# gh wrapper update reference prompt

Use this prompt for future agent runs when GitHub CLI (`gh`) is updated and you want to refresh wrapper policy/report files.

## Original prompt

```text
i want you to create me a report for all available `gh` commands, subcommands, sub sub commands, etc. and if that command is doing a read or a write operation. use `gh` via nix-shell and use the help page to explore all possible commands/subcommands, and determine read/write operation
```

## Current project implementation notes

- Generate policy/report with:
  - `nix-shell -p gh python3 --run 'python3 portal/scripts/gh-policy-gen.py'`
- This updates:
  - `portal/gh-leaf-command-read-write-report.md`
  - `portal/gh-leaf-command-read-write-report.json`
- Rust wrapper consuming JSON policy:
  - `wrappers/src/bin/gh.rs`
