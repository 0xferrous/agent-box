# ab

`ab` is the main Agent Box CLI.

## Purpose

- Manage repository workspaces (git/jj)
- Spawn containerized agent sessions
- Mount context/config/runtime resources for agents

## Binary

- `ab` (`ab/src/main.rs`)

## Development

From repo root:

```bash
cargo run -p ab -- --help
cargo test -p ab
cargo clippy -p ab --all-targets -- -D warnings
```

## Notes

`ab` keeps spawn/runtime orchestration in `ab/src/runtime` and uses `agent-box-common` for shared config/repo/portal functionality.
