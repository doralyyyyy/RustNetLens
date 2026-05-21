# Architecture

```text
React UI
  -> Tauri commands
    -> AppState
      -> RuleEngine
      -> SqliteStore
      -> ProxyServer
        -> HTTP forwarding via reqwest
        -> CONNECT tunneling via TCP
        -> capture events
      -> Event broadcast
```

## Layers
- `crates/core`: proxy, rules, persistence, export, replay, and data model.
- `apps/desktop/src-tauri`: Tauri state and commands.
- `apps/desktop/src`: React UI and event-driven views.

## Data flow
1. Proxy receives request.
2. Request preview is captured and redacted.
3. Rules are applied.
4. Request is mocked or forwarded upstream.
5. Response is captured, redacted, and stored.
6. UI receives summary events and reloads detail on demand.

## Storage
- SQLite stores session summaries, details, and rules.
- Export files are written under app data exports.
