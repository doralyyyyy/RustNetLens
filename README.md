# RustNetLens

RustNetLens is a local desktop network debugging tool built with Rust, Tauri 2, and React.

## Project Summary
- Local HTTP proxy on `127.0.0.1:<port>`
- HTTPS CONNECT tunneling
- Optional local-only HTTPS MITM for authorized debugging
- Session capture, filtering, persistence, and export
- Rule-based header rewrite, mock responses, delay, and Rhai scripts
- Request collections
- gRPC metadata and trailer capture
- WebSocket frame capture
- Replay for captured HTTP sessions

## Rust Highlights
- Tokio async runtime
- Hyper and hyper-rustls proxying
- SQLite persistence via `rusqlite`
- Tauri 2 desktop shell
- Event-driven capture pipeline
- Rhai-based script rules
- Local Root CA generation for HTTPS MITM

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
6. Create collections from captured sessions if you want to save request sets.
7. Generate the local Root CA before enabling HTTPS decrypt.
8. Enable HTTPS decrypt only on devices you control.

## HTTPS MITM
- HTTPS decrypt is disabled by default.
- The app can generate a local Root CA under the user app data directory.
- Install that CA only on machines you own or control.
- Sensitive headers are redacted by default.
- CONNECT tunneling still works when decrypt is off.

## Demo Commands
```bash
curl -x http://127.0.0.1:8899 http://example.com
curl -x http://127.0.0.1:8899 -X POST http://example.com/post -d "hello=world"
curl -x http://127.0.0.1:8899 https://example.com
curl -x http://127.0.0.1:8899 http://example.com/api/user
```

## Rules
See [docs/rule-engine.md](docs/rule-engine.md).

## Tests
```bash
cargo test
cd apps/desktop
npm run build
```

## Safety
- This tool is for local, authorized debugging only.
- HTTPS decryption is optional, local-only, and disabled by default.
- Sensitive headers are redacted by default.
- Exported content is intended to stay redacted unless you explicitly choose otherwise.

## Roadmap
- More polished gRPC presentation
- Compression-aware previews
- Smarter rule authoring UI
- Better collection management and bulk replay
