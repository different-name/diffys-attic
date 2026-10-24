import { createSignal } from "solid-js";
import { createStore, produce } from "solid-js/store";
import { api } from "./api";
import type { Conversation, Folder, Op } from "./types";

export type Pane = "list" | "thread" | "compose";
export type ComposeState = { kind: "new" } | { kind: "reply"; id: number; all: boolean };

export const [folders, setFolders] = createSignal<Folder[]>([]);
export const [folder, setFolder] = createSignal("inbox");
export const [query, setQuery] = createSignal("");
export const [selected, setSelected] = createSignal<number | null>(null);
export const [pane, setPane] = createSignal<Pane>("list");
export const [compose, setCompose] = createSignal<ComposeState | null>(null);
export const [notice, setNotice] = createSignal<string | null>(null);

export const [list, setList] = createStore({
  items: [] as Conversation[],
  next: null as string | null,
  loading: false,
  loaded: false,
});

let generation = 0;
const leaving = new Map<number, { from: string | undefined; until: number }>();

export const searching = () => query().trim() !== "";
const listFolder = () => (searching() ? undefined : folder());

// the server keeps a moved thread in place until the imap move syncs
function settle(items: Conversation[], whole: boolean): Conversation[] {
  const now = Date.now();
  const from = listFolder();
  const present = new Set(items.map((c) => c.thread));
  for (const [thread, l] of leaving) {
    if (l.until < now || (whole && l.from === from && !present.has(thread))) leaving.delete(thread);
  }
  return items.filter((c) => {
    const l = leaving.get(c.thread);
    return !l || l.from !== from;
  });
}

export async function loadFolders() {
  setFolders(await api.folders());
}

export async function loadList() {
  const gen = ++generation;
  setList("loading", true);
  try {
    const page = await api.conversations(listFolder(), query());
    if (gen !== generation) return;
    setList({ items: settle(page.conversations, !page.next), next: page.next, loaded: true });
  } finally {
    if (gen === generation) setList("loading", false);
  }
}

export async function loadMore() {
  if (!list.next || list.loading) return;
  const gen = generation;
  setList("loading", true);
  try {
    const page = await api.conversations(listFolder(), query(), list.next);
    if (gen !== generation) return;
    setList(
      produce((s) => {
        s.items.push(...settle(page.conversations, false));
        s.next = page.next;
      }),
    );
  } finally {
    if (gen === generation) setList("loading", false);
  }
}

export function openFolder(key: string) {
  setQuery("");
  setFolder(key);
  setSelected(null);
  setCompose(null);
  setPane("list");
  void loadList();
}

export function search(q: string) {
  setQuery(q);
  setSelected(null);
  setPane("list");
  void loadList();
}

export function open(thread: number) {
  setSelected(thread);
  setCompose(null);
  setPane("thread");
}

export function startCompose(state: ComposeState) {
  setCompose(state);
  setPane(state.kind === "reply" ? "thread" : "compose");
}

export function closeCompose() {
  const was = compose();
  setCompose(null);
  setPane(was?.kind === "reply" && selected() !== null ? "thread" : "list");
}

function neighbour(thread: number): number | null {
  const i = list.items.findIndex((c) => c.thread === thread);
  if (i < 0) return null;
  return (list.items[i + 1] ?? list.items[i - 1])?.thread ?? null;
}

export function move(step: 1 | -1) {
  const items = list.items;
  if (!items.length) return;
  const i = items.findIndex((c) => c.thread === selected());
  const next = items[Math.min(items.length - 1, Math.max(0, i < 0 ? 0 : i + step))];
  open(next.thread);
}

export async function threadIds(thread: number, op: Op): Promise<number[]> {
  const rows = await api.thread(thread);
  const scope = searching() ? undefined : folder();
  const inScope = rows.filter((r) => (scope ? r.folder === scope : r.folder === "inbox"));
  if (op === "read" || op === "unread" || op === "flag" || op === "unflag") return (inScope.length ? inScope : rows).map((r) => r.id);
  return inScope.map((r) => r.id);
}

export async function act(thread: number, op: Op, target?: string) {
  const ids = await threadIds(thread, op);
  if (!ids.length) return;
  const leaves = op === "archive" || op === "delete" || op === "junk" || op === "move" || op === "inbox";
  const after = leaves ? neighbour(thread) : thread;
  if (leaves) {
    leaving.set(thread, { from: listFolder(), until: Date.now() + 60_000 });
    setList("items", (items) => items.filter((c) => c.thread !== thread));
    if (selected() === thread) {
      setSelected(after);
      if (after === null) setPane("list");
    }
  } else {
    const unread = op === "unread";
    if (op === "read" || op === "unread") setList("items", (c) => c.thread === thread, "unread", unread);
  }
  try {
    await api.act(ids, op, target);
  } catch (e) {
    leaving.delete(thread);
    setNotice(e instanceof Error ? e.message : "That didn't work");
    void loadList();
  }
  void loadFolders();
}
