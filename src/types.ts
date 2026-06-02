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
  configPreview?: string | null;
  checkedAt: string;
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
