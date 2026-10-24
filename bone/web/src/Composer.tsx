import { createResource, createSignal, For, onMount, Show } from "solid-js";
import { api } from "./api";
import { parseAddresses } from "./format";
import { Icon } from "./icons";
import { closeCompose, loadList, setNotice } from "./state";
import type { AddressGroup, Draft, Mailbox } from "./types";

function FromPicker(props: { firstContact: string; groups: AddressGroup[]; onPick: (address: string) => void; onClose: () => void }) {
  const [service, setService] = createSignal("");
  const [preview, setPreview] = createSignal<string | null>(null);
  const [error, setError] = createSignal<string | null>(null);
  const catchAll = () => props.groups.some((g) => g.catch_all);
  const top = () =>
    props.groups
      .flatMap((g) => g.addresses)
      .filter((a) => !a.mailbox)
      .reduce<{ address: string; count: number } | null>((best, a) => (!best || a.count > best.count ? a : best), null)?.address;

  const generate = async () => {
    setError(null);
    try {
      const { address } = await api.newAddress(service());
      setPreview(address);
    } catch (e) {
      setError(e instanceof Error ? e.message : "Couldn't make an address");
    }
  };

  return (
    <>
      <div class="scrim phone-only" onClick={props.onClose} />
      <div class="popover" role="dialog" aria-label="Send from" style={{ top: "44px", left: "56px" }}>
        <Show when={props.firstContact}>
          <div class="note">You haven't written to {props.firstContact} before.</div>
        </Show>
        <Show when={catchAll()}>
          <form
            class="new"
            onSubmit={(e) => {
              e.preventDefault();
              if (preview()) props.onPick(preview()!);
              else void generate();
            }}
          >
            <label for="new-address" class="meta" style={{ "font-size": "12px" }}>
              New address for
            </label>
            <div>
              <input
                id="new-address"
                value={service()}
                placeholder="shop or service"
                onInput={(e) => {
                  setService(e.currentTarget.value);
                  setPreview(null);
                }}
                onBlur={() => service().trim() && !preview() && void generate()}
              />
              <button class="btn primary" type="submit" disabled={!service().trim()}>
                Use
              </button>
            </div>
            <Show when={preview()}>
              <span class="addr" style={{ "font-size": "12px" }}>
                {preview()}
              </span>
            </Show>
            <Show when={error()}>
              <span class="error" style={{ "font-size": "12px" }}>
                {error()}
              </span>
            </Show>
          </form>
        </Show>
        <For each={props.groups}>
          {(g) => (
            <>
              <div class="domain">{g.domain}</div>
              <For each={g.addresses.slice(0, 8)}>
                {(a) => (
                  <button class="option" onClick={() => props.onPick(a.address)}>
                    <span class="mono">{a.address}</span>
                    <span class="hint">{a.mailbox ? "mailbox" : a.address === top() ? "most used" : ""}</span>
                  </button>
                )}
              </For>
            </>
          )}
        </For>
      </div>
    </>
  );
}

export function Composer(props: { reply?: { id: number; all: boolean }; pane?: boolean }) {
  const [draft] = createResource(
    () => props.reply,
    (r) => api.reply(r.id, r.all),
  );
  const [addresses] = createResource(() => api.addresses());
  const [from, setFrom] = createSignal("");
  const [to, setTo] = createSignal("");
  const [cc, setCc] = createSignal("");
  const [subject, setSubject] = createSignal("");
  const [text, setText] = createSignal("");
  const [picker, setPicker] = createSignal(false);
  const [suggestedFor, setSuggestedFor] = createSignal("");
  const [firstContact, setFirstContact] = createSignal("");
  const [sending, setSending] = createSignal(false);
  let body!: HTMLTextAreaElement;

  const replyDraft = (): Draft | undefined => draft();
  const recipients = (): Mailbox[] => {
    const d = replyDraft();
    return d ? d.to : parseAddresses(to());
  };
  const ccs = (): Mailbox[] => {
    const d = replyDraft();
    return d ? d.cc : parseAddresses(cc());
  };
  const sender = () => replyDraft()?.from ?? from();
  const ready = () => !!sender() && (recipients().length > 0 || ccs().length > 0) && !sending();

  onMount(() => body?.focus());

  const suggest = async () => {
    const first = parseAddresses(to())[0]?.addr ?? "";
    if (!first || from() || suggestedFor() === first) return;
    setSuggestedFor(first);
    const res = await api.addresses(first);
    if (res.suggested && !from()) setFrom(res.suggested);
    else if (!from()) {
      setFirstContact(first);
      setPicker(true);
    }
  };

  const send = async () => {
    if (!ready()) return;
    setSending(true);
    const d = replyDraft();
    try {
      const res = await api.send({
        from: sender(),
        to: recipients(),
        cc: ccs(),
        bcc: [],
        subject: d ? d.subject : subject(),
        text: text(),
        reply_to: d?.reply_to,
      });
      setNotice(res.saved ? "Sent" : `Sent, but not saved to Sent: ${res.error}`);
      closeCompose();
      void loadList();
    } catch (e) {
      setNotice(e instanceof Error ? e.message : "Couldn't send");
    } finally {
      setSending(false);
    }
  };

  return (
    <div class="composer" style={{ position: "relative" }} onKeyDown={(e) => e.key === "Enter" && (e.ctrlKey || e.metaKey) && void send()}>
      <div class="fields">
        <span>from</span>
        <Show
          when={!props.reply}
          fallback={
            <div class="locked">
              <Icon name="lock" size={12} style={{ color: "var(--meta)" }} />
              <span class="addr" style={{ "font-size": "12px" }}>
                {replyDraft()?.from ?? ""}
              </span>
            </div>
          }
        >
          <div>
            <button class={`from-btn${from() ? " set" : ""}`} onClick={() => setPicker(!picker())} aria-haspopup="dialog">
              <span style={{ flex: 1 }}>{from() || "Choose who this comes from"}</span>
              <Icon name={picker() ? "up" : "down"} />
            </button>
          </div>
        </Show>
        <span>to</span>
        <Show
          when={!props.reply}
          fallback={
            <div style={{ display: "flex", gap: "6px", "flex-wrap": "wrap" }}>
              <For each={[...recipients(), ...ccs()]}>{(m) => <span class="to-chip">{m.name || m.addr}</span>}</For>
            </div>
          }
        >
          <input class="field" aria-label="To" value={to()} onInput={(e) => setTo(e.currentTarget.value)} onBlur={() => void suggest()} />
        </Show>
        <Show when={!props.reply}>
          <span>cc</span>
          <input class="field" aria-label="Cc" value={cc()} onInput={(e) => setCc(e.currentTarget.value)} />
          <span>subject</span>
          <input class="field" aria-label="Subject" value={subject()} onInput={(e) => setSubject(e.currentTarget.value)} />
        </Show>
      </div>
      <label class="sr" for="body">
        Message
      </label>
      <textarea id="body" ref={body} rows={props.pane ? 12 : 4} placeholder={props.reply ? "Write a reply" : "Write a message"} value={text()} onInput={(e) => setText(e.currentTarget.value)} />
      <div class="foot">
        <button class="btn ghost" onClick={closeCompose}>
          Discard
        </button>
        <span class="mono meta desktop-only" style={{ "font-size": "11px" }}>
          ctrl enter
        </span>
        <button class="btn primary" disabled={!ready()} onClick={() => void send()}>
          {sending() ? "Sending" : "Send"}
        </button>
      </div>
      <Show when={picker() && addresses()}>
        <FromPicker
          firstContact={firstContact()}
          groups={addresses()!.groups}
          onPick={(a) => {
            setFrom(a);
            setPicker(false);
          }}
          onClose={() => setPicker(false)}
        />
      </Show>
    </div>
  );
}
