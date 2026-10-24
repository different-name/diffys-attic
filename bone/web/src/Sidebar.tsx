import { For } from "solid-js";
import { Icon } from "./icons";
import { folder, folders, openFolder, searching, startCompose } from "./state";

const SPECIAL = new Set(["inbox", "archive", "sent", "drafts", "junk", "trash"]);

export function Sidebar(props: { drawer?: boolean; onPick?: () => void }) {
  return (
    <nav class={`sidebar${props.drawer ? " in-drawer" : ""}`} aria-label="Folders">
      <div class="brand">
        <span>bone</span>
        <button class="btn primary icon-btn" aria-label="New message" title="New message (c)" onClick={() => startCompose({ kind: "new" })}>
          <Icon name="plus" />
        </button>
      </div>
      <For each={folders()}>
        {(f, i) => (
          <button
            class="folder"
            classList={{
              active: !searching() && folder() === f.key,
              split: !SPECIAL.has(f.key) && i() > 0 && SPECIAL.has(folders()[i() - 1].key),
            }}
            onClick={() => {
              openFolder(f.key);
              props.onPick?.();
            }}
          >
            <span>{f.name}</span>
            <span class="count">{f.unread || ""}</span>
          </button>
        )}
      </For>
    </nav>
  );
}
