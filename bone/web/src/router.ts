import { batch, createEffect, onCleanup } from "solid-js";
import { compose, folder, loadList, pane, query, selected, setCompose, setFolder, setPane, setQuery, setSelected } from "./state";

export type Route = { folder: string; query: string; thread: number | null; compose: boolean };

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

function parse(): Route {
  const [first, second] = location.pathname.split("/").filter(Boolean).map(decodeURIComponent);
  const thread = second && /^\d+$/.test(second) ? Number(second) : null;
  if (first === "compose") return { folder: folder(), query: "", thread: null, compose: true };
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
    setFolder(r.folder);
    setQuery(r.query);
    setSelected(r.thread);
    setCompose(r.compose ? { kind: "new" } : null);
    setPane(r.compose ? "compose" : r.thread !== null ? "thread" : "list");
  });
  return reload;
}

export function useRouter() {
  apply(parse());
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
    if (apply(parse())) void loadList();
  };
  window.addEventListener("popstate", pop);
  onCleanup(() => window.removeEventListener("popstate", pop));
}
