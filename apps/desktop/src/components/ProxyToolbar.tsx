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
  onGenerateRootCa: () => void;
  onToggleHttpsMitm: (enabled: boolean) => void;
};

export function ProxyToolbar({
  port,
  setPort,
  status,
  busy,
  onStart,
  onStop,
  onClear,
  onExport,
  onGenerateRootCa,
  onToggleHttpsMitm,
}: Props) {
  const running = status?.running ?? false;
  const httpsMitm = status?.https_mitm;
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
      <section className={httpsMitm?.enabled ? "mitm-panel enabled" : "mitm-panel"}>
        <div className="mitm-copy">
          <strong>HTTPS Decrypt {httpsMitm?.enabled ? "On" : "Off"}</strong>
          <p>Disabled by default. Use only for local authorized debugging.</p>
          <small>{httpsMitm?.install_hint}</small>
          {httpsMitm?.root_ca && (
            <code title={httpsMitm.root_ca.fingerprint_sha256}>
              Root CA: {httpsMitm.root_ca.cert_path}
            </code>
          )}
        </div>
        <div className="mitm-actions">
          <button disabled={busy} onClick={onGenerateRootCa}>
            {httpsMitm?.ready ? "Refresh Root CA" : "Generate Root CA"}
          </button>
          <button
            className={httpsMitm?.enabled ? "" : "primary"}
            disabled={busy}
            onClick={() => onToggleHttpsMitm(!(httpsMitm?.enabled ?? false))}
          >
            {httpsMitm?.enabled ? "Disable Decrypt" : "Enable Decrypt"}
          </button>
        </div>
      </section>
    </header>
  );
}
