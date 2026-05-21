# Demo Script

## 1. Start the app
Launch the desktop app and start the proxy on port `8899`.

## 2. Capture HTTP traffic
```bash
curl -x http://127.0.0.1:8899 http://example.com
curl -x http://127.0.0.1:8899 -X POST http://example.com/post -d "hello=world"
```

Check that the request list updates and the detail pane shows headers, bodies, and timeline data.

## 3. Capture CONNECT and HTTPS tunnel metadata
```bash
curl -x http://127.0.0.1:8899 https://example.com
```

Check that the CONNECT session is recorded with host, port, duration, and byte counts.

## 4. Enable HTTPS MITM
1. Generate the local Root CA from the toolbar.
2. Install the CA only on a device you control.
3. Toggle HTTPS decrypt on.
4. Send HTTPS traffic again.

Check that the UI shows MITM status and the decrypted session can be inspected.

## 5. Add a mock rule
Use the rule editor with this rule:
```json
{
  "id": "mock-user-api",
  "name": "Mock user API",
  "enabled": true,
  "priority": 100,
  "match": {
    "url_contains": "/api/user",
    "method": "GET"
  },
  "action": {
    "type": "mock_response",
    "status": 200,
    "headers": [
      {
        "name": "content-type",
        "value": "application/json"
      }
    ],
    "body": "{\"id\":1,\"name\":\"demo\"}"
  }
}
```

Then run:
```bash
curl -x http://127.0.0.1:8899 http://example.com/api/user
```

The response should be mocked and the session should be marked as mocked.

## 6. Add a script rule
Use a Rhai rule to rewrite a request header or inject a response header. Confirm the matched rule id appears in the detail view.

## 7. Save to a collection
Create a collection, then add a captured session from the detail panel.

## 8. Replay and export
- Click `Replay` on an HTTP session.
- Export selected sessions to JSON or HAR-like output.
- Open the exported file and confirm it contains the captured sessions.
