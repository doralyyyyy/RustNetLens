# Performance Report

## Environment
- Local Windows desktop build
- Rust stable
- Tauri 2 + React frontend

## Validation
- `cargo fmt --all` passes
- `cargo check --offline` passes
- `cargo test --offline` passes
- `npm run build` passes
- `cargo test -p rustnetlens-core proxy_smoke_perf -- --ignored --nocapture` passes

## Smoke Benchmark
Run:
```bash
cargo test -p rustnetlens-core proxy_smoke_perf -- --ignored --nocapture
```

Observed on this machine:
- 100 local HTTP proxy requests
- total: 3850.95 ms
- average latency: 38.51 ms
- throughput: 25.97 req/s

## Notes
- Results depend on local machine and upstream server.
- HTTPS MITM is optional and disabled by default.
- The benchmark covers the current hyper-based forwarding path.
