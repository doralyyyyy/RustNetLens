# Architecture

```text
React UI
  -> Tauri commands
    -> AppState
      -> SqliteStore
      -> RuleEngine (Rhai + header rewrite + mock + delay)
      -> ProxyServer
        -> HTTP forwarding via hyper + hyper-rustls
        -> CONNECT tunneling via TCP
        -> Optional local-only HTTPS MITM
        -> WebSocket capture for ws://
        -> gRPC metadata and trailer capture
        -> capture events
      -> Event broadcast
```

## Layers
- `crates/core`: proxy, rules, persistence, export, replay, security, and data model.
- `apps/desktop/src-tauri`: Tauri state and commands.
- `apps/desktop/src`: React UI and event-driven views.

## Runtime Flow
1. Proxy receives a request.
2. Request preview is captured and redacted.
3. Rules are applied in priority order.
4. Request is mocked, rewritten, delayed, or forwarded upstream.
5. Response is captured, redacted, and stored.
6. UI receives summary events and reloads detail on demand.
7. Collections can be created from saved session snapshots.

## Storage
- SQLite stores session summaries, details, rules, and collections.
- Export files are written under the app data export directory.
- HTTPS MITM generates a local Root CA under the app data directory.

## Security Model
- HTTPS MITM is off by default.
- The local Root CA is generated explicitly by the user.
- The UI shows install hints and current MITM state.
- Sensitive headers are redacted in previews by default.
