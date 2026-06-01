export function redactForDisplay(value: string): string {
  return value
    .replace(/https?:\/\/\S+/gi, "[redacted-url]")
    .replace(/\b(?:ss|ssr|trojan|vmess|vless|hysteria2?):\/\/\S+/gi, "[redacted-url]")
    .replace(/(token|secret|password|passwd)=\S+/gi, "$1=[redacted]")
    .replace(/[A-Z]:\\Users\\[^ \n\r\t]+\\\.ssh\\[^ \n\r\t]+/gi, "[redacted-path]");
}
