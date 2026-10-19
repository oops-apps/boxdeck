import { describe, expect, it } from "vitest";
import { colorizeLine } from "./vagrantLog";

const out = (text: string) => colorizeLine({ seq: 0, stream: "stdout", text });
const SGR = new RegExp(String.fromCharCode(27) + "\\[[0-9;]*m", "g");
const plain = (s: string) => s.replace(SGR, "");

// Lines from tests/fixtures/captured/2.4.9-darwin/up-web.stdout, plus Vagrant's usual
// port-forward and ready lines.
describe("colorizeLine", () => {
  it("colors the ==> step prefix and keeps the message", () => {
    expect(out("==> web: Creating the container...")).toBe(
      "\x1b[34m==> web:\x1b[0m Creating the container...",
    );
  });

  it("marks the ready line green", () => {
    expect(out("==> default: Machine booted and ready!")).toBe(
      "\x1b[32m==> default:\x1b[0m Machine booted and ready!",
    );
  });

  it("dims indented detail lines", () => {
    expect(out("    web:  Image: alpine:3")).toBe("\x1b[90m    web:  Image: alpine:3\x1b[0m");
  });

  it("highlights forwarded ports, also inside dimmed details", () => {
    expect(out("==> web: Fixed port collision for 22 => 2222. Now on port 2250.")).toBe(
      "\x1b[34m==> web:\x1b[0m Fixed port collision for \x1b[33m22 => 2222\x1b[0m. Now on port 2250.",
    );
    expect(out("    default: 22 (guest) => 2222 (host) (adapter 1)")).toBe(
      "\x1b[90m    default: \x1b[33m22 (guest) => 2222 (host)\x1b[90m (adapter 1)\x1b[0m",
    );
  });

  it("shows stderr in red and leaves other lines alone", () => {
    expect(colorizeLine({ seq: 1, stream: "stderr", text: "Vagrant failed" })).toBe(
      "\x1b[31mVagrant failed\x1b[0m",
    );
    expect(out("Bringing machine 'web' up with 'docker' provider...")).toBe(
      "Bringing machine 'web' up with 'docker' provider...",
    );
  });

  it("never changes the text itself", () => {
    const line = "    web: Volume: /tmp/boxdeck-capture/dmulti:/vagrant";
    expect(plain(out(line))).toBe(line);
  });
});
