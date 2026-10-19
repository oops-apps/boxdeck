import * as Dialog from "@radix-ui/react-dialog";
import * as Dropdown from "@radix-ui/react-dropdown-menu";
import * as Tooltip from "@radix-ui/react-tooltip";
import { forwardRef, useState, type ButtonHTMLAttributes, type ReactNode } from "react";

export function cn(...classes: (string | false | null | undefined)[]): string {
  return classes.filter(Boolean).join(" ");
}

const ICONS = {
  play: "M5 3.5v9l7.5-4.5z",
  stop: "M4 4h8v8H4z",
  pause: "M4.5 3.5h2.5v9H4.5zM9 3.5h2.5v9H9z",
  terminal: "M2.5 3.5h11v9h-11zM4.5 6l2 2-2 2M8 10.5h3",
  prompt: "M3 4.5 6.5 8 3 11.5M8.5 11.5H13",
  more: "M4 8h.01M8 8h.01M12 8h.01",
  refresh: "M12.5 6A4.75 4.75 0 1 0 13 9.5M12.5 2.5V6H9",
  search: "M7 12A5 5 0 1 0 7 2a5 5 0 0 0 0 10zM10.5 10.5 14 14",
  folder: "M2 4.5h4l1.5 1.5H14v6.5H2z",
  close: "M4 4l8 8M12 4l-8 8",
  chevron: "M6 4l4 4-4 4",
  list: "M3 4.5h10M3 8h10M3 11.5h10",
  activity: "M1.5 8h3l2-5 3 10 2-5h3",
  settings:
    "M8 10a2 2 0 1 0 0-4 2 2 0 0 0 0 4zM8 1.5v2M8 12.5v2M1.5 8h2M12.5 8h2M3.4 3.4l1.4 1.4M11.2 11.2l1.4 1.4M3.4 12.6l1.4-1.4M11.2 4.8l1.4-1.4",
  warning: "M8 2 14.5 13.5h-13zM8 6.5v3M8 11.5h.01",
  box: "M2.5 5 8 2.5 13.5 5v6L8 13.5 2.5 11zM2.5 5 8 7.5 13.5 5M8 7.5v6",
  monitor:
    "M3.3 3h9.4c.7 0 1.3.6 1.3 1.3v5.4c0 .7-.6 1.3-1.3 1.3H3.3C2.6 11 2 10.4 2 9.7V4.3C2 3.6 2.6 3 3.3 3zM5.5 13.5h5M8 11v2.5",
  stack: "M2 5.5 8 2.5l6 3-6 3zM2 8.5l6 3 6-3M2 11.5l6 3 6-3",
  plus: "M8 3v10M3 8h10",
} as const;

export type IconName = keyof typeof ICONS;

export function Icon({ name, className }: { name: IconName; className?: string }) {
  const filled = name === "play" || name === "stop" || name === "pause";
  return (
    <svg
      viewBox="0 0 16 16"
      aria-hidden="true"
      className={cn("size-4 shrink-0", className)}
      fill={filled ? "currentColor" : "none"}
      stroke={filled ? "none" : "currentColor"}
      strokeWidth={1.5}
      strokeLinecap="round"
      strokeLinejoin="round"
    >
      <path d={ICONS[name]} />
    </svg>
  );
}

type Variant = "default" | "primary" | "danger" | "ghost";

const VARIANTS: Record<Variant, string> = {
  default: "bg-panel border-border text-text hover:bg-[var(--hover)] active:bg-[var(--active)]",
  primary:
    "bg-accent-solid border-transparent text-accent-text hover:brightness-110 active:brightness-95",
  danger: "bg-danger border-transparent text-danger-text hover:brightness-110 active:brightness-95",
  ghost:
    "bg-transparent border-transparent text-text hover:bg-[var(--hover)] active:bg-[var(--active)]",
};

export const Button = forwardRef<
  HTMLButtonElement,
  ButtonHTMLAttributes<HTMLButtonElement> & { variant?: Variant; size?: "sm" | "md" }
>(function Button({ variant = "default", size = "md", className, ...props }, ref) {
  return (
    <button
      ref={ref}
      type="button"
      className={cn(
        "inline-flex items-center justify-center gap-1.5 rounded-md border font-medium whitespace-nowrap transition disabled:pointer-events-none disabled:opacity-45",
        size === "sm" ? "h-6 px-2 text-xs" : "h-7 px-2.5 text-[13px]",
        VARIANTS[variant],
        className,
      )}
      {...props}
    />
  );
});

/** Icon-only button; the label is both the accessible name and the tooltip. */
export const IconButton = forwardRef<
  HTMLButtonElement,
  ButtonHTMLAttributes<HTMLButtonElement> & { icon: IconName; label: string; variant?: Variant }
>(function IconButton({ icon, label, variant = "ghost", className, ...props }, ref) {
  return (
    <Tip label={label}>
      <Button
        ref={ref}
        variant={variant}
        aria-label={label}
        className={cn("w-7 px-0", className)}
        {...props}
      >
        <Icon name={icon} />
      </Button>
    </Tip>
  );
});

export function Tip({ label, children }: { label: string; children: ReactNode }) {
  return (
    <Tooltip.Root delayDuration={500}>
      <Tooltip.Trigger asChild>{children}</Tooltip.Trigger>
      <Tooltip.Portal>
        <Tooltip.Content
          sideOffset={4}
          className="anim-popover z-50 rounded bg-text px-2 py-1 text-xs text-bg shadow-md"
        >
          {label}
        </Tooltip.Content>
      </Tooltip.Portal>
    </Tooltip.Root>
  );
}

export interface MenuItem {
  label: string;
  onSelect: () => void;
  danger?: boolean;
  disabled?: boolean;
}

export function Menu({ trigger, items }: { trigger: ReactNode; items: (MenuItem | "sep")[] }) {
  return (
    <Dropdown.Root>
      <Dropdown.Trigger asChild>{trigger}</Dropdown.Trigger>
      <Dropdown.Portal>
        <Dropdown.Content
          align="end"
          sideOffset={4}
          className="anim-popover z-50 min-w-40 rounded-md border border-border bg-panel p-1 shadow-lg"
        >
          {items.map((item, i) =>
            item === "sep" ? (
              <Dropdown.Separator key={i} className="my-1 h-px bg-border" />
            ) : (
              <Dropdown.Item
                key={item.label}
                disabled={item.disabled}
                onSelect={item.onSelect}
                className={cn(
                  "flex h-7 items-center rounded px-2 outline-none data-[disabled]:opacity-40 data-[highlighted]:bg-sunken",
                  item.danger && "text-danger",
                )}
              >
                {item.label}
              </Dropdown.Item>
            ),
          )}
        </Dropdown.Content>
      </Dropdown.Portal>
    </Dropdown.Root>
  );
}

export function ConfirmDialog({
  open,
  title,
  body,
  confirmLabel,
  danger,
  onConfirm,
  onCancel,
  footer,
}: {
  open: boolean;
  title: string;
  body: ReactNode;
  /** Extra note under the body, e.g. a side effect worth calling out. */
  footer?: ReactNode;
  confirmLabel: string;
  danger?: boolean;
  onConfirm: () => void;
  onCancel: () => void;
}) {
  return (
    <Dialog.Root open={open} onOpenChange={(o) => !o && onCancel()}>
      <Dialog.Portal>
        <Dialog.Overlay className="anim-overlay fixed inset-0 z-40 bg-black/30" />
        <Dialog.Content className="anim-dialog fixed top-1/2 left-1/2 z-50 w-[420px] max-w-[calc(100vw-32px)] -translate-x-1/2 -translate-y-1/2 rounded-lg border border-border bg-panel p-5 shadow-xl">
          <Dialog.Title className="text-[15px] font-semibold">{title}</Dialog.Title>
          <Dialog.Description asChild>
            <div className="mt-2 text-muted">{body}</div>
          </Dialog.Description>
          {footer}
          <div className="mt-5 flex justify-end gap-2">
            <Button onClick={onCancel}>Cancel</Button>
            <Button variant={danger ? "danger" : "primary"} onClick={onConfirm} autoFocus>
              {confirmLabel}
            </Button>
          </div>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}

/** Each Vagrant state's color: a dot beside the state word, as on the website. */
export function stateStyle(state: string): { tone: string; dot: string } {
  switch (state) {
    case "running":
      // A lit dot with a soft ring, like a power LED.
      return { tone: "text-ok", dot: "bg-ok ring-[3px] ring-ok/20" };
    case "saved":
    case "suspended":
    case "paused":
      return { tone: "text-warn", dot: "bg-warn" };
    case "aborted":
    case "error":
      return { tone: "text-danger", dot: "bg-danger" };
    default:
      return { tone: "text-idle", dot: "bg-idle" };
  }
}

/** Counts how many times `value` has changed since mount; 0 means it never has. */
export function useChangeCount<T>(value: T): number {
  const [seen, setSeen] = useState(value);
  const [count, setCount] = useState(0);
  if (value !== seen) {
    setSeen(value);
    setCount(count + 1);
  }
  return count;
}

export function StateBadge({
  state,
  label,
  stale,
  changes = 0,
}: {
  state: string;
  label: string;
  stale?: boolean;
  /** From `useChangeCount(state)`; a new value replays the dot's settle animation. */
  changes?: number;
}) {
  const { dot } = stateStyle(state);
  return (
    <span
      className="flex max-w-full min-w-0 items-center gap-2 font-mono text-xs whitespace-nowrap text-muted"
      title={stale ? "Cached state from Vagrant's index; may be out of date" : undefined}
    >
      <span
        key={changes}
        aria-hidden="true"
        className={cn("size-2 shrink-0 rounded-full", dot, changes > 0 && "anim-glyph")}
      />
      <span className="truncate">{label}</span>
      {/* Hover shows it; screen readers still hear it. */}
      {stale && <span className="sr-only">(cached)</span>}
    </span>
  );
}
