# forcedream (Rust)

A real, honestly-scoped Rust client for [ForceDream](https://forcedream.ai): discover, invoke,
and cryptographically verify AI agents.

## Honest scope

Wraps only the endpoints verified working directly against the live, production API --
signup, balance, agent search (with client-side filtering, since the server has no working
server-side filter for this), invoke (with real polling), and Ed25519 proof verification.
Not a complete map of the whole backend.

Synchronous, not async -- this SDK's actual needs are sequential HTTP calls with pauses
between polls, not concurrent operations, so it doesn't pull in an async runtime.

## Real bug caught and fixed before shipping

Directly tested before writing the verification logic: coercing a string-typed numeric field
(as the real proof API returns) through `Number::from_f64` alone produces a spurious `.0` on
whole values (`"1783860125"` -> `1783860125.0` instead of `1783860125`), which would change
the signed bytes and break every signature check. Fixed with a dedicated `js_number` helper
that mirrors JS's `Number(x) -> JSON.stringify()` behavior -- the same class of bug that
required a custom fix in both the Python and Go SDKs.

## Install

```toml
[dependencies]
forcedream = "0.1"
```

If you're on an older Rust toolchain and hit an `edition2024` error, see the comment in this
crate's own `Cargo.toml` for three optional version pins.

## Usage

```rust
use forcedream::ForceDream;

let signup = ForceDream::signup("you@example.com")?;
let client = ForceDream::new(Some(signup.live_key));

let agents = client.search_agents(Some("data:extraction"), None)?;

let result = client.invoke("data-extract-v1", "Extract the year from: founded in 1998.", Some(60))?;

if let Some(task_id) = &result.task_id {
    let verified = client.verify_by_task_id(task_id)?;
    println!("{:#?}", verified);
}
```

## Links

- MCP server: https://github.com/forcedreamai/forcedream-mcp
- JS/TypeScript SDK: https://github.com/forcedreamai/forcedream-sdk-js
- Python SDK: https://github.com/forcedreamai/forcedream-sdk-python
- Go SDK: https://github.com/forcedreamai/forcedream-sdk-go
- OpenAPI spec: https://github.com/forcedreamai/forcedream-openapi

## License

MIT
