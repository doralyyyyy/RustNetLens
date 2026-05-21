# RustNetLens

RustNetLens is a local desktop HTTP proxy and traffic inspector built with Rust and Tauri 2.

## Core features
- Local HTTP proxy on `127.0.0.1:<port>`
- HTTPS CONNECT tunneling
- Session capture, persistence, filtering, and export
- Rule-based request/response header rewrite and mock responses
- Replay for captured HTTP sessions
- React + TypeScript desktop UI

## Rust highlights
- Tokio async runtime
- Hyper + Reqwest proxying
- SQLite persistence via `rusqlite`
- Tauri 2 desktop shell
- Event-driven capture pipeline

## Architecture
See [docs/architecture.md](docs/architecture.md).

## Start
### Frontend
```bash
cd apps/desktop
npm install
npm run dev
```

### Desktop app
```bash
cargo run -p rustnetlens-app
```

## Usage
1. Start the app.
2. Click `Start Proxy`.
3. Set your browser or curl proxy to `http://127.0.0.1:8899`.
4. Send HTTP traffic through the proxy.
5. Inspect captured sessions in the table and detail panel.

## Demo commands
```bash
curl -x http://127.0.0.1:8899 http://example.com
curl -x http://127.0.0.1:8899 -X POST http://example.com/post -d "hello=world"
curl -x http://127.0.0.1:8899 http://example.com/api/user
```

## Rules
See [docs/rule-engine.md](docs/rule-engine.md).

## Tests
```bash
cargo test
```

## Safety
- This tool is for local, authorized debugging only.
- HTTPS decryption is not enabled in MVP.
- Sensitive headers are redacted by default.

## Roadmap
- WebSocket capture
- HAR export
- Timeline breakdown
- Compression handling
- HTTPS MITM for local authorized debugging only
