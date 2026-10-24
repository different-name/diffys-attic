import { createEffect, createMemo, createResource, createSignal, For, on, Show } from "solid-js";
import { api, token } from "./api";
import { Composer } from "./Composer";
import { local, when, who } from "./format";
import { Icon } from "./icons";
import { MessageView } from "./Message";
import { act, actOn, compose, folder, folders, list, searching, setList, setPane, startCompose } from "./state";
import type { Row } from "./types";

export const [threadVersion, bumpThread] = createSignal(0);

export function MoveMenu(props: { threads: number[]; onClose: () => void }) {
  const targets = () => folders().filter((f) => f.key !== folder() && f.key !== "sent" && f.key !== "drafts");
  return (
    <>
      <div class="scrim" onClick={props.onClose} />
      <div class="menu anchored" role="menu" aria-label="Move to">
        <div class="grip phone-only" aria-hidden="true" />
        <For each={targets()}>
          {(f) => (
            <button
              role="menuitem"
              onClick={() => {
                props.onClose();
                void actOn(props.threads, "move", f.key);
              }}
            >
              {f.name}
            </button>
          )}
        </For>
      </div>
    </>
  );
}

export function Thread(props: { thread: number }) {
  const [rows] = createResource(
    () => [props.thread, threadVersion(), token()] as const,
    ([id]) => api.thread(id),
  );
  const [expanded, setExpanded] = createSignal<Set<number>>(new Set());
  const [moving, setMoving] = createSignal(false);

  const messages = (): Row[] => rows() ?? [];
  const ids = createMemo(() => messages().map((m) => m.id), [], {
    equals: (a, b) => a.length === b.length && a.every((id, i) => id === b[i]),
  });
  const byId = createMemo(() => new Map(messages().map((m) => [m.id, m])));
  const latest = () => messages()[messages().length - 1];
  const unread = () => messages().some((m) => m.unread);
  const replying = () => {
    const c = compose();
    return c?.kind === "reply" ? c : null;
  };

  createEffect(
    on(
      () => props.thread,
      () => setExpanded(new Set<number>()),
    ),
  );

  createEffect(
    on(rows, (r) => {
      if (r?.some((m) => m.unread)) {
        void act(props.thread, "read");
        setList("items", (c) => c.thread === props.thread, "unread", false);
      }
    }),
  );

  const isOpen = (m: Row) => m.id === latest()?.id || expanded().has(m.id);
  const conversation = () => list.items.find((c) => c.thread === props.thread);

  return (
    <section class="reader" aria-label={latest()?.subject ?? "Conversation"}>
      <div class="toolbar">
        <button class="btn ghost icon-btn phone-only" aria-label="Back" onClick={() => setPane("list")}>
          <Icon name="back" size={20} />
        </button>
        <Show when={searching() || folder() !== "archive"}>
          <button class="btn done desktop-only" onClick={() => void act(props.thread, "archive")}>
            Archive<span class="kbd">e</span>
          </button>
        </Show>
        <Show when={latest()}>
          <button class="btn desktop-only" onClick={() => startCompose({ kind: "reply", id: latest()!.id, all: false })}>
            Reply<span class="kbd">r</span>
          </button>
          <button class="btn desktop-only" onClick={() => startCompose({ kind: "reply", id: latest()!.id, all: true })}>
            Reply all<span class="kbd">a</span>
          </button>
        </Show>
        <div class="end">
          <button
            class="btn ghost icon-btn"
            aria-label={unread() || conversation()?.unread ? "Mark read" : "Mark unread"}
            title="Mark unread (u)"
            onClick={() => void act(props.thread, unread() ? "read" : "unread")}
          >
            <Icon name="mail" size={16} />
          </button>
          <button class="btn ghost icon-btn" aria-label="Move" title="Move (v)" onClick={() => setMoving(true)}>
            <Icon name="folder" size={16} />
          </button>
          <button class="btn danger icon-btn" aria-label="Delete" title="Delete (#)" onClick={() => void act(props.thread, "delete")}>
            <Icon name="trash" size={16} />
          </button>
        </div>
        <Show when={moving()}>
          <MoveMenu threads={[props.thread]} onClose={() => setMoving(false)} />
        </Show>
      </div>

      <div class="scroll">
        <Show when={latest()}>
          <div class="subject-line">
            <h2>{latest()!.subject || "(no subject)"}</h2>
            <Show when={messages().length > 1}>
              <span class="mono meta" style={{ "font-size": "11px" }}>
                {messages().length} messages
              </span>
            </Show>
          </div>
        </Show>
        <For each={ids()}>
          {(id) => {
            const m = () => byId().get(id)!;
            return (
              <Show
                when={isOpen(m())}
                fallback={
                  <button class="collapsed" onClick={() => setExpanded(new Set([...expanded(), id]))}>
                    <span class="who">{who(m().from)}</span>
                    <span class="snippet">{m().snippet}</span>
                    <span class="mono meta" style={{ "font-size": "11px" }}>
                      {when(m().date)}
                    </span>
                  </button>
                }
              >
                <MessageView id={id} />
              </Show>
            );
          }}
        </For>
        <Show when={replying()}>{(r) => <Composer reply={{ id: r().id, all: r().all }} />}</Show>
      </div>

      <Show when={latest() && !replying()}>
        <div class="thumb">
          <button class="reply-field" onClick={() => startCompose({ kind: "reply", id: latest()!.id, all: false })}>
            <span>Reply</span>
            <Show when={latest()!.recipient}>
              <span class="addr">as {local(latest()!.recipient)}</span>
            </Show>
          </button>
          <div class="row-btns">
            <button class="btn done" onClick={() => void act(props.thread, "archive")}>
              <Icon name="archive" size={17} />
              Archive
            </button>
            <button class="btn" onClick={() => startCompose({ kind: "reply", id: latest()!.id, all: true })}>
              Reply all
            </button>
          </div>
        </div>
      </Show>
    </section>
  );
}
