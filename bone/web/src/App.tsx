import { createEffect, onCleanup, onMount, Show } from "solid-js";
import { listen, online, token } from "./api";
import { Composer } from "./Composer";
import { closeDrawer, Drawer, drawerOpen } from "./Drawer";
import { Icon } from "./icons";
import { List } from "./List";
import { Sidebar } from "./Sidebar";
import { SignIn } from "./SignIn";
import {
  act,
  actOn,
  checked,
  clearChecked,
  closeSearch,
  compose,
  folder,
  loadDomains,
  loadFolders,
  loadList,
  move,
  notice,
  open,
  pane,
  searchOpen,
  searching,
  selected,
  setNotice,
  setCompose,
  setPane,
  startCompose,
  list,
  toggleChecked,
} from "./state";
import { useRouter } from "./router";
import { bumpThread, Thread } from "./Thread";

function typing(e: KeyboardEvent) {
  const el = e.target as HTMLElement | null;
  return !!el && (el.tagName === "INPUT" || el.tagName === "TEXTAREA" || el.isContentEditable);
}

function Mail() {
  let searchInput: HTMLInputElement | undefined;
  let refresh: number | undefined;

  useRouter();
  onMount(() => {
    void loadFolders();
    void loadList();
    void loadDomains();
  });

  const stop = listen((changed) => {
    window.clearTimeout(refresh);
    refresh = window.setTimeout(() => {
      void loadFolders();
      if (!changed.length || searching() || changed.includes(folder())) void loadList();
      if (selected() !== null) bumpThread((n) => n + 1);
    }, 300);
  });
  onCleanup(stop);

  createEffect(() => {
    if (!notice()) return;
    const t = window.setTimeout(() => setNotice(null), 4000);
    onCleanup(() => window.clearTimeout(t));
  });

  const key = (e: KeyboardEvent) => {
    if (e.ctrlKey || e.metaKey || e.altKey || typing(e)) return;
    const thread = selected();
    const latest = list.items.find((c) => c.thread === thread);
    switch (e.key) {
      case "j":
        move(1);
        break;
      case "k":
        move(-1);
        break;
      case "/":
        searchInput?.focus();
        break;
      case "c":
        startCompose({ kind: "new" });
        break;
      case "Escape":
        if (checked().size) clearChecked();
        else if (pane() !== "list") setPane("list");
        return;
      case "x":
        if (thread === null) return;
        toggleChecked(thread);
        break;
      default:
        if (checked().size) {
          const threads = [...checked()];
          const unread = list.items.some((c) => c.unread && checked().has(c.thread));
          if (e.key === "e") void actOn(threads, "archive");
          else if (e.key === "u") void actOn(threads, unread ? "read" : "unread");
          else if (e.key === "#") void actOn(threads, "delete");
          else return;
          break;
        }
        if (thread === null) return;
        if (e.key === "e") void act(thread, "archive");
        else if (e.key === "u") void act(thread, latest?.unread ? "read" : "unread");
        else if (e.key === "#") void act(thread, "delete");
        else if ((e.key === "r" || e.key === "a") && latest) startCompose({ kind: "reply", id: latest.id, all: e.key === "a" });
        else if (e.key === "Enter") open(thread);
        else return;
    }
    e.preventDefault();
  };
  document.addEventListener("keydown", key);
  onCleanup(() => document.removeEventListener("keydown", key));

  window.boneBack = () => {
    if (drawerOpen()) closeDrawer();
    else if (checked().size) clearChecked();
    else if (compose()?.kind === "reply") setCompose(null);
    else if (searchOpen() || searching()) closeSearch();
    else return false;
    return true;
  };
  onCleanup(() => (window.boneBack = undefined));

  return (
    <div class="app">
      <Show when={!online()}>
        <div class="banner" role="status">
          <Icon name="offline" />
          <span>Can't reach bone.</span>
        </div>
      </Show>
      <div class="panes" data-pane={pane()}>
        <Sidebar />
        <List searchRef={(el) => (searchInput = el)} />
        <Show
          when={compose()?.kind === "new"}
          fallback={
            <Show when={selected()} fallback={<section class="reader" aria-label="Reading pane" />}>
              {(thread) => <Thread thread={thread()} />}
            </Show>
          }
        >
          <section class="reader compose-pane" aria-label="New message">
            <div class="toolbar">
              <button class="btn ghost icon-btn phone-only" aria-label="Discard" onClick={() => setPane("list")}>
                <Icon name="close" size={20} />
              </button>
              <h2 style={{ margin: 0, "font-size": "14px", "font-weight": 600 }}>New message</h2>
            </div>
            <Composer pane />
          </section>
        </Show>
        <Drawer />
        <button class="fab" onClick={() => startCompose({ kind: "new" })}>
          <Icon name="pencil" size={18} />
          Compose
        </button>
      </div>
      <Show when={notice()}>
        <div class="notice" role="status">
          {notice()}
        </div>
      </Show>
    </div>
  );
}

export function App() {
  return (
    <Show when={token()} fallback={<SignIn />}>
      <Mail />
    </Show>
  );
}
