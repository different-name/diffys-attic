import { createSignal } from "solid-js";
import type { AddressGroup, Conversation, Draft, Folder, Message, Op, Outgoing, Row, SavedDraft } from "./types";

const KEY = "bone.token";

function stored(): string {
  try {
    return localStorage.getItem(KEY) ?? "";
  } catch {
    return "";
  }
}

declare global {
  interface Window {
    BoneAndroid?: { setToken(token: string): void };
    boneBack?: () => boolean;
  }
}

export const [token, setTokenSignal] = createSignal(stored());
window.BoneAndroid?.setToken(token());
export const [online, setOnline] = createSignal(true);

export function setToken(value: string) {
  try {
    if (value) localStorage.setItem(KEY, value);
    else localStorage.removeItem(KEY);
  } catch {}
  setTokenSignal(value);
  window.BoneAndroid?.setToken(value);
}

export class ApiError extends Error {
  constructor(
    readonly status: number,
    message: string,
  ) {
    super(message);
  }
}

async function request<T>(path: string, init: RequestInit = {}): Promise<T> {
  const headers: Record<string, string> = { Authorization: `Bearer ${token()}` };
  if (init.body) headers["Content-Type"] = "application/json";
  let res: Response;
  try {
    res = await fetch(path, { ...init, headers });
  } catch (e) {
    setOnline(false);
    throw e;
  }
  setOnline(true);
  if (res.status === 401) {
    setToken("");
    throw new ApiError(401, "sign in again");
  }
  if (!res.ok) {
    let message = res.statusText;
    try {
      message = (await res.json()).error ?? message;
    } catch {}
    throw new ApiError(res.status, message);
  }
  return res.json() as Promise<T>;
}

const qs = (params: Record<string, string | undefined>) => {
  const p = new URLSearchParams();
  for (const [k, v] of Object.entries(params)) if (v) p.set(k, v);
  const s = p.toString();
  return s ? `?${s}` : "";
};

export const api = {
  folders: () => request<Folder[]>("/api/folders"),
  conversations: (folder: string | undefined, q: string, before?: string) =>
    request<{ conversations: Conversation[]; next: string | null }>(
      `/api/conversations${qs({ folder, q, before })}`,
    ),
  thread: (id: number) => request<Row[]>(`/api/threads/${id}`),
  message: (id: number) => request<Message>(`/api/messages/${id}`),
  act: (ids: number[], op: Op, folder?: string) =>
    request<{ done: number }>("/api/actions", { method: "POST", body: JSON.stringify({ ids, op, folder }) }),
  reply: (id: number, all: boolean) => request<Draft>(`/api/messages/${id}/reply${all ? "?all=1" : ""}`),
  saveDraft: (body: Outgoing) =>
    request<{ draft: string }>("/api/drafts", { method: "POST", body: JSON.stringify(body) }),
  discardDraft: (draft: string) =>
    request<{ ok: boolean }>("/api/drafts/discard", { method: "POST", body: JSON.stringify({ draft }) }),
  draft: (draft: string) => request<SavedDraft>(`/api/draft${qs({ id: draft })}`),
  draftOf: (id: number) => request<SavedDraft>(`/api/messages/${id}/draft`),
  send: (body: Outgoing) =>
    request<{ sent: boolean; saved: boolean; error: string | null }>("/api/send", {
      method: "POST",
      body: JSON.stringify(body),
    }),
  addresses: (to?: string) =>
    request<{ groups: AddressGroup[]; suggested: string | null }>(`/api/addresses${qs({ to })}`),
  domains: () => request<{ domain: string; color: string }[]>("/api/domains"),
  newAddress: (service: string) => request<{ address: string }>(`/api/addresses/new${qs({ service })}`),
};

export function withToken(path: string): string {
  return `${path}${path.includes("?") ? "&" : "?"}token=${encodeURIComponent(token())}`;
}

let source: EventSource | undefined;
let retry: number | undefined;
let active: ((folders: string[]) => void) | undefined;

function open() {
  if (!active) return;
  const onChanged = active;
  source = new EventSource(withToken("/api/events"));
  source.onopen = () => setOnline(true);
  source.addEventListener("changed", (e) => onChanged(JSON.parse((e as MessageEvent).data).folders));
  source.addEventListener("resync", () => onChanged([]));
  source.onerror = () => {
    setOnline(false);
    source?.close();
    retry = window.setTimeout(open, 5000);
  };
}

export function listen(onChanged: (folders: string[]) => void): () => void {
  active = onChanged;
  open();
  return () => {
    active = undefined;
    source?.close();
    window.clearTimeout(retry);
  };
}

export function reconnect() {
  window.clearTimeout(retry);
  source?.close();
  open();
}
