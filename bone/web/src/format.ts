import type { Mailbox } from "./types";

const DAY = 24 * 60 * 60 * 1000;
const WEEKDAYS = ["sun", "mon", "tue", "wed", "thu", "fri", "sat"];
const MONTHS = ["jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec"];

const pad = (n: number) => String(n).padStart(2, "0");

export function when(seconds: number, now = new Date()): string {
  const d = new Date(seconds * 1000);
  const startOfToday = new Date(now.getFullYear(), now.getMonth(), now.getDate()).getTime();
  if (d.getTime() >= startOfToday) return `${pad(d.getHours())}:${pad(d.getMinutes())}`;
  if (d.getTime() >= startOfToday - DAY) return "yesterday";
  if (d.getTime() >= startOfToday - 6 * DAY) return WEEKDAYS[d.getDay()];
  if (d.getFullYear() === now.getFullYear()) return `${MONTHS[d.getMonth()]} ${d.getDate()}`;
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
}

export function fullWhen(seconds: number): string {
  const d = new Date(seconds * 1000);
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${pad(d.getHours())}:${pad(d.getMinutes())}`;
}

export const local = (address: string | null) => (address ? `${address.split("@")[0]}@` : "");

export const who = (m: Mailbox | null) => (m ? m.name || m.addr : "Unknown sender");

export function initials(m: Mailbox | null): string {
  const name = who(m);
  const parts = name.split(/[\s@.]+/).filter(Boolean);
  return (parts[0]?.[0] ?? "?").concat(parts[1]?.[0] ?? "").toUpperCase();
}

export function size(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${Math.round(bytes / 1024)} KB`;
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
}

export function parseAddresses(input: string): Mailbox[] {
  return input
    .split(/[,;\n]/)
    .map((part) => part.trim())
    .filter(Boolean)
    .map((part) => {
      const angle = part.match(/^(.*)<([^>]+)>$/);
      if (angle) return { name: angle[1].trim().replace(/^"|"$/g, "") || null, addr: angle[2].trim() };
      return { name: null, addr: part };
    });
}

export const OPERATORS = ["from:", "to:", "subject:", "in:", "is:", "has:", "before:", "after:"];

export function splitQuery(q: string): { chips: string[]; text: string } {
  const chips: string[] = [];
  const words: string[] = [];
  for (const token of q.split(/\s+/).filter(Boolean)) {
    if (OPERATORS.some((op) => token.toLowerCase().startsWith(op) && token.length > op.length)) chips.push(token);
    else words.push(token);
  }
  return { chips, text: words.join(" ") };
}
