import { createSignal } from "solid-js";
import { createStore, produce } from "solid-js/store";
import { api } from "./api";
import type { Conversation, Folder, Op } from "./types";

export type Pane = "list" | "thread" | "compose";
export type Prefill = { to?: string; cc?: string; subject?: string; body?: string };
export type ComposeState = { kind: "new"; prefill?: Prefill } | { kind: "reply"; id: number; all: boolean };

export const [folders, setFolders] = createSignal<Folder[]>([]);
export const [folder, setFolder] = createSignal("inbox");
export const [query, setQuery] = createSignal("");
export const [selected, setSelected] = createSignal<number | null>(null);
export const [pane, setPane] = createSignal<Pane>("list");
export const [compose, setCompose] = createSignal<ComposeState | null>(null);
export const [notice, setNotice] = createSignal<string | null>(null);
export const [checked, setChecked] = createSignal<ReadonlySet<number>>(new Set());
export const [searchOpen, setSearchOpen] = createSignal(false);
export const [domainColors, setDomainColors] = createSignal<ReadonlyMap<string, string> | null>(null);

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

export async function loadDomains() {
  const domains = await api.domains();
  setDomainColors(new Map(domains.map((d) => [d.domain, d.color])));
}

export function addressColor(address: string | null | undefined): string | undefined {
  const colors = domainColors();
  if (!colors || !address) return undefined;
  const domain = address.slice(address.lastIndexOf("@") + 1).toLowerCase();
  return `var(--ctp-${colors.get(domain) ?? "red"})`;
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

let anchor: number | null = null;

export function clearChecked() {
  anchor = null;
  setChecked(new Set<number>());
}

export function toggleChecked(thread: number, range = false) {
  const next = new Set(checked());
  const ids = list.items.map((c) => c.thread);
  const from = anchor === null ? -1 : ids.indexOf(anchor);
  const to = ids.indexOf(thread);
  if (range && from >= 0 && to >= 0) {
    for (const t of ids.slice(Math.min(from, to), Math.max(from, to) + 1)) next.add(t);
  } else if (next.has(thread)) {
    next.delete(thread);
  } else {
    next.add(thread);
  }
  anchor = thread;
  setChecked(next);
}

export function checkAll() {
  setChecked(new Set(list.items.map((c) => c.thread)));
}

export function closeSearch() {
  setSearchOpen(false);
  if (query()) search("");
}

export function openFolder(key: string) {
  clearChecked();
  setSearchOpen(false);
  setQuery("");
  setFolder(key);
  setSelected(null);
  setCompose(null);
  setPane("list");
  void loadList();
}

export function search(q: string) {
  clearChecked();
  if (q.trim()) setSearchOpen(true);
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

function neighbour(thread: number, gone: ReadonlySet<number>): number | null {
  const items = list.items;
  const i = items.findIndex((c) => c.thread === thread);
  if (i < 0) return null;
  const after = items.slice(i + 1).find((c) => !gone.has(c.thread));
  const before = items.slice(0, i).reverse().find((c) => !gone.has(c.thread));
  return (after ?? before)?.thread ?? null;
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

export function act(thread: number, op: Op, target?: string) {
  return actOn([thread], op, target);
}

export async function actOn(threads: number[], op: Op, target?: string) {
  const ids = (await Promise.all(threads.map((t) => threadIds(t, op)))).flat();
  if (!ids.length) return;
  const gone = new Set(threads);
  const leaves = op === "archive" || op === "delete" || op === "junk" || op === "move" || op === "inbox";
  if (leaves) {
    const open = selected();
    const after = open !== null && gone.has(open) ? neighbour(open, gone) : open;
    for (const t of threads) leaving.set(t, { from: listFolder(), until: Date.now() + 60_000 });
    setList("items", (items) => items.filter((c) => !gone.has(c.thread)));
    setChecked((c) => new Set([...c].filter((t) => !gone.has(t))));
    if (after !== open) {
      setSelected(after);
      if (after === null) setPane("list");
    }
  } else if (op === "read" || op === "unread") {
    setList("items", (c) => gone.has(c.thread), "unread", op === "unread");
  }
  try {
    await api.act(ids, op, target);
  } catch (e) {
    for (const t of threads) leaving.delete(t);
    setNotice(e instanceof Error ? e.message : "That didn't work");
    void loadList();
  }
  void loadFolders();
}
