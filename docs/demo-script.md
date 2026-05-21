# Demo Script

1. Launch the desktop app.
2. Click `Start Proxy` on port `8899`.
3. Set browser or curl proxy to `http://127.0.0.1:8899`.
4. Send HTTP traffic:
```bash
curl -x http://127.0.0.1:8899 http://example.com
curl -x http://127.0.0.1:8899 -X POST http://example.com/post -d "hello=world"
```
5. Open a session and inspect headers, bodies, and raw JSON.
6. Add a mock rule for `/api/user`.
7. Replay a session.
8. Export selected sessions.
