import { useEffect, useState } from "react";
import type { CapturedSession, HeaderPair, RequestCollection, TrailersPreview } from "../types";

type Props = {
  session: CapturedSession | null;
  collections: RequestCollection[];
  onReplay: () => void;
  onExport: () => void;
  onAddToCollection: (collectionId: string) => void;
};

function HeaderList({ headers }: { headers: HeaderPair[] }) {
  return (
    <div className="kv-list">
      {headers.map((header, index) => (
        <div className="kv" key={`${header.name}-${index}`}>
          <span>{header.name}</span>
          <code>{header.value}</code>
        </div>
      ))}
      {headers.length === 0 && <p className="muted">No headers captured.</p>}
    </div>
  );
}

function TrailerList({ trailers }: { trailers: TrailersPreview }) {
  return <HeaderList headers={trailers.headers} />;
}

function BodyBlock({ title, body }: { title: string; body: CapturedSession["request_body"] }) {
  const content = body.pretty ?? body.text ?? (body.base64 ? `[base64] ${body.base64}` : "");
  return (
    <section className="body-block">
      <h4>{title}</h4>
      <p className="muted">
        {body.content_type ?? "unknown"} {body.encoding ? `· ${body.encoding}` : ""} · {body.size} bytes
        {body.truncated ? " · truncated" : ""}
      </p>
      <pre>{content || "No body"}</pre>
    </section>
  );
}

function TimelineBlock({ timeline }: { timeline: CapturedSession["timeline"] }) {
  return (
    <section className="body-block">
      <h4>Timeline</h4>
      <div className="timeline-list">
        {timeline.map((entry) => (
          <div className="timeline-item" key={`${entry.name}-${entry.started_at}`}>
            <strong>{entry.name}</strong>
            <span>{entry.duration_ms == null ? "-" : `${entry.duration_ms} ms`}</span>
          </div>
        ))}
        {timeline.length === 0 && <p className="muted">No timeline captured.</p>}
      </div>
    </section>
  );
}

function WebSocketFramesBlock({ frames }: { frames: CapturedSession["websocket_frames"] }) {
  return (
    <section className="body-block">
      <h4>WebSocket Frames</h4>
      <div className="frame-list">
        {frames.map((frame, index) => (
          <div className="frame-item" key={`${frame.direction}-${frame.opcode}-${index}`}>
            <div className="frame-meta">
              <strong>{frame.direction}</strong>
              <span>
                {frame.opcode} · {frame.size} bytes
              </span>
            </div>
            <pre>{frame.text ?? (frame.base64 ? `[base64] ${frame.base64}` : "binary")}</pre>
          </div>
        ))}
        {frames.length === 0 && <p className="muted">No websocket frames captured.</p>}
      </div>
    </section>
  );
}

export function RequestDetail({ session, collections, onReplay, onExport, onAddToCollection }: Props) {
  const [collectionId, setCollectionId] = useState("");

  useEffect(() => {
    if (!collections.some((collection) => collection.id === collectionId)) {
      setCollectionId(collections[0]?.id ?? "");
    }
  }, [collectionId, collections]);

  if (!session) {
    return (
      <aside className="detail empty-detail">
        <h2>Request Detail</h2>
        <p>Select a captured request to inspect headers, bodies, timeline and replay controls.</p>
      </aside>
    );
  }

  const isConnect = session.kind === "ConnectTunnel";
  return (
    <aside className="detail">
      <div className="detail-header">
        <div>
          <h2>
            {session.method ?? session.kind} {session.url ?? session.host}
          </h2>
          <p>
            {session.state} · {session.duration_ms ?? "-"} ms · status {session.status ?? "-"}
          </p>
        </div>
        <div className="detail-actions">
          <button disabled={isConnect} onClick={onReplay}>
            Replay
          </button>
          <button onClick={onExport}>Export Selected</button>
        </div>
      </div>

      {isConnect && (
        <div className="notice">
          CONNECT tunnels stay opaque while HTTPS decrypt is off. Enable it only for local authorized debugging.
        </div>
      )}

      <section className="collection-save">
        <div>
          <strong>Save Request</strong>
          <p className="muted">Collections store the current redacted request snapshot.</p>
        </div>
        <select value={collectionId} onChange={(event) => setCollectionId(event.target.value)}>
          {collections.map((collection) => (
            <option value={collection.id} key={collection.id}>{collection.name}</option>
          ))}
          {collections.length === 0 && <option value="">Create a collection first</option>}
        </select>
        <button disabled={!collectionId || isConnect} onClick={() => onAddToCollection(collectionId)}>
          Add
        </button>
      </section>

      <section className="overview-grid">
        <div>
          <span>Host</span>
          <strong>{session.host ?? "-"}</strong>
        </div>
        <div>
          <span>Port</span>
          <strong>{session.port ?? "-"}</strong>
        </div>
        <div>
          <span>Bytes Up</span>
          <strong>{session.bytes_up}</strong>
        </div>
        <div>
          <span>Bytes Down</span>
          <strong>{session.bytes_down}</strong>
        </div>
        <div>
          <span>Rule Hits</span>
          <strong>{session.matched_rule_ids.join(", ") || "-"}</strong>
        </div>
        <div>
          <span>Error</span>
          <strong>{session.error ?? "-"}</strong>
        </div>
      </section>

      <div className="detail-columns">
        <section>
          <h3>Request Headers</h3>
          <HeaderList headers={session.request_headers} />
        </section>
        <section>
          <h3>Response Headers</h3>
          <HeaderList headers={session.response_headers} />
        </section>
      </div>

      {(session.grpc_request_metadata.length > 0
        || session.grpc_response_metadata.length > 0
        || session.grpc_request_trailers.headers.length > 0
        || session.grpc_response_trailers.headers.length > 0) && (
        <section className="body-block grpc-block">
          <h4>gRPC Metadata</h4>
          <div className="detail-columns">
            <section>
              <h3>Request Metadata</h3>
              <HeaderList headers={session.grpc_request_metadata} />
              <h3>Request Trailers</h3>
              <TrailerList trailers={session.grpc_request_trailers} />
            </section>
            <section>
              <h3>Response Metadata</h3>
              <HeaderList headers={session.grpc_response_metadata} />
              <h3>Response Trailers</h3>
              <TrailerList trailers={session.grpc_response_trailers} />
            </section>
          </div>
        </section>
      )}

      <BodyBlock title="Request Body" body={session.request_body} />
      <BodyBlock title="Response Body" body={session.response_body} />
      <TimelineBlock timeline={session.timeline} />
      {session.kind === "WebSocket" && <WebSocketFramesBlock frames={session.websocket_frames} />}

      <section className="body-block">
        <h4>Raw JSON</h4>
        <pre>{JSON.stringify(session, null, 2)}</pre>
      </section>
    </aside>
  );
}
