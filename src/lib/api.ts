import { invoke } from "@tauri-apps/api/core";
import type {
  BackupSnapshot,
  CommandResult,
  EgressTestResult,
  ManagedSshKeyInfo,
  ManualServerInput,
  OperationLog,
  ProxyGroup,
  ProxyNode,
  RemoteProxyConfig,
  RemoteProxyInput,
  Server,
  ServerBootstrapInput,
  ServerHealth,
  ServiceCommandResult,
  SubscriptionInput,
  SubscriptionProfile,
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
  listBackups: (serverId: number) => call<BackupSnapshot[]>("list_backups", { serverId }),
  createBackup: (serverId: number, label?: string) =>
    call<BackupSnapshot>("create_backup", { serverId, label: label || null }),
  restoreBackup: (serverId: number, backupId: number) =>
    call<CommandResult>("restore_backup", { serverId, backupId }),
  deleteBackup: (serverId: number, backupId: number) =>
    call<CommandResult>("delete_backup", { serverId, backupId }),
  listSubscriptions: () => call<SubscriptionProfile[]>("list_subscriptions"),
  saveSubscription: (input: SubscriptionInput) =>
    call<SubscriptionProfile>("save_subscription", { input }),
  deleteSubscription: (subscriptionId: number) =>
    call<SubscriptionProfile[]>("delete_subscription", { subscriptionId }),
  markSubscriptionUsed: (subscriptionId: number) =>
    call<SubscriptionProfile>("mark_subscription_used", { subscriptionId }),
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
  setMihomoTunEnabled: (serverId: number, enabled: boolean) =>
    call<CommandResult>("set_mihomo_tun_enabled", { serverId, enabled }),
  inspectRemoteProxy: (serverId: number) =>
    call<RemoteProxyConfig>("inspect_remote_proxy", { serverId }),
  saveRemoteProxy: (serverId: number, input: RemoteProxyInput) =>
    call<CommandResult>("save_remote_proxy", { serverId, input }),
  setRemoteProxyEnabled: (serverId: number, enabled: boolean) =>
    call<CommandResult>("set_remote_proxy_enabled", { serverId, enabled }),
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
  autoRecoverProxyNode: (serverId: number, group: string, currentNode: string, failedForSeconds: number) =>
    call<ProxyGroup[]>("auto_recover_proxy_node", {
      serverId,
      group,
      currentNode,
      failedForSeconds,
    }),
  readMihomoLogs: (serverId: number, lines = 200) =>
    call<string>("read_mihomo_logs", { serverId, lines }),
  readMihomoConfig: (serverId: number) => call<string>("read_mihomo_config", { serverId }),
};

let mockSubscriptions: SubscriptionProfile[] = [
  {
    id: 1,
    name: "Cyber Paws",
    url: "https://example.com/sub?token=cyber-paws",
    createdAt: new Date(Date.now() - 1000 * 60 * 60 * 26).toISOString(),
    updatedAt: new Date(Date.now() - 1000 * 60 * 60).toISOString(),
    lastUsedAt: new Date(Date.now() - 1000 * 60 * 60).toISOString(),
  },
  {
    id: 2,
    name: "HENET",
    url: "https://getinfo.bigwater.example/subscription",
    createdAt: new Date(Date.now() - 1000 * 60 * 60 * 72).toISOString(),
    updatedAt: new Date(Date.now() - 1000 * 60 * 36).toISOString(),
    lastUsedAt: null,
  },
];

let mockRemoteProxy: RemoteProxyConfig = {
  enabled: true,
  managed: true,
  profilePath: "/etc/profile.d/mihomo-manager-proxy.sh",
  httpProxy: "http://127.0.0.1:7890",
  httpsProxy: "http://127.0.0.1:7890",
  allProxy: "socks5h://127.0.0.1:7890",
  noProxy: "localhost,127.0.0.1,::1,10.40.2.0/24",
  detectedEnv: [
    { name: "http_proxy", value: "http://127.0.0.1:7890" },
    { name: "HTTPS_PROXY", value: "http://127.0.0.1:7890" },
    { name: "ALL_PROXY", value: "socks5h://127.0.0.1:7890" },
    { name: "no_proxy", value: "localhost,127.0.0.1,::1,10.40.2.0/24" },
  ],
};

let mockTunEnabled = false;

let mockBackups: BackupSnapshot[] = [
  {
    id: 1,
    serverId: 1,
    reason: "update_subscription",
    label: "更新订阅前",
    remoteDir: "/etc/mihomo/manager-backups/20260611123000000-update-subscription",
    status: "ok",
    createdAt: new Date(Date.now() - 1000 * 60 * 30).toISOString(),
    files: [
      {
        kind: "config",
        remotePath: "/etc/mihomo/config.yaml",
        backupFile: "config.yaml",
        present: true,
        sizeBytes: 4096,
        sha256: "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
      },
      {
        kind: "subscription",
        remotePath: "/etc/mihomo/subscription.url",
        backupFile: "subscription.url",
        present: true,
        sizeBytes: 80,
        sha256: "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
      },
      {
        kind: "remote_proxy",
        remotePath: "/etc/profile.d/mihomo-manager-proxy.sh",
        backupFile: "mihomo-manager-proxy.sh",
        present: true,
        sizeBytes: 320,
        sha256: "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
      },
    ],
  },
];

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
    tun: {
      enabled: mockTunEnabled,
      stack: mockTunEnabled ? "system" : null,
      autoRoute: mockTunEnabled ? true : null,
      autoDetectInterface: mockTunEnabled ? true : null,
      autoRedirect: null,
      dnsHijack: mockTunEnabled ? ["any:53"] : [],
      routeExcludeAddress: mockTunEnabled
        ? ["10.0.0.0/8", "172.16.0.0/12", "192.168.0.0/16", "10.40.2.10/32"]
        : [],
      sshProtection: mockTunEnabled ? ["10.40.2.10/32", "10.0.0.0/8"] : [],
    },
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
    case "set_mihomo_tun_enabled":
      mockTunEnabled = Boolean(args?.enabled);
      return {
        ok: true,
        code: 0,
        stdout: `tun ${mockTunEnabled ? "enabled" : "disabled"}`,
        stderr: "",
      } as T;
    case "inspect_remote_proxy":
      return mockRemoteProxy as T;
    case "save_remote_proxy": {
      const input = args?.input as RemoteProxyInput | undefined;
      mockRemoteProxy = remoteProxyFromInput(input ?? remoteProxyInputFromConfig(mockRemoteProxy));
      return {
        ok: true,
        code: 0,
        stdout: `remote proxy ${mockRemoteProxy.enabled ? "enabled" : "disabled"}\nprofile: ${mockRemoteProxy.profilePath}\n`,
        stderr: "",
      } as T;
    }
    case "set_remote_proxy_enabled": {
      mockRemoteProxy = remoteProxyFromInput({
        ...remoteProxyInputFromConfig(mockRemoteProxy),
        enabled: Boolean(args?.enabled),
      });
      return {
        ok: true,
        code: 0,
        stdout: `remote proxy ${mockRemoteProxy.enabled ? "enabled" : "disabled"}\n`,
        stderr: "",
      } as T;
    }
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
    case "auto_recover_proxy_node":
      return [
        {
          name: String(args?.group ?? "Cyber Paws"),
          now: "JP-02",
          nodes: [
            { name: "HK-01", nodeType: "ss", udp: true, delayMs: null, alive: false },
            { name: "JP-02", nodeType: "trojan", udp: true, delayMs: 134, alive: true },
          ],
        },
      ] as T;
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
    case "list_backups":
      return mockBackups.filter((backup) => backup.serverId === args?.serverId) as T;
    case "create_backup": {
      const now = new Date().toISOString();
      const id = Math.max(0, ...mockBackups.map((backup) => backup.id)) + 1;
      const snapshot: BackupSnapshot = {
        id,
        serverId: Number(args?.serverId ?? 1),
        reason: "manual",
        label: typeof args?.label === "string" ? args.label : "手动备份",
        remoteDir: `/etc/mihomo/manager-backups/mock-${id}-manual`,
        status: "ok",
        createdAt: now,
        files: mockBackups[0]?.files ?? [],
      };
      mockBackups = [snapshot, ...mockBackups].slice(0, 20);
      return snapshot as T;
    }
    case "restore_backup":
      return { ok: true, code: 0, stdout: "restored backup", stderr: "" } as T;
    case "delete_backup":
      mockBackups = mockBackups.filter((backup) => backup.id !== args?.backupId);
      return { ok: true, code: 0, stdout: "deleted backup", stderr: "" } as T;
    case "list_subscriptions":
      return mockSubscriptions as T;
    case "save_subscription": {
      const input = args?.input as SubscriptionInput | undefined;
      const now = new Date().toISOString();
      const id = input?.id ?? Math.max(0, ...mockSubscriptions.map((item) => item.id)) + 1;
      const url = input?.url.trim() || "https://example.com/sub";
      const name = input?.name?.trim() || subscriptionNameFromUrl(url);
      const existing = mockSubscriptions.find((item) => item.id === id);
      const saved: SubscriptionProfile = {
        id,
        name,
        url,
        createdAt: existing?.createdAt ?? now,
        updatedAt: now,
        lastUsedAt: existing?.lastUsedAt ?? null,
      };
      mockSubscriptions = [saved, ...mockSubscriptions.filter((item) => item.id !== id)];
      return saved as T;
    }
    case "delete_subscription":
      mockSubscriptions = mockSubscriptions.filter((item) => item.id !== args?.subscriptionId);
      return mockSubscriptions as T;
    case "mark_subscription_used": {
      const now = new Date().toISOString();
      mockSubscriptions = mockSubscriptions.map((item) =>
        item.id === args?.subscriptionId ? { ...item, updatedAt: now, lastUsedAt: now } : item,
      );
      return mockSubscriptions.find((item) => item.id === args?.subscriptionId) as T;
    }
    default:
      throw new Error(`mock command not implemented: ${command}`);
  }
}

function subscriptionNameFromUrl(url: string): string {
  try {
    return new URL(url).hostname.replace(/^www\./, "") || "Subscription";
  } catch {
    return "Subscription";
  }
}

function remoteProxyInputFromConfig(config: RemoteProxyConfig): RemoteProxyInput {
  return {
    enabled: config.enabled,
    httpProxy: config.httpProxy ?? "",
    httpsProxy: config.httpsProxy ?? "",
    allProxy: config.allProxy ?? "",
    noProxy: config.noProxy ?? "",
  };
}

function remoteProxyFromInput(input: RemoteProxyInput): RemoteProxyConfig {
  const detectedEnv = input.enabled
    ? [
        { name: "http_proxy", value: input.httpProxy },
        { name: "https_proxy", value: input.httpsProxy },
        { name: "all_proxy", value: input.allProxy },
        { name: "no_proxy", value: input.noProxy },
      ].filter((item) => item.value)
    : [];
  return {
    enabled: input.enabled,
    managed: true,
    profilePath: "/etc/profile.d/mihomo-manager-proxy.sh",
    httpProxy: input.httpProxy,
    httpsProxy: input.httpsProxy,
    allProxy: input.allProxy,
    noProxy: input.noProxy,
    detectedEnv,
  };
}
