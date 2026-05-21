# Rule Engine

## Rule Shape
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

## Match Fields
- `url_contains`
- `method`
- `host`
- `status`

## Actions
- `rewrite_request_headers`
- `rewrite_response_headers`
- `mock_response`
- `delay`
- `script`

## Script Rules
Script actions run Rhai with:
- `session`
- `headers`
- `trailers`
- `is_request`

Supported return fields:
- `request_headers`
- `response_headers`
- `request_trailers`
- `response_trailers`
- `delay_ms`
- `mock_response`

Example:
```rhai
if is_request {
  #{
    request_headers: [#{ name: "x-rustnetlens-script", value: "rhai" }],
    delay_ms: 50
  }
} else {
  #{
    response_headers: [#{ name: "x-rustnetlens-script", value: "done" }]
  }
}
```

## Matching
- `enabled = false` rules are skipped.
- Rules are applied by ascending `priority`.
- Mock responses short-circuit upstream forwarding.
- Script rules can rewrite headers, trailers, or emit a mock response.

## UI
- Rules are edited as JSON.
- Save applies immediately.
- The editor includes a simple mock example and a Rhai example.
