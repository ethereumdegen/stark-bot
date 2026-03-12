import { useState, useEffect, useCallback } from 'react';
import { Bot, ChevronRight, Clock, Zap, Link2, Brain, ListTodo, RefreshCw, Plus } from 'lucide-react';
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
}

interface HooksResponse {
  configured: boolean;
  hooks: unknown[];
  event_names: string[];
}

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
    loadDetail(capability, 'sessions');
  };

  const handleTabChange = (tab: DetailTab) => {
    setActiveTab(tab);
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

  const selectedAgent = agents.find(a => a.capability === selectedCapability);

  const capabilityColor: Record<string, string> = {
    crypto: 'text-amber-400 bg-amber-500/20',
    image_gen: 'text-pink-400 bg-pink-500/20',
    video_gen: 'text-purple-400 bg-purple-500/20',
    social_media: 'text-blue-400 bg-blue-500/20',
    general: 'text-green-400 bg-green-500/20',
  };

  const statusDot = (status: string) => {
    if (status === 'completed') return 'bg-green-400';
    if (status === 'failed') return 'bg-red-400';
    if (status === 'pending' || status === 'running') return 'bg-amber-400';
    return 'bg-slate-400';
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
                <button
                  onClick={() => handleReprovision(selectedAgent.capability)}
                  className="flex items-center gap-1.5 px-3 py-1.5 rounded-lg bg-slate-700 hover:bg-slate-600 text-slate-300 text-xs transition-colors"
                >
                  <RefreshCw className="w-3.5 h-3.5" />
                  Reprovision
                </button>
              </div>

              {/* Tabs */}
              <div className="flex gap-1 mb-4 bg-slate-800/50 rounded-lg p-1">
                {([
                  { key: 'sessions', label: 'Sessions', icon: Clock },
                  { key: 'hooks', label: 'Hooks', icon: Zap },
                  { key: 'memories', label: 'Memories', icon: Brain },
                  { key: 'tasks', label: 'Tasks', icon: ListTodo },
                  { key: 'integrations', label: 'Integrations', icon: Link2 },
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
                    <div className="space-y-2 max-h-[500px] overflow-y-auto">
                      {(Array.isArray(detailData) ? detailData : []).length === 0 ? (
                        <p className="text-slate-500 text-sm text-center py-4">No sessions yet</p>
                      ) : (
                        (detailData as StarflaskSession[]).map((session) => (
                          <div key={session.id} className="flex items-center justify-between p-3 rounded-lg bg-slate-700/30">
                            <div className="flex items-center gap-3 min-w-0">
                              <span className={`w-2 h-2 rounded-full flex-shrink-0 ${statusDot(session.status)}`} />
                              <span className="text-sm text-slate-300 font-mono truncate">{session.id.slice(0, 8)}...</span>
                              {session.hook_event && (
                                <span className="text-xs px-1.5 py-0.5 rounded bg-purple-500/20 text-purple-300">{session.hook_event}</span>
                              )}
                            </div>
                            <span className="text-xs text-slate-500">{session.status}</span>
                          </div>
                        ))
                      )}
                    </div>
                  ) : activeTab === 'hooks' ? (
                    <div>
                      {(detailData as HooksResponse)?.configured ? (
                        <div className="space-y-2">
                          <p className="text-green-400 text-sm mb-3">Hooks configured</p>
                          {(detailData as HooksResponse).event_names.map(name => (
                            <div key={name} className="flex items-center gap-2 p-2 rounded bg-slate-700/30">
                              <Zap className="w-3.5 h-3.5 text-amber-400" />
                              <span className="text-sm text-slate-300">{name}</span>
                            </div>
                          ))}
                        </div>
                      ) : (
                        <p className="text-slate-500 text-sm text-center py-4">No hooks configured</p>
                      )}
                    </div>
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
