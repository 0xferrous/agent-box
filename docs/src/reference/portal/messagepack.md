# Portal MessagePack type reference

Portal uses [MessagePack](https://msgpack.org) encoding over a Unix domain socket.

This page documents request/response pairs by method.

## Envelopes

### Request envelope

```text
PortalRequest {
  version: u16,
  id: u64,
  method: RequestMethod (flattened)
}
```

- `version`: protocol version (currently `1`)
- `id`: client-generated request ID
- `method`: serde-tagged enum (`method` + optional `params`)

### Response envelope

```text
PortalResponse {
  version: u16,
  id: u64,
  ok: bool,
  result: ResponseResult | null,
  error: PortalError | null
}
```

- `id` is echoed from request
- if `ok = true`, `result` is set
- if `ok = false`, `error` is set

### Error object

```text
PortalError {
  code: string,
  message: string
}
```

Common `code` values: `denied`, `prompt_failed`, `rate_limited`, `clipboard_failed`, `gh_exec_failed`, `too_busy`.

## Method pairs

### `ping`

Request:

```text
{ "method": "ping" }
```

Success response (`Pong`):

```text
{
  "type": "Pong",
  "data": {
    "now_unix_ms": u128
  }
}
```

### `whoami`

Request:

```text
{ "method": "whoami" }
```

Success response (`WhoAmI`):

```text
{
  "type": "WhoAmI",
  "data": {
    "pid": i32,
    "uid": u32,
    "gid": u32,
    "container_id": string | null
  }
}
```

### `clipboard.read_image`

Request:

```text
{
  "method": "clipboard.read_image",
  "params": {
    "reason": string | null
  }
}
```

Success response (`ClipboardImage`):

```text
{
  "type": "ClipboardImage",
  "data": {
    "mime": string,
    "bytes": binary
  }
}
```

Common error codes for this method:

- `denied`
- `prompt_failed`
- `clipboard_failed`
- `rate_limited`
- `too_busy`

### `gh.exec`

Request:

```text
{
  "method": "gh.exec",
  "params": {
    "argv": [string, ...],
    "reason": string | null,
    "require_approval": bool
  }
}
```

Success response (`GhExec`):

```text
{
  "type": "GhExec",
  "data": {
    "exit_code": i32,
    "stdout": binary,
    "stderr": binary
  }
}
```

Common error codes for this method:

- `denied`
- `prompt_failed`
- `gh_exec_failed`
- `rate_limited`
- `too_busy`

### `exec`

Request:

```text
{
  "method": "exec",
  "params": {
    "argv": [string, ...],
    "reason": string | null,
    "cwd": string | null,
    "env": { string: string, ... } | null
  }
}
```

Success response (`Exec`):

```text
{
  "type": "Exec",
  "data": {
    "exit_code": i32,
    "stdout": binary,
    "stderr": binary
  }
}
```

Common error codes for this method:

- `denied`
- `prompt_failed`
- `exec_failed`
- `rate_limited`
- `too_busy`

## Serialization notes

- Request enum tags: `method` + `params`
- Response enum tags: `type` + `data`
- `PortalRequest.method` is flattened into request envelope
- Rust source of truth: `common/src/portal.rs`
