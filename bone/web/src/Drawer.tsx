import { createSignal, onCleanup } from "solid-js";
import { Sidebar } from "./Sidebar";

const OPEN_AT = 0.3;

const [open, setOpen] = createSignal(false);
const [drag, setDrag] = createSignal<number | null>(null);

export const openDrawer = () => setOpen(true);
export const drawerOpen = open;
export const closeDrawer = () => setOpen(false);

const width = () => Math.min(300, window.innerWidth * 0.82);
const shown = () => drag() ?? (open() ? width() : 0);

export function drawerSwipe(direction: 1 | -1, enabled: () => boolean = () => true) {
  let start: { x: number; y: number; id: number } | null = null;
  let active = false;
  let decided = false;

  const up = () => {
    if (active) {
      const d = drag() ?? 0;
      setOpen(d > width() * (direction === 1 ? OPEN_AT : 1 - OPEN_AT));
    }
    setDrag(null);
    start = null;
    active = false;
  };

  return {
    onPointerDown(e: PointerEvent) {
      if (e.pointerType === "mouse" || !enabled() || !matchMedia("(max-width: 760px)").matches) return;
      start = { x: e.clientX, y: e.clientY, id: e.pointerId };
      active = false;
      decided = false;
    },
    onPointerMove(e: PointerEvent) {
      if (!start || e.pointerId !== start.id) return;
      const dx = e.clientX - start.x;
      const dy = e.clientY - start.y;
      if (!decided) {
        if (Math.abs(dx) < 10 && Math.abs(dy) < 10) return;
        decided = true;
        active = Math.abs(dx) > Math.abs(dy) && Math.sign(dx) === direction;
      }
      if (!active) return;
      const from = direction === 1 ? 0 : width();
      setDrag(Math.max(0, Math.min(width(), from + dx)));
    },
    onPointerUp: up,
    onPointerCancel: up,
  };
}

export function Drawer() {
  const close = drawerSwipe(-1);
  const key = (e: KeyboardEvent) => e.key === "Escape" && open() && setOpen(false);
  document.addEventListener("keydown", key);
  onCleanup(() => document.removeEventListener("keydown", key));

  return (
    <div class="drawer-layer" classList={{ live: shown() > 0, dragging: drag() !== null }} {...close}>
      <div class="drawer-scrim" style={{ opacity: shown() / width() }} onClick={() => setOpen(false)} />
      <div class="drawer" style={{ width: `${width()}px`, transform: `translateX(${shown() - width()}px)` }}>
        <Sidebar drawer onPick={() => setOpen(false)} />
      </div>
    </div>
  );
}
