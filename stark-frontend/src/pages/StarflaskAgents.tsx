import { useState, useEffect, useCallback } from 'react';
import { Bot, ChevronRight, Clock, Zap, Link2, Brain, ListTodo, RefreshCw, Plus, Trash2, Play, ExternalLink } from 'lucide-react';
import Card, { CardContent } from '@/components/ui/Card';
import { apiFetch } from '@/lib/api';

interface StarflaskAgent {
  id: number;
  capability: string;
  agent_id: string;
  name: string;
  description: string;
  pack_hashes: string[];
  status: string;
  created_at: string;
  updated_at: string;
}

interface StarflaskSession {
  id: string;
  agent_id: string;
  status: string;
  result?: unknown;
  error?: string;
  hook_event?: string;
  hook_payload?: unknown;
}

interface HooksResponse {
  configured: boolean;
  hooks: unknown[];
  event_names: string[];
}

interface StarflaskIntegration {
  id: string;
  agent_id: string;
  platform: string;
  enabled: boolean;
}

const AVAILABLE_PLATFORMS = ['discord', 'telegram', 'slack', 'twitter', 'webhook'];

type DetailTab = 'sessions' | 'hooks' | 'memories' | 'tasks' | 'integrations';

export default function StarflaskAgents() {
  const [agents, setAgents] = useState<StarflaskAgent[]>([]);
  const [remoteAgents, setRemoteAgents] = useState<{ id: string; name: string; description?: string; active: boolean }[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [selectedCapability, setSelectedCapability] = useState<string | null>(null);
  const [activeTab, setActiveTab] = useState<DetailTab>('sessions');
  const [detailData, setDetailData] = useState<unknown>(null);
  const [detailLoading, setDetailLoading] = useState(false);
  const [provisioning, setProvisioning] = useState(false);
  const [expandedSession, setExpandedSession] = useState<string | null>(null);
  const [addingIntegration, setAddingIntegration] = useState(false);
  const [firingHook, setFiringHook] = useState<string | null>(null);
  const [hookPayload, setHookPayload] = useState('{}');

  const fetchAgents = useCallback(async () => {
    setLoading(true);
    try {
      const [local, remote] = await Promise.all([
        apiFetch<StarflaskAgent[]>('/starflask/agents').catch(() => []),
        apiFetch<{ id: string; name: string; description?: string; active: boolean }[]>('/starflask/remote/agents').catch(() => []),
      ]);
      setAgents(local);
      setRemoteAgents(remote);
      setError(null);
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Failed to load agents');
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => { fetchAgents(); }, [fetchAgents]);

  const loadDetail = useCallback(async (capability: string, tab: DetailTab) => {
    setDetailLoading(true);
    try {
      let data;
      switch (tab) {
        case 'sessions':
          data = await apiFetch<StarflaskSession[]>(`/starflask/agents/${capability}/sessions?limit=50`);
          break;
        case 'hooks':
          data = await apiFetch<HooksResponse>(`/starflask/agents/${capability}/hooks`);
          break;
        case 'memories':
          data = await apiFetch(`/starflask/agents/${capability}/memories?limit=50`);
          break;
        case 'tasks':
          data = await apiFetch(`/starflask/agents/${capability}/tasks`);
          break;
        case 'integrations':
          data = await apiFetch(`/starflask/agents/${capability}/integrations`);
          break;
      }
      setDetailData(data);
    } catch {
      setDetailData(null);
    } finally {
      setDetailLoading(false);
    }
  }, []);

  const handleSelectAgent = (capability: string) => {
    setSelectedCapability(capability);
    setActiveTab('sessions');
    setExpandedSession(null);
    loadDetail(capability, 'sessions');
  };

  const handleTabChange = (tab: DetailTab) => {
    setActiveTab(tab);
    setExpandedSession(null);
    if (selectedCapability) loadDetail(selectedCapability, tab);
  };

  const handleProvision = async () => {
    setProvisioning(true);
    try {
      await apiFetch('/starflask/provision', { method: 'POST' });
      await fetchAgents();
    } catch {
      // ignore
    } finally {
      setProvisioning(false);
    }
  };

  const handleReprovision = async (capability: string) => {
    try {
      await apiFetch(`/starflask/reprovision/${capability}`, { method: 'POST' });
      await fetchAgents();
      if (selectedCapability === capability) loadDetail(capability, activeTab);
    } catch {
      // ignore
    }
  };

  const handleAddIntegration = async (platform: string) => {
    if (!selectedCapability) return;
    const agent = agents.find(a => a.capability === selectedCapability);
    if (!agent) return;
    setAddingIntegration(true);
    try {
      await apiFetch(`/starflask/remote/agents/${agent.agent_id}/integrations`, {
        method: 'POST',
        body: JSON.stringify({ platform }),
      });
      await loadDetail(selectedCapability, 'integrations');
    } catch {
      // ignore
    } finally {
      setAddingIntegration(false);
    }
  };

  const handleDeleteIntegration = async (integrationId: string) => {
    if (!selectedCapability) return;
    const agent = agents.find(a => a.capability === selectedCapability);
    if (!agent) return;
    try {
      await apiFetch(`/starflask/remote/agents/${agent.agent_id}/integrations/${integrationId}`, {
        method: 'DELETE',
      });
      await loadDetail(selectedCapability, 'integrations');
    } catch {
      // ignore
    }
  };

  const handleFireHook = async (eventName: string) => {
    if (!selectedCapability) return;
    setFiringHook(eventName);
    try {
      let payload = {};
      try { payload = JSON.parse(hookPayload); } catch { /* use empty */ }
      await apiFetch(`/starflask/agents/${selectedCapability}/fire_hook`, {
        method: 'POST',
        body: JSON.stringify({ event: eventName, payload, wait: false }),
      });
      // Refresh sessions to show the new hook-triggered session
      if (selectedCapability) loadDetail(selectedCapability, 'hooks');
    } catch {
      // ignore
    } finally {
      setFiringHook(null);
    }
  };

  const selectedAgent = agents.find(a => a.capability === selectedCapability);

  const capabilityColor: Record<string, string> = {
    crypto: 'text-amber-400 bg-amber-500/20',
    image_gen: 'text-pink-400 bg-pink-500/20',
    video_gen: 'text-purple-400 bg-purple-500/20',
    discord_moderator: 'text-indigo-400 bg-indigo-500/20',
    telegram_moderator: 'text-sky-400 bg-sky-500/20',
    general: 'text-green-400 bg-green-500/20',
  };

  const platformIcon: Record<string, string> = {
    discord: 'bg-indigo-500/20 text-indigo-400 border-indigo-500/30',
    telegram: 'bg-sky-500/20 text-sky-400 border-sky-500/30',
    slack: 'bg-emerald-500/20 text-emerald-400 border-emerald-500/30',
    twitter: 'bg-blue-500/20 text-blue-400 border-blue-500/30',
    webhook: 'bg-orange-500/20 text-orange-400 border-orange-500/30',
  };

  const statusDot = (status: string) => {
    if (status === 'completed') return 'bg-green-400';
    if (status === 'failed') return 'bg-red-400';
    if (status === 'pending' || status === 'running') return 'bg-amber-400 animate-pulse';
    return 'bg-slate-400';
  };

  const renderSessions = () => {
    const sessions = Array.isArray(detailData) ? detailData as StarflaskSession[] : [];
    if (sessions.length === 0) {
      return <p className="text-slate-500 text-sm text-center py-4">No sessions yet</p>;
    }
    return (
      <div className="space-y-2 max-h-[500px] overflow-y-auto">
        {sessions.map((session) => (
          <div key={session.id}>
            <button
              onClick={() => setExpandedSession(expandedSession === session.id ? null : session.id)}
              className="w-full flex items-center justify-between p-3 rounded-lg bg-slate-700/30 hover:bg-slate-700/50 transition-colors"
            >
              <div className="flex items-center gap-3 min-w-0">
                <span className={`w-2 h-2 rounded-full flex-shrink-0 ${statusDot(session.status)}`} />
                <span className="text-sm text-slate-300 font-mono truncate">{session.id.slice(0, 8)}...</span>
                {session.hook_event && (
                  <span className="text-xs px-1.5 py-0.5 rounded bg-purple-500/20 text-purple-300">
                    <Zap className="w-3 h-3 inline mr-1" />{session.hook_event}
                  </span>
                )}
              </div>
              <div className="flex items-center gap-2">
                {session.error && <span className="text-xs text-red-400">error</span>}
                <span className="text-xs text-slate-500">{session.status}</span>
              </div>
            </button>
            {expandedSession === session.id && (
              <div className="mt-1 ml-5 p-3 rounded-lg bg-slate-800/80 border border-slate-700/50 space-y-2">
                <div className="text-xs text-slate-500">
                  <span className="font-medium text-slate-400">Session ID:</span> <span className="font-mono">{session.id}</span>
                </div>
                {session.hook_event && (
                  <div className="text-xs">
                    <span className="font-medium text-purple-400">Hook Event:</span>{' '}
                    <span className="text-slate-300">{session.hook_event}</span>
                  </div>
                )}
                {!!session.hook_payload && (
                  <div>
                    <span className="text-xs font-medium text-purple-400">Hook Payload:</span>
                    <pre className="text-xs text-slate-400 mt-1 p-2 rounded bg-slate-900/50 overflow-auto max-h-32">
                      {JSON.stringify(session.hook_payload, null, 2)}
                    </pre>
                  </div>
                )}
                {session.error && (
                  <div className="text-xs text-red-400">
                    <span className="font-medium">Error:</span> {session.error}
                  </div>
                )}
                {!!session.result && (
                  <div>
                    <span className="text-xs font-medium text-slate-400">Result:</span>
                    <pre className="text-xs text-slate-400 mt-1 p-2 rounded bg-slate-900/50 overflow-auto max-h-48">
                      {JSON.stringify(session.result, null, 2)}
                    </pre>
                  </div>
                )}
              </div>
            )}
          </div>
        ))}
      </div>
    );
  };

  const renderHooks = () => {
    const hooks = detailData as HooksResponse | null;
    if (!hooks?.configured) {
      return <p className="text-slate-500 text-sm text-center py-4">No hooks configured</p>;
    }
    return (
      <div className="space-y-3">
        <div className="flex items-center justify-between">
          <p className="text-green-400 text-sm flex items-center gap-1.5">
            <span className="w-2 h-2 rounded-full bg-green-400" />
            Hooks active — {hooks.event_names.length} event{hooks.event_names.length !== 1 ? 's' : ''}
          </p>
        </div>

        {hooks.event_names.map(name => (
          <div key={name} className="p-3 rounded-lg bg-slate-700/30 border border-slate-700/50">
            <div className="flex items-center justify-between mb-2">
              <div className="flex items-center gap-2">
                <Zap className="w-4 h-4 text-amber-400" />
                <span className="text-sm font-medium text-slate-200">{name}</span>
              </div>
              <button
                onClick={() => handleFireHook(name)}
                disabled={firingHook === name}
                className="flex items-center gap-1.5 px-2.5 py-1 rounded-md bg-amber-500/10 border border-amber-500/30 text-amber-400 hover:bg-amber-500/20 transition-colors text-xs disabled:opacity-50"
              >
                {firingHook === name ? (
                  <>Firing...</>
                ) : (
                  <><Play className="w-3 h-3" />Fire</>
                )}
              </button>
            </div>
            {firingHook === name || (firingHook === null && name === hooks.event_names[0]) ? null : null}
          </div>
        ))}

        {/* Payload editor for firing hooks */}
        <div className="p-3 rounded-lg bg-slate-800/50 border border-slate-700/50">
          <label className="text-xs font-medium text-slate-400 block mb-1.5">Hook Payload (JSON)</label>
          <textarea
            value={hookPayload}
            onChange={(e) => setHookPayload(e.target.value)}
            rows={3}
            className="w-full bg-slate-900/50 border border-slate-700 rounded-md p-2 text-xs font-mono text-slate-300 focus:outline-none focus:border-amber-500/50 resize-none"
            placeholder='{"key": "value"}'
          />
        </div>

        {/* Recent hook-triggered sessions */}
        {hooks.hooks.length > 0 && (
          <div>
            <h4 className="text-xs font-medium text-slate-400 mb-2">Hook Details</h4>
            <div className="space-y-1">
              {hooks.hooks.map((hook, i) => (
                <pre key={i} className="text-xs text-slate-400 p-2 rounded bg-slate-900/50 overflow-auto max-h-32">
                  {JSON.stringify(hook, null, 2)}
                </pre>
              ))}
            </div>
          </div>
        )}
      </div>
    );
  };

  const renderIntegrations = () => {
    const integrations = Array.isArray(detailData) ? detailData as StarflaskIntegration[] : [];
    const existingPlatforms = new Set(integrations.map(i => i.platform));

    return (
      <div className="space-y-4">
        {/* Existing integrations */}
        {integrations.length === 0 ? (
          <p className="text-slate-500 text-sm text-center py-2">No integrations connected</p>
        ) : (
          <div className="space-y-2">
            {integrations.map((integration) => (
              <div
                key={integration.id}
                className={`flex items-center justify-between p-3 rounded-lg border ${
                  platformIcon[integration.platform] || 'bg-slate-500/20 text-slate-400 border-slate-500/30'
                }`}
              >
                <div className="flex items-center gap-3">
                  <Link2 className="w-4 h-4" />
                  <div>
                    <span className="text-sm font-medium capitalize">{integration.platform}</span>
                    <span className="text-xs text-slate-500 ml-2 font-mono">{integration.id.slice(0, 8)}...</span>
                  </div>
                  <span className={`text-xs px-1.5 py-0.5 rounded ${
                    integration.enabled ? 'bg-green-500/20 text-green-400' : 'bg-red-500/20 text-red-400'
                  }`}>
                    {integration.enabled ? 'enabled' : 'disabled'}
                  </span>
                </div>
                <button
                  onClick={() => handleDeleteIntegration(integration.id)}
                  className="p-1.5 rounded-md hover:bg-red-500/20 text-slate-500 hover:text-red-400 transition-colors"
                  title="Remove integration"
                >
                  <Trash2 className="w-3.5 h-3.5" />
                </button>
              </div>
            ))}
          </div>
        )}

        {/* Add integration */}
        <div className="border-t border-slate-700/50 pt-4">
          <h4 className="text-xs font-medium text-slate-400 mb-3">Add Integration</h4>
          <div className="flex flex-wrap gap-2">
            {AVAILABLE_PLATFORMS.filter(p => !existingPlatforms.has(p)).map(platform => (
              <button
                key={platform}
                onClick={() => handleAddIntegration(platform)}
                disabled={addingIntegration}
                className={`flex items-center gap-1.5 px-3 py-2 rounded-lg border text-xs font-medium transition-colors hover:opacity-80 disabled:opacity-50 capitalize ${
                  platformIcon[platform] || 'bg-slate-500/20 text-slate-400 border-slate-500/30'
                }`}
              >
                <Plus className="w-3 h-3" />
                {platform}
              </button>
            ))}
            {AVAILABLE_PLATFORMS.filter(p => !existingPlatforms.has(p)).length === 0 && (
              <p className="text-slate-500 text-xs">All platforms connected</p>
            )}
          </div>
        </div>
      </div>
    );
  };

  return (
    <div className="p-8">
      <div className="flex items-center justify-between mb-8">
        <div>
          <h1 className="text-2xl font-bold text-white mb-1">Starflask Agents</h1>
          <p className="text-slate-400 text-sm">
            {agents.length} provisioned · {remoteAgents.length} on Starflask
          </p>
        </div>
        <div className="flex gap-2">
          <button onClick={fetchAgents} className="p-2 rounded-lg bg-slate-700 hover:bg-slate-600 text-slate-300 transition-colors">
            <RefreshCw className="w-4 h-4" />
          </button>
          <button
            onClick={handleProvision}
            disabled={provisioning}
            className="flex items-center gap-2 px-4 py-2 rounded-lg bg-stark-500/20 border border-stark-500/30 text-stark-400 hover:bg-stark-500/30 transition-colors text-sm font-medium disabled:opacity-50"
          >
            <Plus className="w-4 h-4" />
            {provisioning ? 'Provisioning...' : 'Provision from Seed'}
          </button>
        </div>
      </div>

      {error && (
        <div className="mb-6 p-4 rounded-lg bg-red-500/10 border border-red-500/30 text-red-300 text-sm">
          {error}
        </div>
      )}

      <div className="grid grid-cols-1 lg:grid-cols-3 gap-6">
        {/* Agent List */}
        <div className="space-y-3">
          {loading ? (
            <div className="text-slate-400 text-sm p-4">Loading agents...</div>
          ) : agents.length === 0 ? (
            <Card>
              <CardContent>
                <div className="text-center py-8">
                  <Bot className="w-12 h-12 text-slate-600 mx-auto mb-3" />
                  <p className="text-slate-400 text-sm mb-3">No agents provisioned yet</p>
                  <button
                    onClick={handleProvision}
                    className="px-4 py-2 rounded-lg bg-stark-500/20 border border-stark-500/30 text-stark-400 hover:bg-stark-500/30 transition-colors text-sm"
                  >
                    Provision Agents
                  </button>
                </div>
              </CardContent>
            </Card>
          ) : (
            agents.map((agent) => (
              <button
                key={agent.capability}
                onClick={() => handleSelectAgent(agent.capability)}
                className={`w-full text-left p-4 rounded-lg border transition-colors ${
                  selectedCapability === agent.capability
                    ? 'bg-slate-700/80 border-stark-500/50'
                    : 'bg-slate-800/50 border-slate-700 hover:bg-slate-700/50'
                }`}
              >
                <div className="flex items-center justify-between mb-2">
                  <span className={`text-xs px-2 py-0.5 rounded-full font-medium ${capabilityColor[agent.capability] || 'text-slate-400 bg-slate-500/20'}`}>
                    {agent.capability}
                  </span>
                  <ChevronRight className="w-4 h-4 text-slate-500" />
                </div>
                <h3 className="text-white font-medium text-sm">{agent.name}</h3>
                <p className="text-slate-400 text-xs mt-1">{agent.description}</p>
                <div className="flex items-center gap-2 mt-2">
                  <span className="text-xs text-slate-500 font-mono">{agent.agent_id.slice(0, 8)}...</span>
                  <span className={`text-xs px-1.5 py-0.5 rounded ${agent.status === 'provisioned' ? 'bg-green-500/20 text-green-400' : 'bg-slate-500/20 text-slate-400'}`}>
                    {agent.status}
                  </span>
                </div>
              </button>
            ))
          )}
        </div>

        {/* Agent Detail Panel */}
        <div className="lg:col-span-2">
          {selectedAgent ? (
            <div>
              <div className="flex items-center justify-between mb-4">
                <div>
                  <h2 className="text-lg font-semibold text-white">{selectedAgent.name}</h2>
                  <p className="text-sm text-slate-400">{selectedAgent.description}</p>
                </div>
                <div className="flex gap-2">
                  <a
                    href={`https://starflask.com/dashboard/agent/${selectedAgent.agent_id}`}
                    target="_blank"
                    rel="noopener noreferrer"
                    className="flex items-center gap-1.5 px-3 py-1.5 rounded-lg bg-stark-500/20 border border-stark-500/30 text-stark-400 hover:bg-stark-500/30 text-xs font-medium transition-colors"
                  >
                    <ExternalLink className="w-3.5 h-3.5" />
                    View on Starflask
                  </a>
                  <button
                    onClick={() => handleReprovision(selectedAgent.capability)}
                    className="flex items-center gap-1.5 px-3 py-1.5 rounded-lg bg-slate-700 hover:bg-slate-600 text-slate-300 text-xs transition-colors"
                  >
                    <RefreshCw className="w-3.5 h-3.5" />
                    Reprovision
                  </button>
                </div>
              </div>

              {/* Tabs */}
              <div className="flex gap-1 mb-4 bg-slate-800/50 rounded-lg p-1">
                {([
                  { key: 'sessions', label: 'Sessions', icon: Clock },
                  { key: 'hooks', label: 'Hooks', icon: Zap },
                  { key: 'integrations', label: 'Integrations', icon: Link2 },
                  { key: 'memories', label: 'Memories', icon: Brain },
                  { key: 'tasks', label: 'Tasks', icon: ListTodo },
                ] as { key: DetailTab; label: string; icon: typeof Clock }[]).map(tab => (
                  <button
                    key={tab.key}
                    onClick={() => handleTabChange(tab.key)}
                    className={`flex items-center gap-1.5 px-3 py-2 rounded-md text-xs font-medium transition-colors ${
                      activeTab === tab.key
                        ? 'bg-slate-700 text-white'
                        : 'text-slate-400 hover:text-slate-300'
                    }`}
                  >
                    <tab.icon className="w-3.5 h-3.5" />
                    {tab.label}
                  </button>
                ))}
              </div>

              {/* Detail Content */}
              <Card>
                <CardContent>
                  {detailLoading ? (
                    <div className="text-slate-400 text-sm py-8 text-center">Loading...</div>
                  ) : !detailData ? (
                    <div className="text-slate-500 text-sm py-8 text-center">No data available</div>
                  ) : activeTab === 'sessions' ? (
                    renderSessions()
                  ) : activeTab === 'hooks' ? (
                    renderHooks()
                  ) : activeTab === 'integrations' ? (
                    renderIntegrations()
                  ) : (
                    <pre className="text-xs text-slate-400 overflow-auto max-h-[500px] whitespace-pre-wrap">
                      {JSON.stringify(detailData, null, 2)}
                    </pre>
                  )}
                </CardContent>
              </Card>
            </div>
          ) : (
            <Card>
              <CardContent>
                <div className="text-center py-16">
                  <Bot className="w-16 h-16 text-slate-700 mx-auto mb-4" />
                  <p className="text-slate-500">Select an agent to view details</p>
                </div>
              </CardContent>
            </Card>
          )}
        </div>
      </div>
    </div>
  );
}
