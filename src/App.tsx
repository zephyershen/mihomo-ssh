import {
  Activity,
  Cable,
  CheckCircle2,
  CircleDot,
  Clipboard,
  Download,
  FileText,
  Gauge,
  Globe,
  KeyRound,
  ListRestart,
  Loader2,
  Network,
  Plus,
  Power,
  PowerOff,
  RefreshCcw,
  RotateCw,
  Save,
  Server as ServerIcon,
  Settings,
  ShieldAlert,
  TerminalSquare,
  Trash2,
  UploadCloud,
  Wifi,
  XCircle,
} from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { api } from "./lib/api";
import { redactForDisplay } from "./lib/redaction";
import {
  checkForAppUpdate,
  downloadInstallAndRelaunch,
  type AppUpdateProgress,
  type AppUpdateStatus,
} from "./lib/updater";
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
} from "./types";

type Tab = "overview" | "install" | "subscription" | "nodes" | "config" | "logs" | "updates";
type ServerDraft = {
  displayName: string;
  hostName: string;
  user: string;
  port: string;
  password: string;
};

const tabs: Array<{ id: Tab; label: string; icon: typeof Activity }> = [
  { id: "overview", label: "概览", icon: Gauge },
  { id: "install", label: "安装/健康", icon: Download },
  { id: "subscription", label: "订阅", icon: UploadCloud },
  { id: "nodes", label: "节点", icon: Network },
  { id: "config", label: "配置", icon: Settings },
  { id: "logs", label: "日志", icon: TerminalSquare },
  { id: "updates", label: "更新", icon: Download },
];

export function App() {
  const [servers, setServers] = useState<Server[]>([]);
  const [selectedId, setSelectedId] = useState<number | null>(null);
  const [showAddServer, setShowAddServer] = useState(false);
  const [serverDraft, setServerDraft] = useState<ServerDraft>({
    displayName: "",
    hostName: "",
    user: "root",
    port: "22",
    password: "",
  });
  const [managedKey, setManagedKey] = useState<ManagedSshKeyInfo | null>(null);
  const [health, setHealth] = useState<ServerHealth | null>(null);
  const [activeTab, setActiveTab] = useState<Tab>("overview");
  const [busy, setBusy] = useState<string | null>(null);
  const [toast, setToast] = useState<string>("");
  const [commandOutput, setCommandOutput] = useState<string>("");
  const [egressUrl, setEgressUrl] = useState("https://www.gstatic.com/generate_204");
  const [egressResult, setEgressResult] = useState<EgressTestResult | null>(null);
  const [subscriptionUrl, setSubscriptionUrl] = useState("");
  const [groups, setGroups] = useState<ProxyGroup[]>([]);
  const [selectedGroup, setSelectedGroup] = useState<string>("");
  const [measuredNodes, setMeasuredNodes] = useState<ProxyNode[]>([]);
  const [logs, setLogs] = useState("");
  const [operationLogs, setOperationLogs] = useState<OperationLog[]>([]);
  const [updateStatus, setUpdateStatus] = useState<AppUpdateStatus | null>(null);
  const [updateProgress, setUpdateProgress] = useState<AppUpdateProgress>({
    phase: "idle",
    message: "",
  });

  const selected = useMemo(
    () => servers.find((server) => server.id === selectedId) ?? null,
    [servers, selectedId],
  );

  const loadServers = useCallback(async () => {
    const next = await api.listServers();
    setServers(next);
    setSelectedId((current) =>
      current && next.some((server) => server.id === current) ? current : next[0]?.id ?? null,
    );
  }, []);

  useEffect(() => {
    void loadServers().catch((error) => setToast(String(error)));
  }, [loadServers]);

  useEffect(() => {
    if (!selectedId) {
      return;
    }
    setEgressResult(null);
    void refreshHealth(selectedId);
    void refreshOperationLogs(selectedId);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [selectedId]);

  useEffect(() => {
    if (!toast || busy) {
      return;
    }

    const timeout = window.setTimeout(
      () => setToast(""),
      toast.includes("失败") ? 6500 : 3200,
    );
    return () => window.clearTimeout(timeout);
  }, [busy, toast]);

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

  async function toggleAddServer() {
    setShowAddServer((current) => !current);
    if (!managedKey) {
      await run("准备 SSH key", api.getManagedSshKey, setManagedKey);
    }
  }

  function updateServerDraft(field: keyof ServerDraft, value: string) {
    setServerDraft((current) => ({ ...current, [field]: value }));
  }

  function manualServerInput(): ManualServerInput {
    return {
      displayName: serverDraft.displayName || null,
      hostName: serverDraft.hostName,
      user: serverDraft.user,
      port: Number(serverDraft.port) || 22,
    };
  }

  function selectAddedServer(next: Server[], input: ManualServerInput) {
    const port = input.port || 22;
    const match = next.find(
      (server) =>
        server.source === "manual" &&
        server.hostName === input.hostName.trim() &&
        server.user === input.user.trim() &&
        (server.port || 22) === port,
    );
    setSelectedId(match?.id ?? next[0]?.id ?? null);
    setHealth(null);
    setGroups([]);
    setMeasuredNodes([]);
    setCommandOutput("");
    setEgressResult(null);
  }

  async function addManualServer() {
    const input = manualServerInput();
    await run("添加服务器", () => api.addManualServer(input), (next) => {
      setServers(next);
      selectAddedServer(next, input);
      setShowAddServer(false);
      setServerDraft((current) => ({ ...current, password: "" }));
    });
  }

  async function bootstrapServer() {
    const input: ServerBootstrapInput = {
      ...manualServerInput(),
      password: serverDraft.password,
    };
    await run("初始化 SSH key", () => api.bootstrapServerWithPassword(input), (next) => {
      setServers(next);
      selectAddedServer(next, input);
      setShowAddServer(false);
      setServerDraft((current) => ({ ...current, password: "" }));
    });
  }

  async function copyManagedPublicKey() {
    await run("复制公钥", async () => {
      const key = managedKey ?? (await api.getManagedSshKey());
      setManagedKey(key);
      await navigator.clipboard.writeText(key.publicKey);
      return key;
    });
  }

  async function deleteSelectedServer() {
    if (!selected) return;
    const confirmed = window.confirm(
      `删除本地服务器条目 "${selected.displayName}"？这不会删除远端服务器或远端 mihomo。`,
    );
    if (!confirmed) return;

    await run("删除服务器", () => api.deleteServer(selected.id), (next) => {
      setServers(next);
      setSelectedId(next[0]?.id ?? null);
      setHealth(null);
      setGroups([]);
      setMeasuredNodes([]);
      setLogs("");
      setOperationLogs([]);
      setCommandOutput("");
      setEgressResult(null);
    });
  }

  async function testEgress() {
    if (!selected) return;
    const result = await run(
      "测试外网访问",
      () => api.testServerEgress(selected.id, egressUrl),
      (value) => setEgressResult(value),
    );
    if (result) {
      setCommandOutput(redactForDisplay(result.output.ok ? result.output.stdout : result.output.stderr));
      void refreshOperationLogs();
    }
  }

  async function loadProxyGroups() {
    if (!selected) return;
    await run("加载节点", () => api.listProxyGroups(selected.id), (next) => {
      setGroups(next);
      setSelectedGroup((current) => current || next[0]?.name || "");
      setMeasuredNodes([]);
    });
  }

  function changeSelectedGroup(group: string) {
    setSelectedGroup(group);
    setMeasuredNodes([]);
  }

  async function measureDelay() {
    if (!selected || !selectedGroup) return;
    await run("测速", () => api.measureProxyDelay(selected.id, selectedGroup), setMeasuredNodes);
  }

  function mergeNodeResult(nodes: ProxyNode[], result: ProxyNode): ProxyNode[] {
    if (!nodes.length) return [result];
    return nodes.map((node) =>
      node.name === result.name
        ? {
            ...node,
            delayMs: result.delayMs,
            alive: result.alive,
            nodeType: node.nodeType ?? result.nodeType,
            udp: node.udp ?? result.udp,
          }
        : node,
    );
  }

  async function testSingleNode(node: string) {
    if (!selected) return;
    await run("测试节点", () => api.measureProxyNodeDelay(selected.id, node), (result) => {
      setGroups((current) =>
        current.map((group) =>
          group.name === selectedGroup ? { ...group, nodes: mergeNodeResult(group.nodes, result) } : group,
        ),
      );
      setMeasuredNodes((current) =>
        mergeNodeResult(current.length ? current : currentGroup?.nodes ?? [], result),
      );
    });
  }

  async function selectNode(group: string, node: string) {
    if (!selected) return;
    await run("切换节点", () => api.selectProxyNode(selected.id, group, node), (next) => {
      setGroups(next);
    });
  }

  async function readLogs() {
    if (!selected) return;
    await run("读取日志", () => api.readMihomoLogs(selected.id, 240), (value) =>
      setLogs(redactForDisplay(value)),
    );
  }

  async function checkUpdates() {
    setUpdateProgress({ phase: "checking", message: "正在检查更新" });
    const result = await run("检查软件更新", checkForAppUpdate, (value) => {
      setUpdateStatus(value);
      setUpdateProgress({ phase: "idle", message: "" });
    });
    if (!result) {
      setUpdateProgress({ phase: "error", message: "检查更新失败" });
    }
  }

  async function installUpdate() {
    setBusy("安装软件更新");
    setToast("");
    try {
      await downloadInstallAndRelaunch(setUpdateProgress);
    } catch (error) {
      setUpdateProgress({ phase: "error", message: String(error) });
      setToast(`安装更新失败：${String(error)}`);
    } finally {
      setBusy(null);
    }
  }

  const currentGroup = groups.find((group) => group.name === selectedGroup);
  const displayedNodes = measuredNodes.length ? measuredNodes : currentGroup?.nodes ?? [];
  const selectedNodeName = currentGroup?.now ?? null;

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
          <button className="icon-button" title="添加服务器" onClick={toggleAddServer} disabled={!!busy}>
            <Plus size={17} />
          </button>
          <button className="icon-button" title="导入 SSH config" onClick={importHosts} disabled={!!busy}>
            <UploadCloud size={17} />
          </button>
          <button className="icon-button" title="刷新服务器列表" onClick={loadServers} disabled={!!busy}>
            <RefreshCcw size={17} />
          </button>
        </div>

        {showAddServer && (
          <AddServerPanel
            draft={serverDraft}
            keyInfo={managedKey}
            busy={busy}
            onChange={updateServerDraft}
            onAdd={addManualServer}
            onBootstrap={bootstrapServer}
            onCopyKey={copyManagedPublicKey}
          />
        )}

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
            <button className="tool-button danger-text" title="删除本地服务器条目" disabled={!selected || !!busy} onClick={deleteSelectedServer}>
              <Trash2 size={16} />
              Delete
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
              egressUrl={egressUrl}
              setEgressUrl={setEgressUrl}
              egressResult={egressResult}
              onTestEgress={testEgress}
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
              setSelectedGroup={changeSelectedGroup}
              nodes={displayedNodes}
              selectedNodeName={selectedNodeName}
              onOpenTunnel={() => selected && run("打开控制通道", () => api.openControllerTunnel(selected.id))}
              onCloseTunnel={() => selected && run("关闭控制通道", () => api.closeControllerTunnel(selected.id))}
              onLoad={loadProxyGroups}
              onMeasure={measureDelay}
              onTestNode={testSingleNode}
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

          {activeTab === "updates" && (
            <UpdatesPanel
              busy={busy}
              updateStatus={updateStatus}
              progress={updateProgress}
              onCheck={checkUpdates}
              onInstall={installUpdate}
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

function AddServerPanel(props: {
  draft: ServerDraft;
  keyInfo: ManagedSshKeyInfo | null;
  busy: string | null;
  onChange: (field: keyof ServerDraft, value: string) => void;
  onAdd: () => void;
  onBootstrap: () => void;
  onCopyKey: () => void;
}) {
  return (
    <div className="add-server-panel">
      <label className="field-label">
        <span>Name</span>
        <input
          value={props.draft.displayName}
          placeholder="codex-box"
          onChange={(event) => props.onChange("displayName", event.target.value)}
        />
      </label>
      <label className="field-label">
        <span>Host / IP</span>
        <input
          value={props.draft.hostName}
          placeholder="10.40.2.39"
          onChange={(event) => props.onChange("hostName", event.target.value)}
        />
      </label>
      <div className="field-grid">
        <label className="field-label">
          <span>User</span>
          <input
            value={props.draft.user}
            placeholder="root"
            onChange={(event) => props.onChange("user", event.target.value)}
          />
        </label>
        <label className="field-label">
          <span>Port</span>
          <input
            value={props.draft.port}
            inputMode="numeric"
            placeholder="22"
            onChange={(event) => props.onChange("port", event.target.value)}
          />
        </label>
      </div>
      <label className="field-label">
        <span>Password</span>
        <input
          type="password"
          value={props.draft.password}
          autoComplete="off"
          placeholder="one-time"
          onChange={(event) => props.onChange("password", event.target.value)}
        />
      </label>
      <div className="key-chip" title={props.keyInfo?.privateKeyHint ?? "managed SSH key"}>
        <KeyRound size={14} />
        <span>{props.keyInfo?.privateKeyHint ?? "managed key"}</span>
      </div>
      <div className="button-row">
        <button
          className="command-button primary compact-button"
          disabled={!!props.busy || !props.draft.hostName || !props.draft.user || !props.draft.password}
          onClick={props.onBootstrap}
        >
          <KeyRound size={15} />
          Bootstrap
        </button>
        <button
          className="command-button compact-button"
          disabled={!!props.busy || !props.draft.hostName || !props.draft.user}
          onClick={props.onAdd}
        >
          <Plus size={15} />
          Add
        </button>
        <button className="icon-button" title="复制 app 公钥" disabled={!!props.busy} onClick={props.onCopyKey}>
          <Clipboard size={15} />
        </button>
      </div>
    </div>
  );
}

function UpdatesPanel(props: {
  busy: string | null;
  updateStatus: AppUpdateStatus | null;
  progress: AppUpdateProgress;
  onCheck: () => void;
  onInstall: () => void;
}) {
  const { updateStatus, progress } = props;
  const progressText =
    progress.total && progress.downloaded != null
      ? `${Math.round((progress.downloaded / progress.total) * 100)}%`
      : progress.message;

  return (
    <div className="split-layout">
      <div className="work-panel">
        <div className="panel-title">Software Update</div>
        <div className={`update-card ${updateStatus?.state ?? "idle"}`}>
          {updateStatus?.state === "available" ? (
            <>
              <strong>Version {updateStatus.version}</strong>
              <span>{updateStatus.date ?? "Release available"}</span>
            </>
          ) : updateStatus ? (
            <>
              <strong>{updateStatus.state === "current" ? "Up to date" : "Update unavailable"}</strong>
              <span>{updateStatus.message}</span>
            </>
          ) : (
            <>
              <strong>Not checked</strong>
              <span>Release builds can check GitHub Releases for signed updates.</span>
            </>
          )}
        </div>

        <div className="button-row">
          <button className="command-button" disabled={!!props.busy} onClick={props.onCheck}>
            <RefreshCcw size={17} />
            Check
          </button>
          <button
            className="command-button primary"
            disabled={!!props.busy || updateStatus?.state !== "available"}
            onClick={props.onInstall}
          >
            <Download size={17} />
            Install
          </button>
        </div>

        {progress.message && (
          <div className={`update-progress ${progress.phase}`}>
            {progress.phase === "downloading" && progress.total ? (
              <div className="progress-bar" aria-label="Update download progress">
                <span style={{ width: progressText }} />
              </div>
            ) : null}
            <span>{progressText}</span>
          </div>
        )}
      </div>

      <div className="output-panel">
        <div className="panel-title">Release Notes</div>
        <pre>
          {updateStatus?.state === "available"
            ? updateStatus.body || "No release notes provided."
            : "No update release notes loaded."}
        </pre>
      </div>
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
  egressUrl: string;
  setEgressUrl: (value: string) => void;
  egressResult: EgressTestResult | null;
  onTestEgress: () => void;
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

      <div className="wide-panel">
        <div className="panel-title">External Access</div>
        <label className="field-label">
          <span>Target URL</span>
          <input
            value={props.egressUrl}
            placeholder="https://www.gstatic.com/generate_204"
            onChange={(event) => props.setEgressUrl(event.target.value)}
          />
        </label>
        <div className="button-row">
          <button className="command-button" disabled={!selected || !!busy} onClick={props.onTestEgress}>
            <Globe size={17} />
            Test URL
          </button>
        </div>
        {props.egressResult && (
          <div className={`egress-result ${props.egressResult.ok ? "ok" : "error"}`} title={props.egressResult.url}>
            <strong>{props.egressResult.ok ? "reachable" : "failed"}</strong>
            <span>
              {props.egressResult.status ?? "-"}
              {props.egressResult.elapsedMs ? ` · ${props.egressResult.elapsedMs} ms` : ""}
            </span>
          </div>
        )}
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
  selectedNodeName: string | null;
  onOpenTunnel: () => void;
  onCloseTunnel: () => void;
  onLoad: () => void;
  onMeasure: () => void;
  onTestNode: (node: string) => void;
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
            {props.nodes.map((node) => {
              const selected = node.name === props.selectedNodeName;
              const delay = node.alive === false ? "fail" : node.delayMs ? `${node.delayMs} ms` : "-";
              return (
                <tr key={node.name} className={selected ? "selected-node-row" : ""}>
                  <td className="node-name" title={node.name}>{node.name}</td>
                  <td title={node.nodeType ?? "-"}>{node.nodeType ?? "-"}</td>
                  <td>{node.udp == null ? "-" : node.udp ? "yes" : "no"}</td>
                  <td title={delay}>
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
                      title="测试此节点"
                      disabled={!!props.busy}
                      onClick={() => props.onTestNode(node.name)}
                    >
                      <Activity size={15} />
                    </button>
                    <button
                      className={`icon-button small ${selected ? "selected-node-button" : ""}`}
                      title={selected ? "当前节点" : "切换到此节点"}
                      disabled={!props.selectedGroup || !!props.busy || selected}
                      onClick={() => props.onSelect(props.selectedGroup, node.name)}
                    >
                      <CheckCircle2 size={15} />
                    </button>
                  </td>
                </tr>
              );
            })}
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
          <KeyValue label="Port" value={selected?.port} />
          <KeyValue label="Identity" value={selected?.identityFileHint || "ssh config/default key"} />
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
