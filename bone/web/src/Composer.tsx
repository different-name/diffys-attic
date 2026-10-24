import { createResource, createSignal, For, onMount, Show } from "solid-js";
import { api } from "./api";
import { parseAddresses } from "./format";
import { Icon } from "./icons";
import { closeCompose, compose, loadList, setNotice } from "./state";
import type { AddressGroup, Draft, Mailbox } from "./types";

type Choice = { address: string; count: number; mailbox: boolean };

function FromPicker(props: {
  firstContact: string;
  groups: AddressGroup[];
  focus: boolean;
  onPick: (address: string) => void;
  onClose: () => void;
}) {
  const [text, setText] = createSignal("");
  const [making, setMaking] = createSignal(false);
  const [error, setError] = createSignal<string | null>(null);
  let input!: HTMLInputElement;

  const all = (): Choice[] => props.groups.flatMap((g) => g.addresses);
  const catchAll = () => props.groups.some((g) => g.catch_all);
  const top = () =>
    all()
      .filter((a) => !a.mailbox)
      .reduce<Choice | null>((best, a) => (!best || a.count > best.count ? a : best), null);
  const rest = () =>
    props.groups
      .map((g) => ({ domain: g.domain, addresses: g.addresses.filter((a) => a.address !== top()?.address) }))
      .filter((g) => g.addresses.length);
  const needle = () => text().trim().toLowerCase();
  const matches = () =>
    all()
      .filter((a) => a.address.toLowerCase().includes(needle()))
      .sort((a, b) => b.count - a.count);

  onMount(() => props.focus && input.focus());

  const make = async () => {
    setError(null);
    setMaking(true);
    try {
      const { address } = await api.newAddress(text().trim());
      props.onPick(address);
    } catch (e) {
      setError(e instanceof Error ? e.message : "Couldn't make an address");
    } finally {
      setMaking(false);
    }
  };

  const Option = (o: { choice: Choice }) => (
    <button class="option" onClick={() => props.onPick(o.choice.address)}>
      <span class="mono">{o.choice.address}</span>
      <span class="hint">{o.choice.mailbox ? "mailbox" : o.choice.address === top()?.address ? "most used" : ""}</span>
    </button>
  );

  return (
    <>
      <div class="scrim" onClick={props.onClose} />
      <div class="popover from-picker" role="dialog" aria-label="Send from">
        <div class="grip phone-only" aria-hidden="true" />
        <div class="picker-title phone-only">Send from</div>
        <Show when={props.firstContact}>
          <div class="note">You haven't written to {props.firstContact} before.</div>
        </Show>
        <form
          class="find"
          onSubmit={(e) => {
            e.preventDefault();
            const first = matches()[0];
            if (first) props.onPick(first.address);
            else if (catchAll() && needle()) void make();
          }}
        >
          <Icon name="search" size={14} />
          <input
            ref={input}
            aria-label="Find an address"
            placeholder={catchAll() ? "Find or make an address" : "Find an address"}
            value={text()}
            onInput={(e) => setText(e.currentTarget.value)}
          />
        </form>
        <div class="options">
          <Show
            when={needle()}
            fallback={
              <>
                <Show when={top()}>{(t) => <Option choice={t()} />}</Show>
                <For each={rest()}>
                  {(g) => (
                    <>
                      <div class="domain">{g.domain}</div>
                      <For each={g.addresses}>{(a) => <Option choice={a} />}</For>
                    </>
                  )}
                </For>
              </>
            }
          >
            <For each={matches()}>{(a) => <Option choice={a} />}</For>
            <Show when={catchAll()}>
              <button class="option new" disabled={making()} onClick={() => void make()}>
                <Icon name="plus" size={14} />
                <span>
                  New address for <strong>{text().trim()}</strong>
                </span>
              </button>
            </Show>
            <Show when={!catchAll() && !matches().length}>
              <div class="nothing">No address matches</div>
            </Show>
          </Show>
          <Show when={error()}>
            <div class="nothing error">{error()}</div>
          </Show>
        </div>
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
  const state = compose();
  const prefill = !props.reply && state?.kind === "new" ? (state.prefill ?? {}) : {};
  const [from, setFrom] = createSignal("");
  const [to, setTo] = createSignal(prefill.to ?? "");
  const [cc, setCc] = createSignal(prefill.cc ?? "");
  const [subject, setSubject] = createSignal(prefill.subject ?? "");
  const [text, setText] = createSignal(prefill.body ?? "");
  const [picker, setPicker] = createSignal(false);
  const [pickerFocus, setPickerFocus] = createSignal(false);
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

  onMount(() => {
    body?.focus();
    if (prefill.to) void suggest();
  });

  const suggest = async () => {
    const first = parseAddresses(to())[0]?.addr ?? "";
    if (!first || from() || suggestedFor() === first) return;
    setSuggestedFor(first);
    const res = await api.addresses(first);
    if (res.suggested && !from()) setFrom(res.suggested);
    else if (!from()) {
      setFirstContact(first);
      setPickerFocus(false);
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
            <button class={`from-btn${from() ? " set" : ""}`} onClick={() => {
                setPickerFocus(true);
                setPicker(!picker());
              }}
              aria-haspopup="dialog"
            >
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
          focus={pickerFocus()}
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
