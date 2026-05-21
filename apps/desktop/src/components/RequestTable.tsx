import type { SessionSummary } from "../types";

type Props = {
  sessions: SessionSummary[];
  selectedId?: string | null;
  onSelect: (id: string) => void;
};

const formatBytes = (bytes: number) => {
  if (bytes > 1024 * 1024) return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
  if (bytes > 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${bytes} B`;
};

export function RequestTable({ sessions, selectedId, onSelect }: Props) {
  return (
    <div className="table-shell">
      <table className="request-table">
        <thead>
          <tr>
            <th>Time</th>
            <th>Method</th>
            <th>URL / Host</th>
            <th>Status</th>
            <th>Type</th>
            <th>Duration</th>
            <th>Size</th>
            <th>Rule Hit</th>
          </tr>
        </thead>
        <tbody>
          {sessions.map((session) => (
            <tr
              key={session.id}
              className={session.id === selectedId ? "selected" : ""}
              onClick={() => onSelect(session.id)}
            >
              <td>{new Date(session.started_at).toLocaleTimeString()}</td>
              <td><span className="method">{session.method ?? "-"}</span></td>
              <td className="url-cell">{session.url ?? session.host ?? "-"}</td>
              <td className={session.status && session.status >= 400 ? "bad" : "ok"}>
                {session.status ?? "-"}
              </td>
              <td>{session.kind}</td>
              <td>{session.duration_ms == null ? "-" : `${session.duration_ms} ms`}</td>
              <td>{formatBytes(session.bytes_up + session.bytes_down)}</td>
              <td>{session.matched_rule_ids.length ? session.matched_rule_ids.join(", ") : "-"}</td>
            </tr>
          ))}
          {sessions.length === 0 && (
            <tr>
              <td colSpan={8} className="empty">No sessions yet. Start the proxy and send curl traffic through it.</td>
            </tr>
          )}
        </tbody>
      </table>
    </div>
  );
}
