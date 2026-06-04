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
  GripVertical,
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
  RemoteProxyConfig,
  RemoteProxyInput,
  Server,
  ServerBootstrapInput,
  ServerHealth,
  SubscriptionInput,
  SubscriptionProfile,
} from "./types";

type Tab = "overview" | "install" | "subscription" | "nodes" | "config" | "logs" | "updates";
type ServerDraft = {
  displayName: string;
  hostName: string;
  user: string;
  port: string;
  password: string;
};
type SubscriptionDraft = {
  id: number | null;
  name: string;
  url: string;
};

const emptySubscriptionDraft: SubscriptionDraft = {
  id: null,
  name: "",
  url: "",
};

const defaultRemoteProxyInput: RemoteProxyInput = {
  enabled: true,
  httpProxy: "http://127.0.0.1:7890",
  httpsProxy: "http://127.0.0.1:7890",
  allProxy: "socks5h://127.0.0.1:7890",
  noProxy: "localhost,127.0.0.1,::1,10.40.2.0/24",
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
  const [subscriptions, setSubscriptions] = useState<SubscriptionProfile[]>([]);
  const [selectedSubscriptionId, setSelectedSubscriptionId] = useState<number | null>(null);
  const [subscriptionDraft, setSubscriptionDraft] =
    useState<SubscriptionDraft>(emptySubscriptionDraft);
  const [groups, setGroups] = useState<ProxyGroup[]>([]);
  const [selectedGroup, setSelectedGroup] = useState<string>("");
  const [measuredNodes, setMeasuredNodes] = useState<ProxyNode[]>([]);
  const [remoteProxy, setRemoteProxy] = useState<RemoteProxyConfig | null>(null);
  const [remoteProxyDraft, setRemoteProxyDraft] =
    useState<RemoteProxyInput>(defaultRemoteProxyInput);
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
  const selectedSubscription = useMemo(
    () => subscriptions.find((subscription) => subscription.id === selectedSubscriptionId) ?? null,
    [selectedSubscriptionId, subscriptions],
  );

  const loadServers = useCallback(async () => {
    const next = await api.listServers();
    setServers(next);
    setSelectedId((current) =>
      current && next.some((server) => server.id === current) ? current : next[0]?.id ?? null,
    );
  }, []);

  const loadSubscriptions = useCallback(async () => {
    const next = await api.listSubscriptions();
    setSubscriptions(next);
    const first = next[0] ?? null;
    setSelectedSubscriptionId(first?.id ?? null);
    setSubscriptionDraft(first ? draftFromSubscription(first) : emptySubscriptionDraft);
  }, []);

  useEffect(() => {
    void loadServers().catch((error) => setToast(String(error)));
    void loadSubscriptions().catch((error) => setToast(String(error)));
  }, [loadServers, loadSubscriptions]);

  useEffect(() => {
    if (!selectedId) {
      return;
    }
    setEgressResult(null);
    setRemoteProxy(null);
    void refreshHealth(selectedId);
    void refreshOperationLogs(selectedId);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [selectedId]);

  useEffect(() => {
    if (activeTab !== "config" || !selectedId) {
      return;
    }
    void refreshRemoteProxy(selectedId);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [activeTab, selectedId]);

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

  async function refreshRemoteProxy(id = selectedId) {
    if (!id) return;
    await run("读取远端代理", () => api.inspectRemoteProxy(id), (next) => {
      setRemoteProxy(next);
      setRemoteProxyDraft(remoteProxyInputFromConfig(next));
    });
  }

  async function command(label: string, work: () => Promise<CommandResult>): Promise<CommandResult | undefined> {
    const result = await run(label, work);
    if (result) {
      setCommandOutput(redactForDisplay(result.ok ? result.stdout || "ok" : result.stderr || "failed"));
      void refreshHealth();
      void refreshOperationLogs();
    }
    return result;
  }

  function updateRemoteProxyDraft(field: keyof RemoteProxyInput, value: string | boolean) {
    setRemoteProxyDraft((current) => ({ ...current, [field]: value }));
  }

  async function saveRemoteProxyDraft() {
    if (!selected) return;
    const result = await command("保存远端代理", () =>
      api.saveRemoteProxy(selected.id, remoteProxyDraft),
    );
    if (result?.ok) {
      void refreshRemoteProxy(selected.id);
    }
  }

  async function setRemoteProxyState(enabled: boolean) {
    if (!selected) return;
    const result = await command(enabled ? "打开远端代理" : "关闭远端代理", () =>
      api.setRemoteProxyEnabled(selected.id, enabled),
    );
    if (result?.ok) {
      void refreshRemoteProxy(selected.id);
    }
  }

  async function restartRemoteProxyService() {
    if (!selected) return;
    await command("重启远端代理服务", () => service(selected.id, "restart"));
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
    await run("安装 SSH key", () => api.bootstrapServerWithPassword(input), (next) => {
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

  async function deleteServer(serverToDelete: Server) {
    const confirmed = window.confirm(
      `删除本地服务器条目 "${serverToDelete.displayName}"？这不会删除远端服务器或远端 mihomo。`,
    );
    if (!confirmed) return;

    await run("删除服务器", () => api.deleteServer(serverToDelete.id), (next) => {
      setServers(next);
      const selectedStillExists = next.some((server) => server.id === selectedId);
      const shouldMoveSelection = serverToDelete.id === selectedId || !selectedStillExists;
      setSelectedId(shouldMoveSelection ? (next[0]?.id ?? null) : selectedId);
      if (shouldMoveSelection) {
        setHealth(null);
        setGroups([]);
        setMeasuredNodes([]);
        setLogs("");
        setOperationLogs([]);
        setCommandOutput("");
        setEgressResult(null);
      }
    });
  }

  function updateSubscriptionDraft(field: "name" | "url", value: string) {
    setSubscriptionDraft((current) => ({ ...current, [field]: value }));
  }

  function selectSubscriptionProfile(profile: SubscriptionProfile) {
    setSelectedSubscriptionId(profile.id);
    setSubscriptionDraft(draftFromSubscription(profile));
  }

  function newSubscriptionDraft() {
    setSelectedSubscriptionId(null);
    setSubscriptionDraft(emptySubscriptionDraft);
  }

  function upsertSubscription(profile: SubscriptionProfile) {
    setSubscriptions((current) =>
      sortSubscriptions([profile, ...current.filter((item) => item.id !== profile.id)]),
    );
    selectSubscriptionProfile(profile);
  }

  async function saveCurrentSubscription(): Promise<SubscriptionProfile | undefined> {
    const url = subscriptionDraft.url.trim();
    if (!url) {
      setToast("请先填写订阅链接");
      return undefined;
    }

    const input: SubscriptionInput = {
      id: subscriptionDraft.id,
      name: subscriptionDraft.name.trim() || null,
      url,
    };
    return run("保存订阅", () => api.saveSubscription(input), upsertSubscription);
  }

  async function markSubscriptionUsed(subscriptionId: number) {
    try {
      const profile = await api.markSubscriptionUsed(subscriptionId);
      upsertSubscription(profile);
    } catch {
      // 最近使用时间只影响本地排序，失败不影响远端订阅更新结果。
    }
  }

  async function saveAndRefreshSubscription() {
    if (!selected) return;
    const profile = await saveCurrentSubscription();
    if (!profile) return;
    const result = await command("更新订阅", () => api.updateSubscription(selected.id, profile.url));
    if (result?.ok) {
      void markSubscriptionUsed(profile.id);
    }
  }

  async function refreshSubscriptionProfile(profile: SubscriptionProfile) {
    if (!selected) return;
    selectSubscriptionProfile(profile);
    const result = await command("更新订阅", () => api.updateSubscription(selected.id, profile.url));
    if (result?.ok) {
      void markSubscriptionUsed(profile.id);
    }
  }

  async function installOrRepairSelected() {
    if (!selected) return;
    const subscriptionUrl = subscriptionDraft.url.trim() || selectedSubscription?.url || "";
    const result = await command("安装/修复 mihomo", () =>
      api.installOrRepairMihomo(selected.id, subscriptionUrl),
    );
    if (result?.ok && subscriptionDraft.id) {
      void markSubscriptionUsed(subscriptionDraft.id);
    }
  }

  async function deleteSubscriptionProfile(profile: SubscriptionProfile) {
    const confirmed = window.confirm(`删除本地订阅 "${profile.name}"？这不会删除远端已保存的订阅。`);
    if (!confirmed) return;

    await run("删除订阅", () => api.deleteSubscription(profile.id), (next) => {
      setSubscriptions(next);
      const replacement =
        selectedSubscriptionId === profile.id
          ? next[0] ?? null
          : next.find((item) => item.id === selectedSubscriptionId) ?? next[0] ?? null;
      setSelectedSubscriptionId(replacement?.id ?? null);
      setSubscriptionDraft(replacement ? draftFromSubscription(replacement) : emptySubscriptionDraft);
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
            <div
              key={server.id}
              className={`server-row ${server.id === selectedId ? "selected" : ""}`}
            >
              <button
                type="button"
                className="server-select"
                title={`选择 ${server.displayName}`}
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
              <button
                type="button"
                className="icon-button small server-delete-button"
                title={`删除本地服务器条目：${server.displayName}`}
                disabled={!!busy}
                onClick={(event) => {
                  event.stopPropagation();
                  void deleteServer(server);
                }}
              >
                <Trash2 size={15} />
              </button>
            </div>
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
              subscription={selectedSubscription}
              subscriptionUrl={subscriptionDraft.url}
              setSubscriptionUrl={(value) => updateSubscriptionDraft("url", value)}
              onInstall={installOrRepairSelected}
              onInspect={() => refreshHealth()}
              output={commandOutput}
            />
          )}

          {activeTab === "subscription" && (
            <SubscriptionPanel
              selected={selected}
              health={health}
              busy={busy}
              subscriptions={subscriptions}
              selectedSubscriptionId={selectedSubscriptionId}
              draft={subscriptionDraft}
              onDraftChange={updateSubscriptionDraft}
              onSelect={selectSubscriptionProfile}
              onNew={newSubscriptionDraft}
              onSaveRefresh={saveAndRefreshSubscription}
              onRefreshProfile={refreshSubscriptionProfile}
              onDeleteProfile={deleteSubscriptionProfile}
              onRefreshSaved={() =>
                selected && command("刷新已保存订阅", () => api.updateSubscription(selected.id))
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

          {activeTab === "config" && (
            <ConfigPanel
              health={health}
              selected={selected}
              busy={busy}
              remoteProxy={remoteProxy}
              remoteProxyDraft={remoteProxyDraft}
              commandOutput={commandOutput}
              onProxyDraftChange={updateRemoteProxyDraft}
              onProxyRead={() => refreshRemoteProxy()}
              onProxySave={saveRemoteProxyDraft}
              onProxyEnable={() => setRemoteProxyState(true)}
              onProxyDisable={() => setRemoteProxyState(false)}
              onProxyRestart={restartRemoteProxyService}
            />
          )}

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
        <span>SSH Password</span>
        <input
          type="password"
          value={props.draft.password}
          autoComplete="off"
          placeholder="server login password"
          title="Only used to install the app public key on this server. It is not saved."
          onChange={(event) => props.onChange("password", event.target.value)}
        />
      </label>
      <div className="key-chip" title={props.keyInfo?.privateKeyHint ?? "App-managed SSH key"}>
        <KeyRound size={14} />
        <span>{props.keyInfo?.privateKeyHint ?? "app SSH key"}</span>
      </div>
      <div className="button-row">
        <button
          className="command-button primary compact-button"
          disabled={!!props.busy || !props.draft.hostName || !props.draft.user || !props.draft.password}
          title="Use the current SSH password once to install the app public key, then save this server."
          onClick={props.onBootstrap}
        >
          <KeyRound size={15} />
          Install Key
        </button>
        <button
          className="command-button compact-button"
          disabled={!!props.busy || !props.draft.hostName || !props.draft.user}
          title="Save this server when the app public key is already allowed on the server."
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
  subscription: SubscriptionProfile | null;
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
        {props.subscription && (
          <div className="active-subscription-line">
            <CheckCircle2 size={15} />
            <span>{props.subscription.name}</span>
            <small>{subscriptionHost(props.subscription.url)}</small>
          </div>
        )}
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
  subscriptions: SubscriptionProfile[];
  selectedSubscriptionId: number | null;
  draft: SubscriptionDraft;
  onDraftChange: (field: "name" | "url", value: string) => void;
  onSelect: (profile: SubscriptionProfile) => void;
  onNew: () => void;
  onSaveRefresh: () => void;
  onRefreshProfile: (profile: SubscriptionProfile) => void;
  onDeleteProfile: (profile: SubscriptionProfile) => void;
  onRefreshSaved: () => void;
  output: string;
}) {
  return (
    <div className="split-layout">
      <div className="work-panel">
        <div className="panel-title">Subscription</div>
        <div className="subscription-toolbar">
          <button className="command-button" disabled={!!props.busy} onClick={props.onNew}>
            <Plus size={17} />
            新建
          </button>
          <button
            className="command-button"
            disabled={!props.selected || !!props.busy || !props.health?.hasSubscription}
            title="使用服务器上已保存的订阅地址刷新"
            onClick={props.onRefreshSaved}
          >
            <RefreshCcw size={17} />
            刷新远端保存
          </button>
        </div>

        <div className="subscription-list">
          {props.subscriptions.map((profile) => (
            <SubscriptionCard
              key={profile.id}
              profile={profile}
              selected={profile.id === props.selectedSubscriptionId}
              busy={props.busy}
              disabled={!props.selected}
              onSelect={() => props.onSelect(profile)}
              onRefresh={() => props.onRefreshProfile(profile)}
              onDelete={() => props.onDeleteProfile(profile)}
            />
          ))}
          {!props.subscriptions.length && (
            <div className="subscription-empty">还没有订阅。填写下面的名称和链接后保存。</div>
          )}
        </div>

        <div className="subscription-editor">
          <label className="field-label">
            <span>名称</span>
            <input
              value={props.draft.name}
              placeholder="Cyber Paws"
              onChange={(event) => props.onDraftChange("name", event.target.value)}
            />
          </label>
          <SecretInput value={props.draft.url} onChange={(value) => props.onDraftChange("url", value)} />
        </div>

        <div className="button-row">
          <button
            className="command-button primary"
            disabled={!props.selected || !!props.busy || !props.draft.url.trim()}
            onClick={props.onSaveRefresh}
          >
            <ListRestart size={17} />
            保存并刷新
          </button>
        </div>
        <div className="kv-grid compact">
          <KeyValue label="远端已保存" value={props.health?.hasSubscription ? "yes" : "no"} />
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

function SubscriptionCard(props: {
  profile: SubscriptionProfile;
  selected: boolean;
  busy: string | null;
  disabled: boolean;
  onSelect: () => void;
  onRefresh: () => void;
  onDelete: () => void;
}) {
  const time = props.profile.lastUsedAt ?? props.profile.updatedAt;
  return (
    <div className={`subscription-card ${props.selected ? "selected" : ""}`}>
      <button
        type="button"
        className="subscription-card-main"
        title={`选择 ${props.profile.name}`}
        onClick={props.onSelect}
      >
        <GripVertical size={17} />
        <span className="subscription-card-copy">
          <strong>{props.profile.name}</strong>
          <span>{subscriptionHost(props.profile.url)}</span>
          <small>{formatShortTime(time)}</small>
        </span>
      </button>
      <div className="subscription-card-actions">
        <button
          type="button"
          className="icon-button small"
          title="用这条订阅刷新远端配置"
          disabled={props.disabled || !!props.busy}
          onClick={props.onRefresh}
        >
          <RefreshCcw size={15} />
        </button>
        <button
          type="button"
          className="icon-button small danger-icon"
          title="删除本地订阅"
          disabled={!!props.busy}
          onClick={props.onDelete}
        >
          <Trash2 size={15} />
        </button>
      </div>
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

function ConfigPanel(props: {
  health: ServerHealth | null;
  selected: Server | null;
  busy: string | null;
  remoteProxy: RemoteProxyConfig | null;
  remoteProxyDraft: RemoteProxyInput;
  commandOutput: string;
  onProxyDraftChange: (field: keyof RemoteProxyInput, value: string | boolean) => void;
  onProxyRead: () => void;
  onProxySave: () => void;
  onProxyEnable: () => void;
  onProxyDisable: () => void;
  onProxyRestart: () => void;
}) {
  const { health, selected } = props;
  return (
    <div className="split-layout config-layout">
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
        <RemoteProxyPanel
          selected={selected}
          busy={props.busy}
          config={props.remoteProxy}
          draft={props.remoteProxyDraft}
          onDraftChange={props.onProxyDraftChange}
          onRead={props.onProxyRead}
          onSave={props.onProxySave}
          onEnable={props.onProxyEnable}
          onDisable={props.onProxyDisable}
          onRestart={props.onProxyRestart}
        />
      </div>
      <div className="output-panel">
        <div className="panel-title">YAML Preview</div>
        <pre>{redactForDisplay(health?.configPreview ?? "")}</pre>
        <div className="panel-title output-spacer">Command Output</div>
        <pre className="compact-output">{props.commandOutput}</pre>
      </div>
    </div>
  );
}

function RemoteProxyPanel(props: {
  selected: Server | null;
  busy: string | null;
  config: RemoteProxyConfig | null;
  draft: RemoteProxyInput;
  onDraftChange: (field: keyof RemoteProxyInput, value: string | boolean) => void;
  onRead: () => void;
  onSave: () => void;
  onEnable: () => void;
  onDisable: () => void;
  onRestart: () => void;
}) {
  const status = props.config?.enabled ? "已打开" : props.config ? "已关闭" : "未读取";
  return (
    <div className="remote-proxy-panel">
      <div className="section-heading">
        <div>
          <div className="panel-title">Remote Proxy</div>
          <div className={`proxy-status ${props.config?.enabled ? "on" : "off"}`}>
            {status}
            {props.config?.managed ? " · app 管理" : " · 未接管"}
          </div>
        </div>
        <button className="icon-button" title="读取远端代理" disabled={!props.selected || !!props.busy} onClick={props.onRead}>
          <RefreshCcw size={16} />
        </button>
      </div>

      <label className="toggle-line">
        <input
          type="checkbox"
          checked={props.draft.enabled}
          onChange={(event) => props.onDraftChange("enabled", event.target.checked)}
        />
        <span>保存时启用代理环境变量</span>
      </label>

      <div className="proxy-field-grid">
        <label className="field-label">
          <span>http_proxy / HTTP_PROXY</span>
          <input
            value={props.draft.httpProxy}
            placeholder="http://127.0.0.1:7890"
            onChange={(event) => props.onDraftChange("httpProxy", event.target.value)}
          />
        </label>
        <label className="field-label">
          <span>https_proxy / HTTPS_PROXY</span>
          <input
            value={props.draft.httpsProxy}
            placeholder="http://127.0.0.1:7890"
            onChange={(event) => props.onDraftChange("httpsProxy", event.target.value)}
          />
        </label>
        <label className="field-label">
          <span>all_proxy / ALL_PROXY</span>
          <input
            value={props.draft.allProxy}
            placeholder="socks5h://127.0.0.1:7890"
            onChange={(event) => props.onDraftChange("allProxy", event.target.value)}
          />
        </label>
        <label className="field-label">
          <span>no_proxy / NO_PROXY</span>
          <input
            value={props.draft.noProxy}
            placeholder="localhost,127.0.0.1,::1,10.40.2.0/24"
            onChange={(event) => props.onDraftChange("noProxy", event.target.value)}
          />
        </label>
      </div>

      <div className="button-row">
        <button className="command-button primary" disabled={!props.selected || !!props.busy} onClick={props.onSave}>
          <Save size={17} />
          保存
        </button>
        <button className="command-button" disabled={!props.selected || !!props.busy} onClick={props.onEnable}>
          <Power size={17} />
          打开
        </button>
        <button className="command-button danger" disabled={!props.selected || !!props.busy} onClick={props.onDisable}>
          <PowerOff size={17} />
          关闭
        </button>
        <button className="command-button" disabled={!props.selected || !!props.busy} onClick={props.onRestart}>
          <RotateCw size={17} />
          重启 Mihomo
        </button>
      </div>

      <div className="proxy-meta">
        <KeyValue label="Profile" value={props.config?.profilePath ?? "/etc/profile.d/mihomo-manager-proxy.sh"} />
        <KeyValue label="Detected" value={`${props.config?.detectedEnv.length ?? 0} vars`} />
      </div>

      {props.config?.detectedEnv.length ? (
        <div className="proxy-env-list">
          {props.config.detectedEnv.map((item) => (
            <div key={`${item.name}:${item.value}`} className="proxy-env-row">
              <span>{item.name}</span>
              <strong>{item.value}</strong>
            </div>
          ))}
        </div>
      ) : null}
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

function draftFromSubscription(profile: SubscriptionProfile): SubscriptionDraft {
  return {
    id: profile.id,
    name: profile.name,
    url: profile.url,
  };
}

function sortSubscriptions(profiles: SubscriptionProfile[]): SubscriptionProfile[] {
  return [...profiles].sort((left, right) => {
    const leftTime = left.lastUsedAt ?? left.updatedAt;
    const rightTime = right.lastUsedAt ?? right.updatedAt;
    return rightTime.localeCompare(leftTime) || left.name.localeCompare(right.name);
  });
}

function subscriptionHost(url: string): string {
  try {
    return new URL(url).hostname.replace(/^www\./, "") || "unknown host";
  } catch {
    return "invalid url";
  }
}

function formatShortTime(value?: string | null): string {
  if (!value) return "未使用";
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return "未知时间";
  return date.toLocaleString(undefined, {
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
  });
}

function remoteProxyInputFromConfig(config: RemoteProxyConfig): RemoteProxyInput {
  return {
    enabled: config.enabled,
    httpProxy: config.httpProxy ?? defaultRemoteProxyInput.httpProxy,
    httpsProxy: config.httpsProxy ?? defaultRemoteProxyInput.httpsProxy,
    allProxy: config.allProxy ?? defaultRemoteProxyInput.allProxy,
    noProxy: config.noProxy ?? defaultRemoteProxyInput.noProxy,
  };
}
