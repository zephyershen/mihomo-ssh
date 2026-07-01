export type Server = {
  id: number;
  alias: string;
  displayName: string;
  hostName: string;
  user?: string | null;
  port?: number | null;
  identityFileHint?: string | null;
  source: string;
  lastStatus?: string | null;
  lastSeenAt?: string | null;
};

export type ManualServerInput = {
  displayName?: string | null;
  hostName: string;
  user: string;
  port?: number | null;
};

export type ServerBootstrapInput = ManualServerInput & {
  password: string;
};

export type ManagedSshKeyInfo = {
  publicKey: string;
  publicKeyHint: string;
  privateKeyHint: string;
};

export type ServerHealth = {
  osPrettyName?: string | null;
  osId?: string | null;
  arch?: string | null;
  hasSystemd: boolean;
  mihomoPath?: string | null;
  mihomoVersion?: string | null;
  serviceActive?: string | null;
  serviceEnabled?: string | null;
  hasConfig: boolean;
  hasSubscription: boolean;
  mixedPort?: number | null;
  controller?: string | null;
  allowLan?: boolean | null;
  geoAutoUpdate?: boolean | null;
  tun?: TunConfig | null;
  configPreview?: string | null;
  checkedAt: string;
};

export type TunConfig = {
  enabled: boolean;
  stack?: string | null;
  autoRoute?: boolean | null;
  autoDetectInterface?: boolean | null;
  autoRedirect?: boolean | null;
  dnsHijack: string[];
  routeExcludeAddress: string[];
  sshProtection: string[];
};

export type CommandResult = {
  ok: boolean;
  code?: number | null;
  stdout: string;
  stderr: string;
};

export type EgressTestResult = {
  url: string;
  ok: boolean;
  status?: string | null;
  elapsedMs?: number | null;
  output: CommandResult;
};

export type ServiceCommandResult = {
  state: string;
  output: CommandResult;
};

export type TunnelInfo = {
  serverId: number;
  localPort: number;
  status: string;
};

export type ProxyNode = {
  name: string;
  nodeType?: string | null;
  udp?: boolean | null;
  delayMs?: number | null;
  alive?: boolean | null;
};

export type ProxyGroup = {
  name: string;
  now?: string | null;
  nodes: ProxyNode[];
};

export type OperationLog = {
  id: number;
  serverId?: number | null;
  action: string;
  status: string;
  message: string;
  createdAt: string;
};

export type BackupFile = {
  kind: string;
  remotePath: string;
  backupFile: string;
  present: boolean;
  sizeBytes?: number | null;
  sha256?: string | null;
};

export type BackupSnapshot = {
  id: number;
  serverId: number;
  reason: string;
  label?: string | null;
  remoteDir: string;
  files: BackupFile[];
  status: string;
  createdAt: string;
};

export type SubscriptionProfile = {
  id: number;
  name: string;
  url: string;
  createdAt: string;
  updatedAt: string;
  lastUsedAt?: string | null;
};

export type SubscriptionInput = {
  id?: number | null;
  name?: string | null;
  url: string;
};

export type RemoteProxyEnvVar = {
  name: string;
  value: string;
};

export type RemoteProxyConfig = {
  enabled: boolean;
  managed: boolean;
  profilePath: string;
  httpProxy?: string | null;
  httpsProxy?: string | null;
  allProxy?: string | null;
  noProxy?: string | null;
  detectedEnv: RemoteProxyEnvVar[];
};

export type RemoteProxyInput = {
  enabled: boolean;
  httpProxy: string;
  httpsProxy: string;
  allProxy: string;
  noProxy: string;
};

export type SharedRulesConfig = {
  remotePath: string;
  rules: string;
  appliedCount: number;
};
