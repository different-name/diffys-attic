import { createEffect, createResource, createSignal, For, onCleanup, Show } from "solid-js";
import { api, token, withToken } from "./api";
import { fullWhen, local, size, who } from "./format";
import { Icon } from "./icons";
import { addressColor } from "./state";
import type { Message as MessageData } from "./types";

// firefox doesn't resolve 'self' to the parent origin inside srcdoc
const FRAME_CSP = `default-src 'none'; img-src ${location.origin} data:; style-src 'unsafe-inline'`;

function frameDoc(html: string): string {
  const authed = html.replace(/(\/api\/messages\/\d+\/cid\/[^"']+)/g, (path) => withToken(path));
  return `<!doctype html><html><head><meta charset="utf-8"><meta http-equiv="Content-Security-Policy" content="${FRAME_CSP}"><base target="_blank"><style>html,body{margin:0}body{padding:16px;font:14px/1.5 system-ui,sans-serif;color:#1e1e2e;background:#fff;overflow-wrap:anywhere}img{max-width:100%;height:auto}table{max-width:100%}</style></head><body>${authed}</body></html>`;
}

function fitWidth(doc: Document) {
  const root = doc.documentElement;
  for (let pass = 0; pass < 4 && root.scrollWidth > root.clientWidth + 1; pass++) {
    const limit = doc.body.clientWidth;
    for (const el of doc.body.querySelectorAll<HTMLElement>("*")) {
      if (el.getBoundingClientRect().width <= limit) continue;
      el.removeAttribute("width");
      el.style.setProperty("min-width", "0", "important");
      el.style.setProperty("max-width", "100%", "important");
      el.style.setProperty("width", el.tagName === "TABLE" ? "100%" : "auto", "important");
    }
  }
}

const PHONE_FRAME = 500;
const MIN_SCALE = 0.7;

// phones lay wide mail out wider and scale it down rather than squeezing it into one column
function scaleFor(doc: Document, width: number): number {
  const natural = doc.documentElement.scrollWidth;
  if (width >= PHONE_FRAME || natural <= width) return 1;
  return Math.max(MIN_SCALE, width / natural);
}

function HtmlFrame(props: { html: string }) {
  let wrap!: HTMLDivElement;
  let frame!: HTMLIFrameElement;
  const [height, setHeight] = createSignal(120);
  const [scale, setScale] = createSignal(1);
  let observer: ResizeObserver | undefined;

  const measure = () => {
    const doc = frame.contentDocument;
    if (!doc?.body) return;
    fitWidth(doc);
    setHeight(doc.body.scrollHeight);
  };
  let observed: Document | undefined;
  const watch = () => {
    const doc = frame.contentDocument;
    if (!doc?.body || doc.readyState === "loading" || doc === observed) return;
    observed = doc;
    setScale(scaleFor(doc, wrap.clientWidth));
    observer?.disconnect();
    observer = new ResizeObserver(measure);
    observer.observe(doc.body);
    measure();
  };
  const loaded = () => {
    watch();
    measure();
  };
  // a slow remote image holds back the load event, so start measuring once the document is parsed
  const poll = window.setInterval(watch, 100);
  onCleanup(() => {
    window.clearInterval(poll);
    observer?.disconnect();
  });

  return (
    <div ref={wrap} class="html-frame" style={{ height: `${Math.ceil(height() * scale())}px` }}>
      <iframe
        ref={frame}
        title="Message"
        sandbox="allow-same-origin allow-popups allow-popups-to-escape-sandbox"
        referrerpolicy="no-referrer"
        srcdoc={frameDoc(props.html)}
        style={{
          width: `${100 / scale()}%`,
          height: `${height()}px`,
          transform: scale() === 1 ? undefined : `scale(${scale()})`,
        }}
        onLoad={loaded}
      />
    </div>
  );
}

export function MessageView(props: { id: number; onLoaded?: (m: MessageData) => void }) {
  const [message] = createResource(() => [props.id, token()] as const, ([id]) => api.message(id));
  const [plain, setPlain] = createSignal(false);

  createEffect(() => {
    const m = message();
    if (m) props.onLoaded?.(m);
  });

  return (
    <Show when={message()} fallback={<div class="message meta">{message.error ? "Couldn't load this message." : ""}</div>}>
      {(m) => (
        <div class="message">
          <div class="message-head">
            <span class="name">{who(m().from)}</span>
            <Show when={m().from?.name}>
              <span class="meta">{m().from?.addr}</span>
            </Show>
            <Show when={m().recipient}>
              <span style={{ display: "inline-flex", gap: "8px", "align-items": "baseline" }}>
                <span class="meta">→</span>
                <span class="addr" style={{ "font-size": "12px", color: addressColor(m().recipient) }} title={m().recipient ?? ""}>
                  {m().recipient}
                </span>
              </span>
            </Show>
            <Show when={m().cc.length}>
              <span class="meta">cc {m().cc.map((c) => c.name || local(c.addr)).join(", ")}</span>
            </Show>
            <span class="time" title={fullWhen(m().date)}>
              {fullWhen(m().date)}
            </span>
          </div>
          <Show when={m().attachment_list.length}>
            <div class="files">
              <For each={m().attachment_list}>
                {(a, i) => (
                  <a class="file" href={withToken(`/api/messages/${m().id}/attachments/${i()}`)} download={a.name}>
                    <Icon name="file" size={16} style={{ color: "var(--meta)" }} />
                    <span>{a.name}</span>
                    <span class="mono meta" style={{ "font-size": "11px" }}>
                      {size(a.size)}
                    </span>
                  </a>
                )}
              </For>
            </div>
          </Show>
          <Show when={m().html && m().text}>
            <div class="toggle" role="group" aria-label="Show as" style={{ "align-self": "flex-start" }}>
              <button class={plain() ? "" : "on"} aria-pressed={!plain()} onClick={() => setPlain(false)}>
                Formatted
              </button>
              <button class={plain() ? "on" : ""} aria-pressed={plain()} onClick={() => setPlain(true)}>
                Plain text
              </button>
            </div>
          </Show>
          <Show when={m().html && !plain()} fallback={<div class="body-text">{m().text ?? ""}</div>}>
            <HtmlFrame html={m().html!} />
          </Show>
        </div>
      )}
    </Show>
  );
}
