import type { LogLine } from "../ipc/generated/LogLine";

// ANSI codes; the terminal theme maps them to the app palette (see dock/Terminal.tsx). Jobs run
// with --no-tty, so Vagrant's own output never carries colors of its own.
const RESET = "\x1b[0m";
const RED = "\x1b[31m";
const GREEN = "\x1b[32m";
const YELLOW = "\x1b[33m";
const BLUE = "\x1b[34m";
const DIM = "\x1b[90m";

const STEP = /^(==> [^:]+:)(.*)$/;
const DETAIL = /^\s+\S+:/;
const READY = /booted and ready!|is already running/;
const PORTS = /\b\d+(?: \(guest\))? => \d+(?: \(host\))?/g;

const ports = (text: string, after: string) => text.replace(PORTS, (m) => YELLOW + m + after);

/** One job output line with Vagrant's structure in color: steps, details, ports, errors. */
export function colorizeLine(line: LogLine): string {
  const { text } = line;
  if (line.stream === "stderr") return RED + text + RESET;
  const step = STEP.exec(text);
  if (step) {
    const color = READY.test(step[2]) ? GREEN : BLUE;
    return color + step[1] + RESET + ports(step[2], RESET);
  }
  if (DETAIL.test(text)) return DIM + ports(text, DIM) + RESET;
  return ports(text, RESET);
}
