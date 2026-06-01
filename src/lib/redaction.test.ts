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
});
