import type { JSX } from "solid-js";
import { Dynamic } from "solid-js/web";
import {
  Archive,
  Check,
  ChevronDown,
  ChevronLeft,
  ChevronUp,
  File,
  Folder,
  Inbox,
  Lock,
  Mail,
  Menu,
  Pencil,
  Plus,
  Search,
  Trash2,
  WifiOff,
  X,
} from "lucide-solid";

const icons = {
  plus: Plus,
  search: Search,
  archive: Archive,
  trash: Trash2,
  mail: Mail,
  folder: Folder,
  lock: Lock,
  back: ChevronLeft,
  close: X,
  check: Check,
  menu: Menu,
  down: ChevronDown,
  up: ChevronUp,
  file: File,
  offline: WifiOff,
  pencil: Pencil,
  inbox: Inbox,
};

export type IconName = keyof typeof icons;

export function Icon(props: { name: IconName; size?: number; label?: string; style?: JSX.CSSProperties }) {
  return (
    <Dynamic
      component={icons[props.name]}
      size={props.size ?? 14}
      stroke-width={2}
      style={props.style}
      role={props.label ? "img" : undefined}
      aria-label={props.label}
      aria-hidden={props.label ? undefined : "true"}
    />
  );
}
