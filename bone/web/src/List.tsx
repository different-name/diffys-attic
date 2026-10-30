import { createEffect, createSignal, For, on, onMount, Show } from "solid-js";
import { local, splitQuery, when } from "./format";
import { Icon } from "./icons";
import { threadHref } from "./router";
import { drawerSwipe, openDrawer } from "./Drawer";
import {
  act,
  actOn,
  addressColor,
  checkAll,
  checked,
  clearChecked,
  closeSearch,
  folder,
  folders,
  list,
  loadMore,
  open,
  openDraft,
  query,
  refreshList,
  search,
  searchOpen,
  searching,
  selected,
  setSearchOpen,
  toggleChecked,
} from "./state";
import { MoveMenu } from "./Thread";
import type { Conversation } from "./types";

const SWIPE_ARCHIVE = 110;
const LONG_PRESS = 450;
const PULL_AT = 64;

function pullToRefresh(rows: () => HTMLDivElement | undefined) {
  const [pull, setPull] = createSignal(0);
  const [refreshing, setRefreshing] = createSignal(false);
  let start: { y: number; id: number } | null = null;
  let active = false;
  let decided = false;

  const end = () => {
    if (active && pull() >= PULL_AT && !refreshing()) {
      setRefreshing(true);
      setPull(PULL_AT);
      void refreshList().finally(() => {
        setRefreshing(false);
        setPull(0);
      });
    } else {
      setPull(0);
    }
    start = null;
    active = false;
  };

  return {
    pull,
    refreshing,
    handlers: {
      onPointerDown(e: PointerEvent) {
        const el = rows();
        if (e.pointerType === "mouse" || refreshing() || !el || el.scrollTop > 0) return;
        start = { y: e.clientY, id: e.pointerId };
        active = false;
        decided = false;
      },
      onPointerMove(e: PointerEvent) {
        if (!start || e.pointerId !== start.id) return;
        const dy = e.clientY - start.y;
        if (!decided) {
          if (Math.abs(dy) < 10) return;
          decided = true;
          const el = rows();
          active = dy > 0 && !!el && el.scrollTop <= 0;
        }
        if (!active) return;
        setPull(Math.min(PULL_AT * 1.4, dy * 0.5));
      },
      onPointerUp: end,
      onPointerCancel: end,
    },
  };
}

const selecting = () => checked().size > 0;

function Row(props: { c: Conversation }) {
  const [dx, setDx] = createSignal(0);
  const isChecked = () => checked().has(props.c.thread);
  let startX = 0;
  let startY = 0;
  let tracking = false;
  let horizontal = false;
  let pressed = false;
  let press: number | undefined;

  const down = (e: PointerEvent) => {
    if (e.pointerType === "mouse") return;
    startX = e.clientX;
    startY = e.clientY;
    tracking = true;
    horizontal = false;
    pressed = false;
    press = window.setTimeout(() => {
      pressed = true;
      tracking = false;
      navigator.vibrate?.(10);
      toggleChecked(props.c.thread);
    }, LONG_PRESS);
  };
  const moveTo = (e: PointerEvent) => {
    if (!tracking) return;
    const x = e.clientX - startX;
    const y = e.clientY - startY;
    if (Math.abs(x) > 10 || Math.abs(y) > 10) window.clearTimeout(press);
    if (selecting()) return;
    if (!horizontal && Math.abs(x) > 10 && Math.abs(x) > Math.abs(y)) {
      if (x > 0) {
        tracking = false;
        return;
      }
      horizontal = true;
      (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
    }
    if (horizontal) setDx(Math.min(0, x));
  };
  const up = () => {
    window.clearTimeout(press);
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
        class={`row${props.c.unread ? " unread" : ""}${selected() === props.c.thread ? " selected" : ""}${isChecked() ? " checked" : ""}`}
        href={threadHref(props.c.thread)}
        style={{ transform: dx() ? `translateX(${dx()}px)` : undefined }}
        onClick={(e) => {
          e.preventDefault();
          if (pressed || horizontal) return;
          if (selecting() || e.shiftKey || e.ctrlKey || e.metaKey) toggleChecked(props.c.thread, e.shiftKey);
          else if (props.c.folder === "drafts") void openDraft(props.c.id);
          else open(props.c.thread);
        }}
        onContextMenu={(e) => pressed && e.preventDefault()}
        onPointerDown={down}
        onPointerMove={moveTo}
        onPointerUp={up}
        onPointerCancel={up}
      >
        <div class="line">
          <span class="mark">
            <span class={`dot${props.c.unread ? " on" : ""}`} aria-label={props.c.unread ? "Unread" : undefined} />
            <span
              class="check"
              role="checkbox"
              aria-checked={isChecked()}
              aria-label="Select"
              onClick={(e) => {
                e.preventDefault();
                e.stopPropagation();
                toggleChecked(props.c.thread, e.shiftKey);
              }}
            >
              <Icon name="check" size={12} />
            </span>
          </span>
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
            <span class="addr" style={{ color: addressColor(props.c.recipient) }} title={props.c.recipient ?? ""}>
              {local(props.c.recipient)}
            </span>
          </Show>
        </div>
      </a>
    </div>
  );
}

export function SearchField(props: { autofocus?: boolean; ref?: (el: HTMLInputElement) => void }) {
  let input!: HTMLInputElement;
  onMount(() => props.autofocus && input.focus());
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
        ref={(el) => {
          input = el;
          props.ref?.(el);
        }}
        placeholder={parts().chips.length ? "" : "Search"}
        value={text()}
        onInput={(e) => setText(e.currentTarget.value)}
        onKeyDown={(e) => {
          if (e.key === "Escape") {
            setText("");
            e.currentTarget.blur();
            closeSearch();
          }
          if (e.key === "Backspace" && !text() && parts().chips.length) remove(parts().chips[parts().chips.length - 1]);
        }}
      />
      <span class="hint-key desktop-only">/</span>
    </form>
  );
}

function SelectionBar() {
  const [moving, setMoving] = createSignal(false);
  const threads = () => [...checked()];
  const anyUnread = () => list.items.some((c) => c.unread && checked().has(c.thread));
  const all = () => list.items.length > 0 && list.items.every((c) => checked().has(c.thread));

  return (
    <div class="pane-head selection">
      <button class="btn ghost icon-btn" aria-label="Clear selection" onClick={clearChecked}>
        <Icon name="close" size={16} />
      </button>
      <span class="count">{checked().size} selected</span>
      <button class="btn ghost" onClick={() => (all() ? clearChecked() : checkAll())}>
        {all() ? "None" : "All"}
      </button>
      <div class="end">
        <Show when={searching() || folder() !== "archive"}>
          <button class="btn done" aria-label="Archive" onClick={() => void actOn(threads(), "archive")}>
            <span class="phone-only">
              <Icon name="archive" size={16} />
            </span>
            <span class="desktop-only">Archive</span>
            <span class="kbd desktop-only">e</span>
          </button>
        </Show>
        <button class="btn ghost icon-btn" aria-label={anyUnread() ? "Mark read" : "Mark unread"} title={anyUnread() ? "Mark read (u)" : "Mark unread (u)"} onClick={() => void actOn(threads(), anyUnread() ? "read" : "unread")}>
          <Icon name="mail" size={16} />
        </button>
        <button class="btn ghost icon-btn" aria-label="Move" title="Move" onClick={() => setMoving(true)}>
          <Icon name="folder" size={16} />
        </button>
        <button class="btn danger icon-btn" aria-label="Delete" title="Delete (#)" onClick={() => void actOn(threads(), "delete")}>
          <Icon name="trash" size={16} />
        </button>
      </div>
      <Show when={moving()}>
        <MoveMenu threads={threads()} onClose={() => setMoving(false)} />
      </Show>
    </div>
  );
}

export function List(props: { searchRef?: (el: HTMLInputElement) => void }) {
  const current = () => folders().find((f) => f.key === folder());
  const title = () => (searching() ? "Search" : (current()?.name ?? "Inbox"));
  let rowsEl: HTMLDivElement | undefined;
  const { pull, refreshing, handlers } = pullToRefresh(() => rowsEl);

  return (
    <section class={`list${selecting() ? " selecting" : ""}`} aria-label={title()} {...drawerSwipe(1, () => !selecting())}>
      <Show when={selecting()}>
        <SelectionBar />
      </Show>
      <Show when={!selecting() && (searchOpen() || searching())}>
        <div class="pane-head search-head phone-only">
          <button class="btn ghost icon-btn" aria-label="Close search" onClick={closeSearch}>
            <Icon name="back" size={20} />
          </button>
          <SearchField autofocus={!searching()} />
        </div>
      </Show>
      <div class="pane-head" classList={{ "phone-hidden": searchOpen() || searching() }} hidden={selecting()}>
        <button class="btn ghost icon-btn phone-only" aria-label="Folders" onClick={openDrawer}>
          <Icon name="menu" size={20} />
        </button>
        <h1>{title()}</h1>
        <Show when={!searching() && current()}>
          <span class="mono meta" style={{ "font-size": "11px" }}>
            {current()!.unread ? `${current()!.unread} unread` : ""}
          </span>
        </Show>
        <div class="head-search desktop-only">
          <SearchField ref={props.searchRef} />
        </div>
        <button class="btn ghost icon-btn phone-only" style={{ "margin-left": "auto" }} aria-label="Search" onClick={() => setSearchOpen(true)}>
          <Icon name="search" size={20} />
        </button>
      </div>
      <div class="pull" style={{ height: `${refreshing() ? PULL_AT : pull()}px` }}>
        <Icon
          name="refresh"
          size={18}
          style={refreshing() ? { animation: "spin 0.8s linear infinite" } : { transform: `rotate(${Math.min(pull(), PULL_AT) * (180 / PULL_AT)}deg)` }}
        />
      </div>
      <div
        class="rows"
        ref={rowsEl}
        {...handlers}
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
    </section>
  );
}
