# Agent-box

Agent-box provides sandboxed development workflows for coding agents, plus optional Portal-based host capability mediation.

> This repository now uses the mdBook docs site as the primary documentation source.

## Demo

![Agent-box demo](https://github.com/user-attachments/assets/c7aaaf16-fbcc-4669-97f3-33c423f2ff90)

## Documentation

Read the docs in one of these ways:

- Build locally: `nix develop --command 'mdbook build docs'`
- Open generated site: `docs/book/index.html`

Entry points:

- **Start here:** `docs/src/index.md`
- **Choose your path:** `docs/src/choose-your-path.md`
- **Tutorials:** `docs/src/tutorials/index.md`
- **How-to guides:** `docs/src/how-to/index.md`
- **Reference:** `docs/src/reference/index.md`
- **Explanation + ADRs:** `docs/src/explanation/index.md`

## Table of Contents

- [Demo](#demo)
- [Documentation](#documentation)
- [Quick links](#quick-links)
- [Development](#development)

## Quick links

- Agent-box first run: `docs/src/tutorials/agent-box/first-run.md`
- Agent-box profiles guide: `docs/src/how-to/agent-box/use-profiles.md`
- Portal standalone first run: `docs/src/tutorials/portal/first-run-standalone.md`
- Connect Portal to Agent-box: `docs/src/tutorials/portal-with-agent-box/connect-portal-to-agent-box.md`
- Agent-box config reference: `docs/src/reference/agent-box/config.md`
- Agent-box requirements: `docs/src/reference/agent-box/requirements.md`
- Agent-box workflow internals: `docs/src/explanation/architecture/agent-box-workflow.md`
- Agent-box CLI reference (generated): `docs/src/reference/agent-box/cli.md`
- Portal CLI reference (generated): `docs/src/reference/portal/cli.md`

## Development

From the repo root, run checks in the flake devshell:

```bash
nix develop --command cargo fmt --all
nix develop --command cargo check --workspace
nix develop --command cargo clippy --workspace --all-targets -- -D warnings
```

Regenerate CLI reference pages:

```bash
nix develop --command nix-shell -p nushell --run 'nu docs/scripts/generate-cli-reference.nu'
```
