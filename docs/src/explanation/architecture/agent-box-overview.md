# Agent-box architecture overview

Agent-box orchestrates safe, disposable execution environments for autonomous coding agents.

## Core responsibilities

- Discover and resolve repositories (by default from any location on the filesystem; optionally restricted to `base_repo_dir`)
- Create workspaces (JJ or Git)
- Build container runtime config from layered settings + profiles + CLI overrides
- Spawn containers with deterministic mounts, env, networking, and entrypoint

## Layered configuration model

- Global config defines defaults.
- Repo-local config refines project-specific behavior.
- Profiles provide composable bundles (mounts/env/ports/hosts/context).

## Runtime abstraction

Agent-box supports Docker and Podman backends through runtime-specific implementation while preserving one CLI surface.

## Relationship to Portal

Agent-box can mount Portal socket and export `AGENT_PORTAL_SOCKET`, but Portal remains optional and independently operable.
