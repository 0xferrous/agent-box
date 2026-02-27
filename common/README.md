# agent-box-common

Shared library crate used by `ab`, `agent-portal`, and `agent-wrappers`.

## Modules

- `config`: layered config loading/validation
- `repo`: repo/workspace resolution helpers
- `portal`: portal protocol + policy/config types
- `portal_client`: reusable client for portal socket calls
- `path`, `display`: path/display utilities

## Development

From repo root:

```bash
cargo test -p agent-box-common
cargo clippy -p agent-box-common --all-targets -- -D warnings
```
