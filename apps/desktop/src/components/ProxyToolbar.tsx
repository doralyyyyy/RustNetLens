import type { ProxyStatus } from "../types";

type Props = {
  port: number;
  setPort: (port: number) => void;
  status: ProxyStatus | null;
  busy: boolean;
  onStart: () => void;
  onStop: () => void;
  onClear: () => void;
  onExport: () => void;
};

export function ProxyToolbar({ port, setPort, status, busy, onStart, onStop, onClear, onExport }: Props) {
  const running = status?.running ?? false;
  return (
    <header className="toolbar">
      <div className="brand">
        <div className="brand-mark">RNL</div>
        <div>
          <h1>RustNetLens</h1>
          <p>Local HTTP proxy, rewrite, mock, replay</p>
        </div>
      </div>
      <div className="toolbar-actions">
        <label className="port-input">
          <span>Port</span>
          <input
            type="number"
            min={1}
            max={65535}
            value={port}
            onChange={(event) => setPort(Number(event.target.value))}
            disabled={running || busy}
          />
        </label>
        <button className="primary" disabled={running || busy} onClick={onStart}>
          Start Proxy
        </button>
        <button disabled={!running || busy} onClick={onStop}>
          Stop
        </button>
        <button disabled={busy} onClick={onClear}>
          Clear
        </button>
        <button disabled={busy} onClick={onExport}>
          Export
        </button>
      </div>
      <div className={running ? "status-pill running" : "status-pill"}>
        <span />
        {running ? status?.listen_addr : "Stopped"}
      </div>
    </header>
  );
}
