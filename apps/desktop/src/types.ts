export type SessionKind = "Http" | "ConnectTunnel" | "WebSocket";
export type SessionState = "Pending" | "Completed" | "Failed" | "Mocked" | "Tunneling";

export type HeaderPair = {
  name: string;
  value: string;
};

export type BodyPreview = {
  content_type?: string | null;
  truncated: boolean;
  size: number;
  encoding?: string | null;
  pretty?: string | null;
  text?: string | null;
  base64?: string | null;
};

export type TrailersPreview = {
  headers: HeaderPair[];
};

export type TimelineEntry = {
  name: string;
  started_at: string;
  ended_at?: string | null;
  duration_ms?: number | null;
};

export type WebSocketFramePreview = {
  direction: string;
  opcode: string;
  size: number;
  text?: string | null;
  base64?: string | null;
  truncated: boolean;
};

export type CapturedSession = {
  id: string;
  kind: SessionKind;
  state: SessionState;
  started_at: string;
  ended_at?: string | null;
  duration_ms?: number | null;
  method?: string | null;
  url?: string | null;
  scheme?: string | null;
  host?: string | null;
  port?: number | null;
  path?: string | null;
  status?: number | null;
  request_headers: HeaderPair[];
  response_headers: HeaderPair[];
  request_body: BodyPreview;
  response_body: BodyPreview;
  grpc_request_metadata: HeaderPair[];
  grpc_response_metadata: HeaderPair[];
  grpc_request_trailers: TrailersPreview;
  grpc_response_trailers: TrailersPreview;
  timeline: TimelineEntry[];
  websocket_frames: WebSocketFramePreview[];
  bytes_up: number;
  bytes_down: number;
  error?: string | null;
  matched_rule_ids: string[];
};

export type SessionSummary = {
  id: string;
  kind: SessionKind;
  state: SessionState;
  started_at: string;
  method?: string | null;
  url?: string | null;
  host?: string | null;
  status?: number | null;
  duration_ms?: number | null;
  bytes_up: number;
  bytes_down: number;
  matched_rule_ids: string[];
};

export type SessionFilter = {
  keyword?: string | null;
  method?: string | null;
  status?: number | null;
  host?: string | null;
  onlyFailed: boolean;
  onlyMocked: boolean;
};

export type ProxyStatus = {
  running: boolean;
  listen_addr?: string | null;
  started_at?: string | null;
  active_sessions: number;
  https_mitm: HttpsMitmStatus;
};

export type RootCaInfo = {
  generated_at: string;
  cert_path: string;
  fingerprint_sha256: string;
};

export type HttpsMitmStatus = {
  enabled: boolean;
  ready: boolean;
  local_only: boolean;
  default_off: boolean;
  root_ca?: RootCaInfo | null;
  install_hint: string;
};

export type TrafficBucket = {
  key: string;
  count: number;
  bytes_up: number;
  bytes_down: number;
  average_duration_ms?: number | null;
};

export type TrafficOverview = {
  total_sessions: number;
  total_bytes_up: number;
  total_bytes_down: number;
  average_duration_ms?: number | null;
  p95_duration_ms?: number | null;
  by_host: TrafficBucket[];
  by_method: TrafficBucket[];
  by_status: TrafficBucket[];
};

export type Rule = {
  id: string;
  name: string;
  enabled: boolean;
  priority: number;
  match: {
    url_contains?: string | null;
    method?: string | null;
    host?: string | null;
    status?: number | null;
  };
  action:
    | { type: "rewrite_request_headers"; headers: HeaderPair[] }
    | { type: "rewrite_response_headers"; headers: HeaderPair[] }
    | { type: "mock_response"; status: number; headers: HeaderPair[]; body: string }
    | { type: "delay"; millis: number }
    | { type: "script"; script: string };
  created_at: string;
  updated_at: string;
};

export type CollectionItem = {
  id: string;
  name: string;
  session_id?: string | null;
  method?: string | null;
  url?: string | null;
  headers: HeaderPair[];
  body: BodyPreview;
  created_at: string;
};

export type RequestCollection = {
  id: string;
  name: string;
  description?: string | null;
  items: CollectionItem[];
  created_at: string;
  updated_at: string;
};
