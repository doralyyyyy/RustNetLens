import { useMemo, useState } from "react";
import type { RequestCollection } from "../types";

type Props = {
  collections: RequestCollection[];
  busy: boolean;
  onSave: (collection: RequestCollection) => Promise<void>;
  onDelete: (id: string) => Promise<void>;
};

function newCollection(name: string, description: string): RequestCollection {
  const now = new Date().toISOString();
  return {
    id: `collection-${Date.now()}`,
    name,
    description: description || null,
    items: [],
    created_at: now,
    updated_at: now,
  };
}

export function CollectionPanel({ collections, busy, onSave, onDelete }: Props) {
  const [name, setName] = useState("");
  const [description, setDescription] = useState("");
  const itemCount = useMemo(
    () => collections.reduce((total, collection) => total + collection.items.length, 0),
    [collections],
  );

  return (
    <section className="collections-panel">
      <div className="panel-title">
        <div>
          <h3>Collections</h3>
          <p className="muted">{collections.length} collections / {itemCount} saved requests</p>
        </div>
        <div className="collection-create">
          <input
            aria-label="Collection name"
            placeholder="Collection name"
            value={name}
            onChange={(event) => setName(event.target.value)}
          />
          <input
            aria-label="Collection description"
            placeholder="Description"
            value={description}
            onChange={(event) => setDescription(event.target.value)}
          />
          <button
            disabled={busy || !name.trim()}
            onClick={async () => {
              await onSave(newCollection(name.trim(), description.trim()));
              setName("");
              setDescription("");
            }}
          >
            Create
          </button>
        </div>
      </div>
      <div className="collection-list">
        {collections.map((collection) => (
          <article className="collection-card" key={collection.id}>
            <div>
              <strong>{collection.name}</strong>
              <p>{collection.description || "Saved captured requests"}</p>
            </div>
            <span>{collection.items.length} items</span>
            <button className="danger" disabled={busy} onClick={() => onDelete(collection.id)}>
              Delete
            </button>
          </article>
        ))}
        {collections.length === 0 && <p className="muted">Create a collection, then save sessions from request detail.</p>}
      </div>
    </section>
  );
}
