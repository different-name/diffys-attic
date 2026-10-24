import { For } from "solid-js";
import { Icon } from "./icons";
import { folder, folders, openFolder, searching, startCompose } from "./state";

export function Sidebar() {
  return (
    <nav class="sidebar" aria-label="Folders">
      <div class="brand">
        <span>bone</span>
        <button class="btn primary icon-btn" aria-label="New message" title="New message (c)" onClick={() => startCompose({ kind: "new" })}>
          <Icon name="plus" />
        </button>
      </div>
      <For each={folders()}>
        {(f) => (
          <button class={`folder${!searching() && folder() === f.key ? " active" : ""}`} onClick={() => openFolder(f.key)}>
            <span>{f.name}</span>
            <span class="count">{f.unread || ""}</span>
          </button>
        )}
      </For>
    </nav>
  );
}

export function FolderMenu(props: { onClose: () => void }) {
  return (
    <>
      <div class="scrim" onClick={props.onClose} />
      <div class="menu" role="menu" style={{ top: "56px", left: "12px" }}>
        <For each={folders()}>
          {(f) => (
            <button
              role="menuitem"
              onClick={() => {
                openFolder(f.key);
                props.onClose();
              }}
            >
              {f.name}
              {f.unread ? <span class="mono meta"> {f.unread}</span> : null}
            </button>
          )}
        </For>
      </div>
    </>
  );
}
