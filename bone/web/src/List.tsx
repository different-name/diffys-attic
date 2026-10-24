import { createEffect, createSignal, For, on, Show } from "solid-js";
import { local, splitQuery, when } from "./format";
import { Icon } from "./icons";
import { threadHref } from "./router";
import { FolderMenu } from "./Sidebar";
import { act, folder, folders, list, loadMore, open, query, search, searching, selected } from "./state";
import type { Conversation } from "./types";

const SWIPE_ARCHIVE = 110;

function Row(props: { c: Conversation }) {
  const [dx, setDx] = createSignal(0);
  let startX = 0;
  let startY = 0;
  let tracking = false;
  let horizontal = false;

  const down = (e: PointerEvent) => {
    if (e.pointerType === "mouse") return;
    startX = e.clientX;
    startY = e.clientY;
    tracking = true;
    horizontal = false;
  };
  const moveTo = (e: PointerEvent) => {
    if (!tracking) return;
    const x = e.clientX - startX;
    const y = e.clientY - startY;
    if (!horizontal && Math.abs(x) > 10 && Math.abs(x) > Math.abs(y)) {
      horizontal = true;
      (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
    }
    if (horizontal) setDx(Math.min(0, x));
  };
  const up = () => {
    if (!tracking) return;
    tracking = false;
    if (dx() < -SWIPE_ARCHIVE) void act(props.c.thread, "archive");
    setDx(0);
  };

  return (
    <div class="row-wrap">
      <Show when={dx() < 0}>
        <div class="swipe-under" aria-hidden="true">
          <Icon name="archive" size={18} />
          Archive
        </div>
      </Show>
      <a
        class={`row${props.c.unread ? " unread" : ""}${selected() === props.c.thread ? " selected" : ""}`}
        href={threadHref(props.c.thread)}
        style={{ transform: dx() ? `translateX(${dx()}px)` : undefined }}
        onClick={(e) => {
          e.preventDefault();
          if (!horizontal) open(props.c.thread);
        }}
        onPointerDown={down}
        onPointerMove={moveTo}
        onPointerUp={up}
        onPointerCancel={up}
      >
        <div class="line">
          <span class={`dot${props.c.unread ? " on" : ""}`} aria-label={props.c.unread ? "Unread" : undefined} />
          <span class="from">{props.c.from?.name || props.c.from?.addr || "Unknown sender"}</span>
          <Show when={props.c.messages > 1}>
            <span class="mono meta" style={{ "font-size": "11px" }}>
              {props.c.messages}
            </span>
          </Show>
          <Show when={searching()}>
            <span class="badge">{folders().find((f) => f.key === props.c.folder)?.name ?? props.c.folder}</span>
          </Show>
          <span class="time">{when(props.c.date)}</span>
        </div>
        <div class="line2">
          <span class="text">
            <span class="subject">{props.c.subject || "(no subject)"}</span> <span class="snippet">{props.c.snippet}</span>
          </span>
          <Show when={props.c.recipient}>
            <span class="addr" title={props.c.recipient ?? ""}>
              {local(props.c.recipient)}
            </span>
          </Show>
        </div>
      </a>
    </div>
  );
}

export function SearchField(props: { autofocus?: boolean; ref?: (el: HTMLInputElement) => void }) {
  const parts = () => splitQuery(query());
  const [text, setText] = createSignal(parts().text);
  createEffect(on(query, (q) => setText(splitQuery(q).text), { defer: true }));

  const submit = (e: Event) => {
    e.preventDefault();
    search([...parts().chips, text().trim()].filter(Boolean).join(" "));
  };
  const remove = (chip: string) => search([...parts().chips.filter((c) => c !== chip), parts().text].filter(Boolean).join(" "));

  return (
    <form class="search" role="search" onSubmit={submit}>
      <Icon name="search" size={13} style={{ color: "var(--meta)" }} />
      <For each={parts().chips}>
        {(chip) => (
          <span class="chip" title={chip}>
            <span>{chip}</span>
            <button type="button" aria-label={`Remove ${chip}`} onClick={() => remove(chip)}>
              <Icon name="close" size={10} />
            </button>
          </span>
        )}
      </For>
      <label class="sr" for="search">
        Search
      </label>
      <input
        id="search"
        ref={props.ref}
        autofocus={props.autofocus}
        placeholder={parts().chips.length ? "" : "Search"}
        value={text()}
        onInput={(e) => setText(e.currentTarget.value)}
        onKeyDown={(e) => {
          if (e.key === "Escape") {
            setText("");
            search("");
            e.currentTarget.blur();
          }
          if (e.key === "Backspace" && !text() && parts().chips.length) remove(parts().chips[parts().chips.length - 1]);
        }}
      />
      <span class="hint-key desktop-only">/</span>
    </form>
  );
}

export function List(props: { searchRef?: (el: HTMLInputElement) => void }) {
  const [menu, setMenu] = createSignal(false);
  const [phoneSearch, setPhoneSearch] = createSignal(false);
  const current = () => folders().find((f) => f.key === folder());
  const title = () => (searching() ? "Search" : (current()?.name ?? "Inbox"));

  return (
    <section class="list" aria-label={title()}>
      <div class="pane-head">
        <h1 class="desktop-only">{title()}</h1>
        <button class="btn ghost phone-only" style={{ padding: "0 6px 0 0", color: "var(--text)" }} onClick={() => setMenu(true)}>
          <h1>{title()}</h1>
          <Icon name="down" size={16} style={{ color: "var(--meta)" }} />
        </button>
        <Show when={!searching() && current()}>
          <span class="mono meta" style={{ "font-size": "11px" }}>
            {current()!.unread ? `${current()!.unread} unread` : ""}
          </span>
        </Show>
        <div class="head-search desktop-only">
          <SearchField ref={props.searchRef} />
        </div>
        <button class="btn ghost icon-btn phone-only" style={{ "margin-left": "auto" }} aria-label="Search" onClick={() => setPhoneSearch(!phoneSearch())}>
          <Icon name="search" size={20} />
        </button>
      </div>
      <Show when={phoneSearch() || searching()}>
        <div class="phone-only" style={{ padding: "0 12px 10px" }}>
          <SearchField autofocus />
        </div>
      </Show>
      <div
        class="rows"
        onScroll={(e) => {
          const el = e.currentTarget;
          if (el.scrollTop + el.clientHeight > el.scrollHeight - 400) void loadMore();
        }}
      >
        <For each={list.items}>{(c) => <Row c={c} />}</For>
        <Show when={list.loaded && !list.items.length}>
          <div class="empty">
            <Show when={!searching()} fallback={<strong>No matches</strong>}>
              <Show when={folder() === "inbox"} fallback={<span class="meta">Empty</span>}>
                <Icon name="inbox" size={40} />
                <strong>Nothing waiting</strong>
              </Show>
            </Show>
          </div>
        </Show>
      </div>
      <Show when={menu()}>
        <FolderMenu onClose={() => setMenu(false)} />
      </Show>
    </section>
  );
}
