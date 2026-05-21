import type { CapturedSession, HeaderPair } from "../types";

type Props = {
  session: CapturedSession | null;
  onReplay: () => void;
  onExport: () => void;
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

function BodyBlock({ title, body }: { title: string; body: CapturedSession["request_body"] }) {
  const content = body.text ?? (body.base64 ? `[base64] ${body.base64}` : "");
  return (
    <section className="body-block">
      <h4>{title}</h4>
      <p className="muted">
        {body.content_type ?? "unknown"} · {body.size} bytes {body.truncated ? "· truncated" : ""}
      </p>
      <pre>{content || "No body"}</pre>
    </section>
  );
}

export function RequestDetail({ session, onReplay, onExport }: Props) {
  if (!session) {
    return (
      <aside className="detail empty-detail">
        <h2>Request Detail</h2>
        <p>Select a captured request to inspect headers, bodies, raw JSON and replay controls.</p>
      </aside>
    );
  }

  const isConnect = session.kind === "ConnectTunnel";
  return (
    <aside className="detail">
      <div className="detail-header">
        <div>
          <h2>{session.method ?? session.kind} {session.url ?? session.host}</h2>
          <p>{session.state} · {session.duration_ms ?? "-"} ms · status {session.status ?? "-"}</p>
        </div>
        <div className="detail-actions">
          <button disabled={isConnect} onClick={onReplay}>Replay</button>
          <button onClick={onExport}>Export Selected</button>
        </div>
      </div>

      {isConnect && (
        <div className="notice">
          HTTPS content is not decrypted in MVP. RustNetLens records only tunnel metadata.
        </div>
      )}

      <section className="overview-grid">
        <div><span>Host</span><strong>{session.host ?? "-"}</strong></div>
        <div><span>Port</span><strong>{session.port ?? "-"}</strong></div>
        <div><span>Bytes Up</span><strong>{session.bytes_up}</strong></div>
        <div><span>Bytes Down</span><strong>{session.bytes_down}</strong></div>
        <div><span>Rule Hits</span><strong>{session.matched_rule_ids.join(", ") || "-"}</strong></div>
        <div><span>Error</span><strong>{session.error ?? "-"}</strong></div>
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

      <BodyBlock title="Request Body" body={session.request_body} />
      <BodyBlock title="Response Body" body={session.response_body} />

      <section className="body-block">
        <h4>Raw JSON</h4>
        <pre>{JSON.stringify(session, null, 2)}</pre>
      </section>
    </aside>
  );
}
