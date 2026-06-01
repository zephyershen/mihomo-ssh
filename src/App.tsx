import {
  Activity,
  Cable,
  CheckCircle2,
  CircleDot,
  Download,
  FileText,
  Gauge,
  ListRestart,
  Loader2,
  Network,
  Power,
  PowerOff,
  RefreshCcw,
  RotateCw,
  Save,
  Server as ServerIcon,
  Settings,
  ShieldAlert,
  TerminalSquare,
  UploadCloud,
  Wifi,
  XCircle,
} from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { api } from "./lib/api";
import { redactForDisplay } from "./lib/redaction";
import type { CommandResult, OperationLog, ProxyGroup, ProxyNode, Server, ServerHealth } from "./types";

type Tab = "overview" | "install" | "subscription" | "nodes" | "config" | "logs";

const tabs: Array<{ id: Tab; label: string; icon: typeof Activity }> = [
  { id: "overview", label: "概览", icon: Gauge },
  { id: "install", label: "安装/健康", icon: Download },
  { id: "subscription", label: "订阅", icon: UploadCloud },
  { id: "nodes", label: "节点", icon: Network },
  { id: "config", label: "配置", icon: Settings },
  { id: "logs", label: "日志", icon: TerminalSquare },
];

export function App() {
  const [servers, setServers] = useState<Server[]>([]);
  const [selectedId, setSelectedId] = useState<number | null>(null);
  const [health, setHealth] = useState<ServerHealth | null>(null);
  const [activeTab, setActiveTab] = useState<Tab>("overview");
  const [busy, setBusy] = useState<string | null>(null);
  const [toast, setToast] = useState<string>("");
  const [commandOutput, setCommandOutput] = useState<string>("");
  const [subscriptionUrl, setSubscriptionUrl] = useState("");
  const [groups, setGroups] = useState<ProxyGroup[]>([]);
  const [selectedGroup, setSelectedGroup] = useState<string>("");
  const [measuredNodes, setMeasuredNodes] = useState<ProxyNode[]>([]);
  const [logs, setLogs] = useState("");
  const [operationLogs, setOperationLogs] = useState<OperationLog[]>([]);

  const selected = useMemo(
    () => servers.find((server) => server.id === selectedId) ?? null,
    [servers, selectedId],
  );

  const loadServers = useCallback(async () => {
    const next = await api.listServers();
    setServers(next);
    setSelectedId((current) => current ?? next[0]?.id ?? null);
  }, []);

  useEffect(() => {
    void loadServers().catch((error) => setToast(String(error)));
  }, [loadServers]);

  useEffect(() => {
    if (!selectedId) {
      return;
    }
    void refreshHealth(selectedId);
    void refreshOperationLogs(selectedId);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [selectedId]);

  async function run<T>(
    label: string,
    work: () => Promise<T>,
    onSuccess?: (value: T) => void,
  ): Promise<T | undefined> {
    setBusy(label);
    setToast("");
    try {
      const value = await work();
      onSuccess?.(value);
      setToast(`${label} 完成`);
      return value;
    } catch (error) {
      setToast(`${label} 失败：${String(error)}`);
      return undefined;
    } finally {
      setBusy(null);
    }
  }

  async function refreshHealth(id = selectedId) {
    if (!id) return;
    await run("刷新健康状态", () => api.inspectServer(id), setHealth);
  }

  async function refreshOperationLogs(id = selectedId) {
    if (!id) return;
    try {
      setOperationLogs(await api.listOperationLogs(id, 80));
    } catch {
      setOperationLogs([]);
    }
  }

  async function command(label: string, work: () => Promise<CommandResult>) {
    const result = await run(label, work);
    if (result) {
      setCommandOutput(redactForDisplay(result.ok ? result.stdout || "ok" : result.stderr || "failed"));
      void refreshHealth();
      void refreshOperationLogs();
    }
  }

  async function importHosts() {
    await run("导入 SSH 主机", api.importSshHosts, (next) => {
      setServers(next);
      setSelectedId(next[0]?.id ?? null);
    });
  }

  async function loadProxyGroups() {
    if (!selected) return;
    await run("加载节点", () => api.listProxyGroups(selected.id), (next) => {
      setGroups(next);
      setSelectedGroup((current) => current || next[0]?.name || "");
    });
  }

  async function measureDelay() {
    if (!selected || !selectedGroup) return;
    await run("测速", () => api.measureProxyDelay(selected.id, selectedGroup), setMeasuredNodes);
  }

  async function selectNode(group: string, node: string) {
    if (!selected) return;
    await run("切换节点", () => api.selectProxyNode(selected.id, group, node), setGroups);
  }

  async function readLogs() {
    if (!selected) return;
    await run("读取日志", () => api.readMihomoLogs(selected.id, 240), (value) =>
      setLogs(redactForDisplay(value)),
    );
  }

  const currentGroup = groups.find((group) => group.name === selectedGroup);
  const displayedNodes = measuredNodes.length ? measuredNodes : currentGroup?.nodes ?? [];

  return (
    <div className="app-shell">
      <aside className="sidebar">
        <div className="brand-row">
          <div className="brand-mark">
            <ServerIcon size={20} />
          </div>
          <div>
            <div className="brand-title">Mihomo Manager</div>
            <div className="brand-subtitle">Local SSH Control</div>
          </div>
        </div>

        <div className="sidebar-actions">
          <button className="icon-button" title="导入 SSH config" onClick={importHosts} disabled={!!busy}>
            <UploadCloud size={17} />
          </button>
          <button className="icon-button" title="刷新服务器列表" onClick={loadServers} disabled={!!busy}>
            <RefreshCcw size={17} />
          </button>
        </div>

        <div className="server-list">
          {servers.map((server) => (
            <button
              key={server.id}
              className={`server-row ${server.id === selectedId ? "selected" : ""}`}
              onClick={() => setSelectedId(server.id)}
            >
              <StatusDot status={server.lastStatus} />
              <span className="server-main">
                <span className="server-name">{server.displayName}</span>
                <span className="server-address">
                  {server.user ? `${server.user}@` : ""}
                  {server.hostName}
                  {server.port ? `:${server.port}` : ""}
                </span>
              </span>
            </button>
          ))}
          {!servers.length && <div className="empty-state">No hosts imported</div>}
        </div>
      </aside>

      <main className="main-pane">
        <header className="topbar">
          <div>
            <h1>{selected?.displayName ?? "No server selected"}</h1>
            <div className="topbar-meta">
              {selected ? `${selected.alias} · ${selected.hostName}` : "Import SSH hosts to begin"}
            </div>
          </div>
          <div className="topbar-actions">
            <button className="tool-button" title="连接测试" disabled={!selected || !!busy} onClick={() => selected && command("连接测试", () => api.testConnection(selected.id))}>
              <Cable size={16} />
              Test
            </button>
            <button className="tool-button" title="刷新健康状态" disabled={!selected || !!busy} onClick={() => refreshHealth()}>
              <RefreshCcw size={16} />
              Refresh
            </button>
          </div>
        </header>

        <nav className="tabbar" aria-label="Sections">
          {tabs.map((tab) => {
            const Icon = tab.icon;
            return (
              <button
                key={tab.id}
                className={`tab ${activeTab === tab.id ? "active" : ""}`}
                onClick={() => setActiveTab(tab.id)}
              >
                <Icon size={16} />
                {tab.label}
              </button>
            );
          })}
        </nav>

        <section className="content">
          {activeTab === "overview" && (
            <OverviewPanel
              selected={selected}
              health={health}
              busy={busy}
              onStart={() => selected && command("启动代理", () => service(selected.id, "start"))}
              onStop={() => selected && command("关闭代理", () => service(selected.id, "stop"))}
              onRestart={() => selected && command("重启代理", () => service(selected.id, "restart"))}
            />
          )}

          {activeTab === "install" && (
            <InstallPanel
              selected={selected}
              health={health}
              busy={busy}
              subscriptionUrl={subscriptionUrl}
              setSubscriptionUrl={setSubscriptionUrl}
              onInstall={() =>
                selected &&
                command("安装/修复 mihomo", () => api.installOrRepairMihomo(selected.id, subscriptionUrl))
              }
              onInspect={() => refreshHealth()}
              output={commandOutput}
            />
          )}

          {activeTab === "subscription" && (
            <SubscriptionPanel
              selected={selected}
              health={health}
              busy={busy}
              subscriptionUrl={subscriptionUrl}
              setSubscriptionUrl={setSubscriptionUrl}
              onUpdate={() =>
                selected && command("更新订阅", () => api.updateSubscription(selected.id, subscriptionUrl))
              }
              output={commandOutput}
            />
          )}

          {activeTab === "nodes" && (
            <NodesPanel
              selected={selected}
              busy={busy}
              groups={groups}
              selectedGroup={selectedGroup}
              setSelectedGroup={setSelectedGroup}
              nodes={displayedNodes}
              onOpenTunnel={() => selected && run("打开控制通道", () => api.openControllerTunnel(selected.id))}
              onCloseTunnel={() => selected && run("关闭控制通道", () => api.closeControllerTunnel(selected.id))}
              onLoad={loadProxyGroups}
              onMeasure={measureDelay}
              onSelect={selectNode}
            />
          )}

          {activeTab === "config" && <ConfigPanel health={health} selected={selected} />}

          {activeTab === "logs" && (
            <LogsPanel
              selected={selected}
              busy={busy}
              logs={logs}
              operationLogs={operationLogs}
              onReadLogs={readLogs}
              onReadOps={() => refreshOperationLogs()}
            />
          )}
        </section>

        {(busy || toast) && (
          <div className="status-strip">
            {busy && <Loader2 className="spin" size={16} />}
            <span>{busy || toast}</span>
          </div>
        )}
      </main>
    </div>
  );
}

async function service(serverId: number, serviceState: string): Promise<CommandResult> {
  const response = await api.setMihomoService(serverId, serviceState);
  return response.output;
}

function OverviewPanel(props: {
  selected: Server | null;
  health: ServerHealth | null;
  busy: string | null;
  onStart: () => void;
  onStop: () => void;
  onRestart: () => void;
}) {
  const { selected, health, busy, onStart, onStop, onRestart } = props;
  return (
    <div className="panel-grid">
      <Metric label="SSH" value={selected?.lastStatus ?? "unknown"} tone={selected?.lastStatus === "online" ? "good" : "neutral"} />
      <Metric label="Service" value={health?.serviceActive ?? "unknown"} tone={health?.serviceActive === "active" ? "good" : "warn"} />
      <Metric label="Mihomo" value={health?.mihomoVersion ?? "not detected"} tone={health?.mihomoPath ? "good" : "warn"} />
      <Metric label="Controller" value={health?.controller ?? "missing"} tone={health?.controller ? "good" : "warn"} />

      <div className="wide-panel">
        <div className="panel-title">Power</div>
        <div className="button-row">
          <button className="command-button primary" disabled={!selected || !!busy} onClick={onStart}>
            <Power size={17} />
            Start
          </button>
          <button className="command-button danger" disabled={!selected || !!busy} onClick={onStop}>
            <PowerOff size={17} />
            Stop
          </button>
          <button className="command-button" disabled={!selected || !!busy} onClick={onRestart}>
            <RotateCw size={17} />
            Restart
          </button>
        </div>
      </div>

      <div className="wide-panel">
        <div className="panel-title">Runtime</div>
        <div className="kv-grid">
          <KeyValue label="OS" value={health?.osPrettyName} />
          <KeyValue label="Arch" value={health?.arch} />
          <KeyValue label="Systemd" value={health?.hasSystemd ? "yes" : "no"} />
          <KeyValue label="Subscription" value={health?.hasSubscription ? "saved" : "missing"} />
        </div>
      </div>
    </div>
  );
}

function InstallPanel(props: {
  selected: Server | null;
  health: ServerHealth | null;
  busy: string | null;
  subscriptionUrl: string;
  setSubscriptionUrl: (value: string) => void;
  onInstall: () => void;
  onInspect: () => void;
  output: string;
}) {
  return (
    <div className="split-layout">
      <div className="work-panel">
        <div className="panel-title">Install</div>
        <SecretInput value={props.subscriptionUrl} onChange={props.setSubscriptionUrl} />
        <div className="button-row">
          <button className="command-button primary" disabled={!props.selected || !!props.busy} onClick={props.onInstall}>
            <Download size={17} />
            Install / Repair
          </button>
          <button className="command-button" disabled={!props.selected || !!props.busy} onClick={props.onInspect}>
            <Activity size={17} />
            Inspect
          </button>
        </div>
        <HealthChecklist health={props.health} />
      </div>
      <OutputPanel output={props.output} />
    </div>
  );
}

function SubscriptionPanel(props: {
  selected: Server | null;
  health: ServerHealth | null;
  busy: string | null;
  subscriptionUrl: string;
  setSubscriptionUrl: (value: string) => void;
  onUpdate: () => void;
  output: string;
}) {
  return (
    <div className="split-layout">
      <div className="work-panel">
        <div className="panel-title">Subscription</div>
        <SecretInput value={props.subscriptionUrl} onChange={props.setSubscriptionUrl} />
        <div className="button-row">
          <button className="command-button primary" disabled={!props.selected || !!props.busy} onClick={props.onUpdate}>
            <ListRestart size={17} />
            Update
          </button>
        </div>
        <div className="kv-grid compact">
          <KeyValue label="Saved URL" value={props.health?.hasSubscription ? "yes" : "no"} />
          <KeyValue label="mixed-port" value={props.health?.mixedPort} />
          <KeyValue label="allow-lan" value={String(props.health?.allowLan ?? "unknown")} />
          <KeyValue label="geo update" value={String(props.health?.geoAutoUpdate ?? "unknown")} />
        </div>
        {props.health?.allowLan && (
          <div className="warning-line">
            <ShieldAlert size={16} />
            allow-lan=true
          </div>
        )}
      </div>
      <OutputPanel output={props.output} />
    </div>
  );
}

function NodesPanel(props: {
  selected: Server | null;
  busy: string | null;
  groups: ProxyGroup[];
  selectedGroup: string;
  setSelectedGroup: (value: string) => void;
  nodes: ProxyNode[];
  onOpenTunnel: () => void;
  onCloseTunnel: () => void;
  onLoad: () => void;
  onMeasure: () => void;
  onSelect: (group: string, node: string) => void;
}) {
  return (
    <div className="nodes-layout">
      <div className="toolbar-line">
        <button className="tool-button" disabled={!props.selected || !!props.busy} onClick={props.onOpenTunnel}>
          <Wifi size={16} />
          Tunnel
        </button>
        <button className="tool-button" disabled={!props.selected || !!props.busy} onClick={props.onLoad}>
          <RefreshCcw size={16} />
          Groups
        </button>
        <button className="tool-button" disabled={!props.selectedGroup || !!props.busy} onClick={props.onMeasure}>
          <Activity size={16} />
          Delay
        </button>
        <button className="tool-button ghost" disabled={!props.selected || !!props.busy} onClick={props.onCloseTunnel}>
          <XCircle size={16} />
          Close
        </button>
        <select
          className="select"
          value={props.selectedGroup}
          onChange={(event) => props.setSelectedGroup(event.target.value)}
        >
          {props.groups.map((group) => (
            <option key={group.name} value={group.name}>
              {group.name}
            </option>
          ))}
        </select>
      </div>

      <div className="table-wrap">
        <table>
          <thead>
            <tr>
              <th>Node</th>
              <th>Type</th>
              <th>UDP</th>
              <th>Delay</th>
              <th />
            </tr>
          </thead>
          <tbody>
            {props.nodes.map((node) => (
              <tr key={node.name}>
                <td className="node-name">{node.name}</td>
                <td>{node.nodeType ?? "-"}</td>
                <td>{node.udp == null ? "-" : node.udp ? "yes" : "no"}</td>
                <td>
                  {node.alive === false ? (
                    <span className="bad-text">fail</span>
                  ) : node.delayMs ? (
                    `${node.delayMs} ms`
                  ) : (
                    "-"
                  )}
                </td>
                <td className="row-action">
                  <button
                    className="icon-button small"
                    title="切换到此节点"
                    disabled={!props.selectedGroup || !!props.busy}
                    onClick={() => props.onSelect(props.selectedGroup, node.name)}
                  >
                    <CheckCircle2 size={15} />
                  </button>
                </td>
              </tr>
            ))}
            {!props.nodes.length && (
              <tr>
                <td colSpan={5} className="empty-table">
                  No nodes loaded
                </td>
              </tr>
            )}
          </tbody>
        </table>
      </div>
    </div>
  );
}

function ConfigPanel({ health, selected }: { health: ServerHealth | null; selected: Server | null }) {
  return (
    <div className="split-layout">
      <div className="work-panel">
        <div className="panel-title">Key Fields</div>
        <div className="kv-grid">
          <KeyValue label="Host" value={selected?.hostName} />
          <KeyValue label="User" value={selected?.user} />
          <KeyValue label="mixed-port" value={health?.mixedPort} />
          <KeyValue label="controller" value={health?.controller} />
          <KeyValue label="allow-lan" value={String(health?.allowLan ?? "unknown")} />
          <KeyValue label="geo-auto-update" value={String(health?.geoAutoUpdate ?? "unknown")} />
        </div>
      </div>
      <div className="output-panel">
        <div className="panel-title">YAML Preview</div>
        <pre>{redactForDisplay(health?.configPreview ?? "")}</pre>
      </div>
    </div>
  );
}

function LogsPanel(props: {
  selected: Server | null;
  busy: string | null;
  logs: string;
  operationLogs: OperationLog[];
  onReadLogs: () => void;
  onReadOps: () => void;
}) {
  return (
    <div className="split-layout">
      <div className="work-panel">
        <div className="panel-title">Operations</div>
        <div className="button-row">
          <button className="command-button" disabled={!props.selected || !!props.busy} onClick={props.onReadLogs}>
            <FileText size={17} />
            Journal
          </button>
          <button className="command-button" disabled={!props.selected || !!props.busy} onClick={props.onReadOps}>
            <RefreshCcw size={17} />
            Local
          </button>
        </div>
        <div className="ops-list">
          {props.operationLogs.map((log) => (
            <div key={log.id} className="op-row">
              <span className={`op-status ${log.status}`}>{log.status}</span>
              <span>{log.action}</span>
              <time>{new Date(log.createdAt).toLocaleString()}</time>
            </div>
          ))}
        </div>
      </div>
      <div className="output-panel">
        <div className="panel-title">Journal</div>
        <pre>{props.logs}</pre>
      </div>
    </div>
  );
}

function HealthChecklist({ health }: { health: ServerHealth | null }) {
  const items = [
    ["systemd", health?.hasSystemd],
    ["mihomo", Boolean(health?.mihomoPath)],
    ["service", health?.serviceActive === "active"],
    ["config", health?.hasConfig],
    ["subscription", health?.hasSubscription],
  ] as const;
  return (
    <div className="check-list">
      {items.map(([label, ok]) => (
        <div key={label} className="check-item">
          {ok ? <CheckCircle2 size={16} /> : <CircleDot size={16} />}
          {label}
        </div>
      ))}
    </div>
  );
}

function SecretInput({ value, onChange }: { value: string; onChange: (value: string) => void }) {
  return (
    <label className="field-label">
      <span>Subscription URL</span>
      <input
        type="password"
        value={value}
        autoComplete="off"
        placeholder="https://..."
        onChange={(event) => onChange(event.target.value)}
      />
    </label>
  );
}

function OutputPanel({ output }: { output: string }) {
  return (
    <div className="output-panel">
      <div className="panel-title">Output</div>
      <pre>{output}</pre>
    </div>
  );
}

function Metric({
  label,
  value,
  tone,
}: {
  label: string;
  value?: string | null;
  tone: "good" | "warn" | "neutral";
}) {
  return (
    <div className={`metric ${tone}`}>
      <span>{label}</span>
      <strong>{value || "unknown"}</strong>
    </div>
  );
}

function KeyValue({ label, value }: { label: string; value?: string | number | null }) {
  return (
    <div className="kv">
      <span>{label}</span>
      <strong>{value ?? "-"}</strong>
    </div>
  );
}

function StatusDot({ status }: { status?: string | null }) {
  const className = status === "online" ? "online" : status === "error" || status === "offline" ? "offline" : "";
  return <span className={`status-dot ${className}`} />;
}
