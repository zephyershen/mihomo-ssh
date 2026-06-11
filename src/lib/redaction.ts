export function redactForDisplay(value: string): string {
  return value
    .replace(/\b(?:https?|socks5h?|socks|ss|ssr|trojan|vmess|vless|hysteria2?):\/\/\S+/gi, "[redacted-url]")
    .replace(/(subscription|token|secret|password|passwd)=\S+/gi, "$1=[redacted]")
    .replace(/[A-Z]:\\[^ \n\r\t]*\.ssh\\[^ \n\r\t]+/gi, "[redacted-path]")
    .replace(/(?:~|\/home\/[^ \n\r\t]+|\/root)\/\.ssh\/[^ \n\r\t]+/gi, "[redacted-path]");
}
