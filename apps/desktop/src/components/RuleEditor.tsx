import { useEffect, useState } from "react";
import type { Rule } from "../types";

type Props = {
  rules: Rule[];
  onSave: (rule: Rule) => Promise<void>;
  onDelete: (id: string) => Promise<void>;
  onRefresh: () => Promise<void>;
};

const sampleRule = (): Rule => {
  const now = new Date().toISOString();
  return {
    id: `rule-${Date.now()}`,
    name: "Mock user API",
    enabled: true,
    priority: 100,
    match: {
      url_contains: "/api/user",
      method: "GET",
    },
    action: {
      type: "mock_response",
      status: 200,
      headers: [{ name: "content-type", value: "application/json" }],
      body: "{\"id\":1,\"name\":\"demo\"}",
    },
    created_at: now,
    updated_at: now,
  };
};

export function RuleEditor({ rules, onSave, onDelete, onRefresh }: Props) {
  const [selected, setSelected] = useState<Rule>(sampleRule());
  const [text, setText] = useState(JSON.stringify(selected, null, 2));
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (rules.length > 0) {
      setSelected(rules[0]);
      setText(JSON.stringify(rules[0], null, 2));
    }
  }, [rules]);

  async function save() {
    try {
      const parsed = JSON.parse(text) as Rule;
      await onSave(parsed);
      setSelected(parsed);
      setError(null);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    }
  }

  return (
    <section className="rules-panel">
      <div className="panel-title">
        <h3>Rule Engine</h3>
        <div>
          <button onClick={() => {
            const rule = sampleRule();
            setSelected(rule);
            setText(JSON.stringify(rule, null, 2));
          }}>New</button>
          <button onClick={save}>Save</button>
          <button onClick={onRefresh}>Refresh</button>
        </div>
      </div>
      <div className="rule-list">
        {rules.map((rule) => (
          <button
            key={rule.id}
            className={rule.id === selected.id ? "active" : ""}
            onClick={() => {
              setSelected(rule);
              setText(JSON.stringify(rule, null, 2));
            }}
          >
            <strong>{rule.name}</strong>
            <span>{rule.enabled ? "enabled" : "disabled"} · {rule.action.type}</span>
          </button>
        ))}
      </div>
      <textarea value={text} onChange={(event) => setText(event.target.value)} spellCheck={false} />
      <div className="rule-actions">
        <button className="danger" disabled={!selected.id} onClick={() => onDelete(selected.id)}>Delete</button>
        {error && <span className="error-text">{error}</span>}
      </div>
    </section>
  );
}
