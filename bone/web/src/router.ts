import { batch, createEffect, onCleanup } from "solid-js";
import {
  type Prefill,
  clearChecked,
  compose,
  folder,
  loadList,
  pane,
  query,
  selected,
  setCompose,
  setFolder,
  setPane,
  setQuery,
  setSelected,
} from "./state";

export type Route = { folder: string; query: string; thread: number | null; compose: boolean; prefill?: Prefill };

export function href(r: Route): string {
  if (r.compose) return "/compose";
  let path = r.query ? "/search" : `/${encodeURIComponent(r.folder)}`;
  if (r.thread !== null) path += `/${r.thread}`;
  if (path === "/inbox") path = "/";
  return r.query ? `${path}?q=${encodeURIComponent(r.query)}` : path;
}

export function threadHref(thread: number): string {
  return href({ folder: folder(), query: query(), thread, compose: false });
}

function prefillFrom(search: string): Prefill | undefined {
  const params = new URLSearchParams(search);
  const prefill: Prefill = {};
  for (const key of ["to", "cc", "subject", "body"] as const) {
    const value = params.get(key);
    if (value) prefill[key] = value;
  }
  return Object.keys(prefill).length ? prefill : undefined;
}

function parse(): Route {
  const [first, second] = location.pathname.split("/").filter(Boolean).map(decodeURIComponent);
  const thread = second && /^\d+$/.test(second) ? Number(second) : null;
  if (first === "compose") return { folder: folder(), query: "", thread: null, compose: true, prefill: prefillFrom(location.search) };
  if (first === "search") return { folder: folder(), query: new URLSearchParams(location.search).get("q") ?? "", thread, compose: false };
  return { folder: first ?? "inbox", query: "", thread, compose: false };
}

function current(): Route {
  return {
    folder: folder(),
    query: query(),
    thread: pane() === "list" ? null : selected(),
    compose: compose()?.kind === "new",
  };
}

function apply(r: Route): boolean {
  const reload = r.folder !== folder() || r.query !== query();
  batch(() => {
    if (reload) clearChecked();
    setFolder(r.folder);
    setQuery(r.query);
    setSelected(r.thread);
    setCompose(r.compose ? { kind: "new", prefill: r.prefill } : null);
    setPane(r.compose ? "compose" : r.thread !== null ? "thread" : "list");
  });
  return reload;
}

function dropPrefill() {
  if (location.pathname === "/compose" && location.search) history.replaceState(null, "", "/compose");
}

export function useRouter() {
  const first = parse();
  dropPrefill();
  apply(first);
  let last = current();

  createEffect(() => {
    const r = current();
    const url = href(r);
    if (url !== location.pathname + location.search) {
      const sameList = r.folder === last.folder && r.query === last.query && !r.compose && !last.compose;
      if (sameList && r.thread !== null && last.thread !== null) history.replaceState(null, "", url);
      else history.pushState(null, "", url);
    }
    last = r;
  });

  const pop = () => {
    const r = parse();
    dropPrefill();
    if (apply(r)) void loadList();
  };
  window.addEventListener("popstate", pop);
  onCleanup(() => window.removeEventListener("popstate", pop));
}
