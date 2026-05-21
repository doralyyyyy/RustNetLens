# Performance Report

## Environment
- Local Windows desktop build
- Rust stable
- Tauri 2 + React frontend

## Validation
- `cargo check` passes
- `cargo test` passes
- `npm run build` passes

## Smoke benchmark
Run:
```bash
cargo test -p rustnetlens-core proxy_smoke_perf -- --ignored --nocapture
```

Suggested metric:
- 100 local HTTP proxy requests
- average latency
- requests per second

Observed on this machine:
- 100 requests total: 2662.39 ms
- average latency: 26.62 ms
- throughput: 37.56 req/s

## Notes
- HTTPS MITM is not enabled in MVP.
- Results depend on local machine and upstream server.
