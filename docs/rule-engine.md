# Rule Engine

## Rule shape
```json
{
  "id": "rule-1",
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

## Actions
- `rewrite_request_headers`
- `rewrite_response_headers`
- `mock_response`
- `delay`

## Matching
- `enabled = false` rules are skipped.
- Rules are applied by ascending `priority`.
- Mock responses short-circuit upstream forwarding.

## UI
- Rules are edited as JSON.
- Save applies immediately.
