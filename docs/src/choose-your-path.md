# Choose your path

Use this quick routing guide to pick the right docs entry point.

## I want sandboxed local agent sessions

Use **Agent-box**.

Start with: [Tutorial: Agent-box first run](tutorials/agent-box/first-run.md)

## I want host capability mediation, but no container orchestrator

Use **Portal standalone**.

Start with: [Tutorial: Portal standalone first run](tutorials/portal/first-run-standalone.md)

## I want Agent-box with Portal-enabled wrappers

Use **both**.

Start with: [Tutorial: Connect Portal to Agent-box](tutorials/portal-with-agent-box/connect-portal-to-agent-box.md)

## What’s the difference?

- Agent-box manages repositories, workspaces, and containers.
- Portal brokers selected host operations over a Unix socket with policy controls.
- Wrappers (for example `wl-paste`, `gh`) provide transparent, tool-compatible access to Portal methods.
