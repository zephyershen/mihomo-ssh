import { invoke } from "@tauri-apps/api/core";
import type {
  CommandResult,
  EgressTestResult,
  ManagedSshKeyInfo,
  ManualServerInput,
  OperationLog,
  ProxyGroup,
  ProxyNode,
  Server,
  ServerBootstrapInput,
  ServerHealth,
  ServiceCommandResult,
  TunnelInfo,
} from "../types";

const hasTauri = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

async function call<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  if (hasTauri) {
    return invoke<T>(command, args);
  }
  return mockInvoke<T>(command, args);
}

export const api = {
  listServers: () => call<Server[]>("list_servers"),
  importSshHosts: () => call<Server[]>("import_ssh_hosts"),
  getManagedSshKey: () => call<ManagedSshKeyInfo>("get_managed_ssh_key"),
  addManualServer: (input: ManualServerInput) => call<Server[]>("add_manual_server", { input }),
  bootstrapServerWithPassword: (input: ServerBootstrapInput) =>
    call<Server[]>("bootstrap_server_with_password", { input }),
  deleteServer: (serverId: number) => call<Server[]>("delete_server", { serverId }),
  listOperationLogs: (serverId?: number, limit = 120) =>
    call<OperationLog[]>("list_operation_logs", { serverId, limit }),
  testConnection: (serverId: number) => call<CommandResult>("test_connection", { serverId }),
  testServerEgress: (serverId: number, url: string) =>
    call<EgressTestResult>("test_server_egress", { serverId, url }),
  inspectServer: (serverId: number) => call<ServerHealth>("inspect_server", { serverId }),
  installOrRepairMihomo: (serverId: number, subscriptionUrl?: string) =>
    call<CommandResult>("install_or_repair_mihomo", {
      serverId,
      options: { subscriptionUrl: subscriptionUrl || null },
    }),
  updateSubscription: (serverId: number, subscriptionUrl?: string) =>
    call<CommandResult>("update_subscription", {
      serverId,
      options: { subscriptionUrl: subscriptionUrl || null },
    }),
  setMihomoService: (serverId: number, serviceState: string) =>
    call<ServiceCommandResult>("set_mihomo_service", { serverId, serviceState }),
  openControllerTunnel: (serverId: number) =>
    call<TunnelInfo>("open_controller_tunnel", { serverId }),
  closeControllerTunnel: (serverId: number) =>
    call<TunnelInfo>("close_controller_tunnel", { serverId }),
  listProxyGroups: (serverId: number) => call<ProxyGroup[]>("list_proxy_groups", { serverId }),
  selectProxyNode: (serverId: number, group: string, node: string) =>
    call<ProxyGroup[]>("select_proxy_node", { serverId, group, node }),
  measureProxyDelay: (serverId: number, group: string) =>
    call<ProxyNode[]>("measure_proxy_delay", { serverId, group }),
  measureProxyNodeDelay: (serverId: number, node: string) =>
    call<ProxyNode>("measure_proxy_node_delay", { serverId, node }),
  readMihomoLogs: (serverId: number, lines = 200) =>
    call<string>("read_mihomo_logs", { serverId, lines }),
  readMihomoConfig: (serverId: number) => call<string>("read_mihomo_config", { serverId }),
};

async function mockInvoke<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  await new Promise((resolve) => window.setTimeout(resolve, 220));
  const sampleServer: Server = {
    id: 1,
    alias: "demo-box",
    displayName: "demo-box",
    hostName: "10.0.0.10",
    user: "root",
    port: 22,
    identityFileHint: ".../codex_box_ed25519",
    source: "ssh_config",
    lastStatus: "online",
    lastSeenAt: new Date().toISOString(),
  };
  const manualServer: Server = {
    id: 2,
    alias: "manual:root@10.0.0.22:22",
    displayName: "manual-box",
    hostName: "10.0.0.22",
    user: "root",
    port: 22,
    identityFileHint: ".../mihomo_manager_ed25519",
    source: "manual",
    lastStatus: "unknown",
    lastSeenAt: null,
  };
  const sampleHealth: ServerHealth = {
    osPrettyName: "Ubuntu 24.04 LTS",
    osId: "ubuntu",
    arch: "x86_64",
    hasSystemd: true,
    mihomoPath: "/usr/local/bin/mihomo",
    mihomoVersion: "Mihomo Meta mock",
    serviceActive: "active",
    serviceEnabled: "enabled",
    hasConfig: true,
    hasSubscription: true,
    mixedPort: 7890,
    controller: "127.0.0.1:9090",
    allowLan: true,
    geoAutoUpdate: false,
    configPreview: "mixed-port: 7890\nallow-lan: true\nexternal-controller: 127.0.0.1:9090\n",
    checkedAt: new Date().toISOString(),
  };
  const result: CommandResult = { ok: true, code: 0, stdout: "ok", stderr: "" };

  switch (command) {
    case "list_servers":
    case "import_ssh_hosts":
      return [sampleServer] as T;
    case "get_managed_ssh_key":
      return {
        publicKey: "ssh-ed25519 AAAAMOCK mihomo-server-manager",
        publicKeyHint: ".../mihomo_manager_ed25519.pub",
        privateKeyHint: ".../mihomo_manager_ed25519",
      } as T;
    case "add_manual_server":
    case "bootstrap_server_with_password":
      return [sampleServer, manualServer] as T;
    case "delete_server":
      return [] as T;
    case "inspect_server":
      return sampleHealth as T;
    case "test_server_egress":
      return {
        url: String(args?.url ?? "https://www.gstatic.com/generate_204"),
        ok: true,
        status: "204",
        elapsedMs: 128,
        output: { ok: true, code: 0, stdout: "status=204\nelapsed=0.128\n", stderr: "" },
      } as T;
    case "test_connection":
    case "install_or_repair_mihomo":
    case "update_subscription":
      return result as T;
    case "set_mihomo_service":
      return { state: args?.serviceState ?? "start", output: result } as T;
    case "open_controller_tunnel":
      return { serverId: args?.serverId ?? 1, localPort: 19090, status: "open" } as T;
    case "close_controller_tunnel":
      return { serverId: args?.serverId ?? 1, localPort: 19090, status: "closed" } as T;
    case "list_proxy_groups":
    case "select_proxy_node":
      return [
        {
          name: "Cyber Paws",
          now: "HK-01",
          nodes: [
            { name: "HK-01", nodeType: "ss", udp: true, delayMs: 92 },
            { name: "JP-02", nodeType: "trojan", udp: true, delayMs: 134 },
          ],
        },
      ] as T;
    case "measure_proxy_delay":
      return [
        { name: "HK-01", nodeType: "ss", udp: true, delayMs: 92, alive: true },
        { name: "JP-02", nodeType: "trojan", udp: true, delayMs: 134, alive: true },
      ] as T;
    case "measure_proxy_node_delay":
      return {
        name: String(args?.node ?? "HK-01"),
        nodeType: null,
        udp: null,
        delayMs: Math.round(80 + Math.random() * 220),
        alive: true,
      } as T;
    case "read_mihomo_logs":
      return "mihomo mock log line\nservice active\n" as T;
    case "read_mihomo_config":
      return sampleHealth.configPreview as T;
    case "list_operation_logs":
      return [
        {
          id: 1,
          serverId: 1,
          action: "inspect_server",
          status: "ok",
          message: "health refreshed",
          createdAt: new Date().toISOString(),
        },
      ] as T;
    default:
      throw new Error(`mock command not implemented: ${command}`);
  }
}
