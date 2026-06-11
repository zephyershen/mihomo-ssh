import { describe, expect, it } from "vitest";
import { redactForDisplay } from "./redaction";

describe("redactForDisplay", () => {
  it("redacts subscription urls and tokens", () => {
    const text = "fetch https://example.com/sub?token=abc and vmess://secret";
    const redacted = redactForDisplay(text);

    expect(redacted).toContain("[redacted-url]");
    expect(redacted).not.toContain("token=abc");
    expect(redacted).not.toContain("vmess://secret");
  });

  it("redacts proxy credentials and ssh paths", () => {
    const text =
      "proxy=socks5h://user:pass@127.0.0.1:7890 key=/home/root/.ssh/id_ed25519 subscription=abc";
    const redacted = redactForDisplay(text);

    expect(redacted).toContain("[redacted-url]");
    expect(redacted).toContain("[redacted-path]");
    expect(redacted).toContain("subscription=[redacted]");
    expect(redacted).not.toContain("user:pass");
    expect(redacted).not.toContain("/.ssh/");
  });
});
