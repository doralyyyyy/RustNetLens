import { invoke } from "@tauri-apps/api/core";
import type { CapturedSession, ProxyStatus, Rule, SessionFilter, SessionSummary } from "../types";

export const api = {
  proxyStatus: () => invoke<ProxyStatus>("proxy_status"),
  startProxy: (port: number) => invoke<string>("start_proxy", { port }),
  stopProxy: () => invoke<void>("stop_proxy"),
  listSessions: (filter: SessionFilter, limit = 200, offset = 0) =>
    invoke<SessionSummary[]>("list_sessions", { filter, limit, offset }),
  getSessionDetail: (id: string) => invoke<CapturedSession>("get_session_detail", { id }),
  clearSessions: () => invoke<void>("clear_sessions"),
  listRules: () => invoke<Rule[]>("list_rules"),
  saveRule: (rule: Rule) => invoke<void>("save_rule", { rule }),
  deleteRule: (id: string) => invoke<void>("delete_rule", { id }),
  replaySession: (id: string) => invoke<string>("replay_session", { id }),
  exportSessions: (ids: string[]) => invoke<string>("export_sessions", { ids }),
};
