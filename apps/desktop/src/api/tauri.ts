import { invoke } from "@tauri-apps/api/core";
import type {
  CapturedSession,
  HttpsMitmStatus,
  ProxyStatus,
  RequestCollection,
  RootCaInfo,
  Rule,
  SessionFilter,
  SessionSummary,
  TrafficOverview,
} from "../types";

export const api = {
  proxyStatus: () => invoke<ProxyStatus>("proxy_status"),
  httpsMitmStatus: () => invoke<HttpsMitmStatus>("https_mitm_status"),
  generateRootCa: () => invoke<RootCaInfo>("generate_root_ca"),
  setHttpsMitmEnabled: (enabled: boolean) =>
    invoke<HttpsMitmStatus>("set_https_mitm_enabled", { enabled }),
  startProxy: (port: number) => invoke<string>("start_proxy", { port }),
  stopProxy: () => invoke<void>("stop_proxy"),
  listSessions: (filter: SessionFilter, limit = 200, offset = 0) =>
    invoke<SessionSummary[]>("list_sessions", { filter, limit, offset }),
  trafficOverview: (limit = 500) => invoke<TrafficOverview>("traffic_overview", { limit }),
  getSessionDetail: (id: string) => invoke<CapturedSession>("get_session_detail", { id }),
  clearSessions: () => invoke<void>("clear_sessions"),
  listRules: () => invoke<Rule[]>("list_rules"),
  saveRule: (rule: Rule) => invoke<void>("save_rule", { rule }),
  deleteRule: (id: string) => invoke<void>("delete_rule", { id }),
  listCollections: () => invoke<RequestCollection[]>("list_collections"),
  saveCollection: (collection: RequestCollection) =>
    invoke<void>("save_collection", { collection }),
  deleteCollection: (id: string) => invoke<void>("delete_collection", { id }),
  addSessionToCollection: (collectionId: string, sessionId: string) =>
    invoke<void>("add_session_to_collection", { collection_id: collectionId, session_id: sessionId }),
  replaySession: (id: string) => invoke<string>("replay_session", { id }),
  exportSessions: (ids: string[], format?: "json" | "har") =>
    invoke<string>("export_sessions", { ids, format }),
};
