import { useCallback, useEffect, useMemo, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { api } from "./api/tauri";
import { ProxyToolbar } from "./components/ProxyToolbar";
import { RequestDetail } from "./components/RequestDetail";
import { RequestTable } from "./components/RequestTable";
import { RuleEditor } from "./components/RuleEditor";
import type { CapturedSession, ProxyStatus, Rule, SessionFilter, SessionSummary } from "./types";

const emptyFilter: SessionFilter = {
  keyword: "",
  method: "",
  status: null,
  host: "",
  onlyFailed: false,
  onlyMocked: false,
};

function App() {
  const [port, setPort] = useState(8899);
  const [status, setStatus] = useState<ProxyStatus | null>(null);
  const [sessions, setSessions] = useState<SessionSummary[]>([]);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [detail, setDetail] = useState<CapturedSession | null>(null);
  const [rules, setRules] = useState<Rule[]>([]);
  const [filter, setFilter] = useState<SessionFilter>(emptyFilter);
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState<string | null>(null);

  const loadStatus = useCallback(async () => setStatus(await api.proxyStatus()), []);
  const loadRules = useCallback(async () => setRules(await api.listRules()), []);
  const loadSessions = useCallback(async () => setSessions(await api.listSessions(filter, 500, 0)), [filter]);

  useEffect(() => {
    loadStatus().catch((err) => setMessage(String(err)));
    loadSessions().catch((err) => setMessage(String(err)));
    loadRules().catch((err) => setMessage(String(err)));
  }, [loadStatus, loadRules, loadSessions]);

  useEffect(() => {
    const unlisten = listen<SessionSummary>("session://captured", (event) => {
      setSessions((current) => {
        const withoutDuplicate = current.filter((session) => session.id !== event.payload.id);
        return [event.payload, ...withoutDuplicate].slice(0, 500);
      });
    });
    return () => {
      unlisten.then((fn) => fn()).catch(() => undefined);
    };
  }, []);

  useEffect(() => {
    if (!selectedId) {
      setDetail(null);
      return;
    }
    api.getSessionDetail(selectedId)
      .then(setDetail)
      .catch((err) => setMessage(String(err)));
  }, [selectedId]);

  const filteredSessions = useMemo(() => sessions, [sessions]);

  async function run(action: () => Promise<void>) {
    setBusy(true);
    setMessage(null);
    try {
      await action();
    } catch (err) {
      setMessage(err instanceof Error ? err.message : String(err));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="app-shell">
      <ProxyToolbar
        port={port}
        setPort={setPort}
        status={status}
        busy={busy}
        onStart={() => run(async () => {
          const listenAddr = await api.startProxy(port);
          setMessage(`Proxy listening at ${listenAddr}`);
          await loadStatus();
        })}
        onStop={() => run(async () => {
          await api.stopProxy();
          await loadStatus();
        })}
        onClear={() => run(async () => {
          await api.clearSessions();
          setSessions([]);
          setSelectedId(null);
          setDetail(null);
        })}
        onExport={() => run(async () => {
          const ids = selectedId ? [selectedId] : sessions.map((session) => session.id);
          const path = await api.exportSessions(ids);
          setMessage(`Exported to ${path}`);
        })}
      />

      <main className="main-grid">
        <section className="left-panel">
          <div className="filters">
            <input
              placeholder="Search URL, host, method"
              value={filter.keyword ?? ""}
              onChange={(event) => setFilter({ ...filter, keyword: event.target.value })}
              onKeyDown={(event) => {
                if (event.key === "Enter") loadSessions().catch((err) => setMessage(String(err)));
              }}
            />
            <input
              placeholder="Method"
              value={filter.method ?? ""}
              onChange={(event) => setFilter({ ...filter, method: event.target.value })}
            />
            <input
              placeholder="Host"
              value={filter.host ?? ""}
              onChange={(event) => setFilter({ ...filter, host: event.target.value })}
            />
            <input
              placeholder="Status"
              type="number"
              value={filter.status ?? ""}
              onChange={(event) => setFilter({ ...filter, status: event.target.value ? Number(event.target.value) : null })}
            />
            <label><input type="checkbox" checked={filter.onlyFailed} onChange={(event) => setFilter({ ...filter, onlyFailed: event.target.checked })} /> Failed</label>
            <label><input type="checkbox" checked={filter.onlyMocked} onChange={(event) => setFilter({ ...filter, onlyMocked: event.target.checked })} /> Mocked</label>
            <button onClick={() => loadSessions().catch((err) => setMessage(String(err)))}>Apply</button>
          </div>
          {message && <div className="message">{message}</div>}
          <RequestTable sessions={filteredSessions} selectedId={selectedId} onSelect={setSelectedId} />
          <RuleEditor
            rules={rules}
            onRefresh={loadRules}
            onSave={async (rule) => {
              await api.saveRule(rule);
              await loadRules();
              setMessage("Rule saved and applied");
            }}
            onDelete={async (id) => {
              await api.deleteRule(id);
              await loadRules();
              setMessage("Rule deleted");
            }}
          />
        </section>
        <RequestDetail
          session={detail}
          onReplay={() => selectedId && run(async () => {
            const id = await api.replaySession(selectedId);
            setMessage(`Replay created session ${id}`);
            await loadSessions();
          })}
          onExport={() => detail && run(async () => {
            const path = await api.exportSessions([detail.id]);
            setMessage(`Exported to ${path}`);
          })}
        />
      </main>
    </div>
  );
}

export default App;
