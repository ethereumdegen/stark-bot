import { useState } from 'react';
import {
  Shield,
  Star,
  Search,
  Settings,
  ExternalLink,
  CheckCircle,
  XCircle,
  Copy,
  RefreshCw
} from 'lucide-react';
import Card, { CardContent } from '@/components/ui/Card';
import Button from '@/components/ui/Button';
import { useApi } from '@/hooks/useApi';

type Tab = 'identity' | 'reputation' | 'discovery' | 'config';

interface Eip8004Config {
  chain_id: number;
  chain_name: string;
  identity_registry: string;
  reputation_registry: string;
  validation_registry: string | null;
  identity_deployed: boolean;
  reputation_deployed: boolean;
  explorer_url: string;
}

interface AgentIdentity {
  agent_id: number;
  agent_registry: string;
  chain_id: number;
  registration_uri: string | null;
  wallet_address: string;
  name: string | null;
  is_active: boolean;
  created_at: string;
}

interface IdentityResponse {
  success: boolean;
  registered: boolean;
  identity?: AgentIdentity;
  config?: {
    chain_id: number;
    identity_registry: string;
    deployed: boolean;
  };
}

interface DiscoveredAgent {
  agent_id: number;
  name: string;
  description: string;
  owner: string;
  is_active: boolean;
  has_x402: boolean;
  reputation_count?: number;
  average_score?: number;
}

interface DiscoveryResponse {
  success: boolean;
  agents?: DiscoveredAgent[];
  total?: number;
  error?: string;
}

interface ConfigResponse {
  success: boolean;
  config?: Eip8004Config;
}

export default function EIP8004() {
  const [activeTab, setActiveTab] = useState<Tab>('identity');
  const [searchQuery, setSearchQuery] = useState('');
  const [copied, setCopied] = useState<string | null>(null);

  const { data: configData, isLoading: configLoading } = useApi<ConfigResponse>('/eip8004/config');
  const { data: identityData, isLoading: identityLoading, refetch: refetchIdentity } = useApi<IdentityResponse>('/eip8004/identity');
  const { data: agentsData, isLoading: agentsLoading, refetch: refetchAgents } = useApi<DiscoveryResponse>('/eip8004/agents');

  const config = configData?.config;
  const identity = identityData?.identity;
  const isRegistered = identityData?.registered ?? false;
  const agents = agentsData?.agents ?? [];

  const copyToClipboard = (text: string, label: string) => {
    navigator.clipboard.writeText(text);
    setCopied(label);
    setTimeout(() => setCopied(null), 2000);
  };

  const shortenAddress = (addr: string) => {
    if (addr.length <= 12) return addr;
    return `${addr.slice(0, 6)}...${addr.slice(-4)}`;
  };

  const getTrustBadge = (score: number | undefined, count: number | undefined) => {
    if (!count || count < 5) return { label: 'Unverified', color: 'bg-slate-500/20 text-slate-400' };
    if (!score) return { label: 'Unknown', color: 'bg-slate-500/20 text-slate-400' };
    if (score >= 75 && count >= 10) return { label: 'High Trust', color: 'bg-green-500/20 text-green-400' };
    if (score >= 50 && count >= 5) return { label: 'Medium Trust', color: 'bg-blue-500/20 text-blue-400' };
    if (score < 0) return { label: 'Negative', color: 'bg-red-500/20 text-red-400' };
    return { label: 'Low Trust', color: 'bg-amber-500/20 text-amber-400' };
  };

  const tabs = [
    { id: 'identity' as Tab, label: 'Identity', icon: Shield },
    { id: 'reputation' as Tab, label: 'Reputation', icon: Star },
    { id: 'discovery' as Tab, label: 'Discovery', icon: Search },
    { id: 'config' as Tab, label: 'Config', icon: Settings },
  ];

  return (
    <div className="p-8">
      <div className="mb-8">
        <h1 className="text-2xl font-bold text-white mb-2">EIP-8004: Trustless Agents</h1>
        <p className="text-slate-400">On-chain agent identity, reputation, and discovery</p>
      </div>

      {/* Tab Navigation */}
      <div className="flex gap-2 mb-6 border-b border-slate-700 pb-4">
        {tabs.map(tab => (
          <button
            key={tab.id}
            onClick={() => setActiveTab(tab.id)}
            className={`flex items-center gap-2 px-4 py-2 rounded-lg transition-colors ${
              activeTab === tab.id
                ? 'bg-stark-500 text-white'
                : 'text-slate-400 hover:text-white hover:bg-slate-700/50'
            }`}
          >
            <tab.icon className="w-4 h-4" />
            {tab.label}
          </button>
        ))}
      </div>

      {/* Identity Tab */}
      {activeTab === 'identity' && (
        <div className="space-y-6">
          <Card>
            <CardContent>
              <div className="flex items-center justify-between mb-6">
                <h2 className="text-lg font-semibold text-white">Your Agent Identity</h2>
                <Button variant="secondary" size="sm" onClick={() => refetchIdentity()}>
                  <RefreshCw className="w-4 h-4 mr-2" />
                  Refresh
                </Button>
              </div>

              {identityLoading ? (
                <div className="text-center py-8 text-slate-400">Loading identity...</div>
              ) : isRegistered && identity ? (
                <div className="space-y-4">
                  <div className="flex items-center gap-4 p-4 bg-green-500/10 border border-green-500/30 rounded-lg">
                    <CheckCircle className="w-6 h-6 text-green-400" />
                    <div>
                      <p className="text-green-400 font-medium">Registered Agent</p>
                      <p className="text-slate-400 text-sm">Agent ID: {identity.agent_id}</p>
                    </div>
                  </div>

                  <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
                    <div className="p-4 bg-slate-700/50 rounded-lg">
                      <p className="text-slate-400 text-sm mb-1">Name</p>
                      <p className="text-white font-medium">{identity.name ?? 'Unnamed'}</p>
                    </div>
                    <div className="p-4 bg-slate-700/50 rounded-lg">
                      <p className="text-slate-400 text-sm mb-1">Chain</p>
                      <p className="text-white font-medium">Chain ID: {identity.chain_id}</p>
                    </div>
                    <div className="p-4 bg-slate-700/50 rounded-lg">
                      <p className="text-slate-400 text-sm mb-1">Wallet Address</p>
                      <div className="flex items-center gap-2">
                        <p className="text-white font-mono text-sm">{shortenAddress(identity.wallet_address)}</p>
                        <button
                          onClick={() => copyToClipboard(identity.wallet_address, 'wallet')}
                          className="text-slate-400 hover:text-white"
                        >
                          <Copy className="w-4 h-4" />
                        </button>
                        {copied === 'wallet' && <span className="text-green-400 text-xs">Copied!</span>}
                      </div>
                    </div>
                    <div className="p-4 bg-slate-700/50 rounded-lg">
                      <p className="text-slate-400 text-sm mb-1">Status</p>
                      <p className={`font-medium ${identity.is_active ? 'text-green-400' : 'text-red-400'}`}>
                        {identity.is_active ? 'Active' : 'Inactive'}
                      </p>
                    </div>
                  </div>

                  {identity.registration_uri && (
                    <div className="p-4 bg-slate-700/50 rounded-lg">
                      <p className="text-slate-400 text-sm mb-1">Registration URI</p>
                      <a
                        href={identity.registration_uri}
                        target="_blank"
                        rel="noopener noreferrer"
                        className="text-stark-400 hover:text-stark-300 flex items-center gap-1"
                      >
                        {identity.registration_uri}
                        <ExternalLink className="w-4 h-4" />
                      </a>
                    </div>
                  )}
                </div>
              ) : (
                <div className="space-y-4">
                  <div className="flex items-center gap-4 p-4 bg-amber-500/10 border border-amber-500/30 rounded-lg">
                    <XCircle className="w-6 h-6 text-amber-400" />
                    <div>
                      <p className="text-amber-400 font-medium">Not Registered</p>
                      <p className="text-slate-400 text-sm">
                        Register your agent to enable discovery and reputation tracking
                      </p>
                    </div>
                  </div>

                  <div className="p-6 border border-dashed border-slate-600 rounded-lg text-center">
                    <Shield className="w-12 h-12 text-slate-500 mx-auto mb-4" />
                    <h3 className="text-white font-medium mb-2">Get Your Agent Identity</h3>
                    <p className="text-slate-400 text-sm mb-4">
                      Mint an ERC-721 identity token to make your agent discoverable on-chain.
                    </p>
                    <p className="text-slate-500 text-xs">
                      Use the /eip8004-register skill in Agent Chat to register
                    </p>
                  </div>
                </div>
              )}
            </CardContent>
          </Card>
        </div>
      )}

      {/* Reputation Tab */}
      {activeTab === 'reputation' && (
        <div className="space-y-6">
          <div className="grid grid-cols-1 md:grid-cols-3 gap-6">
            <Card>
              <CardContent>
                <div className="flex items-center gap-4">
                  <div className="p-3 rounded-lg bg-green-500/20">
                    <Star className="w-6 h-6 text-green-400" />
                  </div>
                  <div>
                    <p className="text-2xl font-bold text-white">0</p>
                    <p className="text-sm text-slate-400">Feedback Given</p>
                  </div>
                </div>
              </CardContent>
            </Card>

            <Card>
              <CardContent>
                <div className="flex items-center gap-4">
                  <div className="p-3 rounded-lg bg-blue-500/20">
                    <Star className="w-6 h-6 text-blue-400" />
                  </div>
                  <div>
                    <p className="text-2xl font-bold text-white">0</p>
                    <p className="text-sm text-slate-400">Feedback Received</p>
                  </div>
                </div>
              </CardContent>
            </Card>

            <Card>
              <CardContent>
                <div className="flex items-center gap-4">
                  <div className="p-3 rounded-lg bg-purple-500/20">
                    <Star className="w-6 h-6 text-purple-400" />
                  </div>
                  <div>
                    <p className="text-2xl font-bold text-white">N/A</p>
                    <p className="text-sm text-slate-400">Average Score</p>
                  </div>
                </div>
              </CardContent>
            </Card>
          </div>

          <Card>
            <CardContent>
              <h2 className="text-lg font-semibold text-white mb-4">How Reputation Works</h2>
              <div className="space-y-3 text-slate-400">
                <p>
                  EIP-8004 enables on-chain reputation tracking with proof-of-payment verification.
                </p>
                <ul className="list-disc list-inside space-y-2">
                  <li>
                    <span className="text-white">Submit feedback</span> after using x402-paid services
                  </li>
                  <li>
                    <span className="text-white">Include proof-of-payment</span> (tx hash) to verify interactions
                  </li>
                  <li>
                    <span className="text-white">Build trust</span> through consistent positive feedback
                  </li>
                  <li>
                    <span className="text-white">Discover trusted agents</span> based on reputation scores
                  </li>
                </ul>
              </div>

              <div className="mt-6 p-4 bg-slate-700/50 rounded-lg">
                <h3 className="text-white font-medium mb-2">Trust Levels</h3>
                <div className="grid grid-cols-2 md:grid-cols-4 gap-2">
                  <span className="px-2 py-1 bg-green-500/20 text-green-400 rounded text-xs text-center">
                    High (75+, 10+ reviews)
                  </span>
                  <span className="px-2 py-1 bg-blue-500/20 text-blue-400 rounded text-xs text-center">
                    Medium (50+, 5+ reviews)
                  </span>
                  <span className="px-2 py-1 bg-amber-500/20 text-amber-400 rounded text-xs text-center">
                    Low (&lt;50 or &lt;5 reviews)
                  </span>
                  <span className="px-2 py-1 bg-red-500/20 text-red-400 rounded text-xs text-center">
                    Negative (&lt;0 score)
                  </span>
                </div>
              </div>
            </CardContent>
          </Card>
        </div>
      )}

      {/* Discovery Tab */}
      {activeTab === 'discovery' && (
        <div className="space-y-6">
          <Card>
            <CardContent>
              <div className="flex items-center justify-between mb-4">
                <h2 className="text-lg font-semibold text-white">Discover Agents</h2>
                <Button variant="secondary" size="sm" onClick={() => refetchAgents()}>
                  <RefreshCw className="w-4 h-4 mr-2" />
                  Refresh
                </Button>
              </div>

              <div className="mb-4">
                <input
                  type="text"
                  placeholder="Search agents by name or service..."
                  value={searchQuery}
                  onChange={(e) => setSearchQuery(e.target.value)}
                  className="w-full px-4 py-2 bg-slate-700/50 border border-slate-600 rounded-lg text-white placeholder-slate-400 focus:outline-none focus:border-stark-400"
                />
              </div>

              {agentsLoading ? (
                <div className="text-center py-8 text-slate-400">Loading agents...</div>
              ) : agents.length === 0 ? (
                <div className="text-center py-8 text-slate-400">
                  <Search className="w-12 h-12 mx-auto mb-4 text-slate-500" />
                  <p>No agents found.</p>
                  <p className="text-sm">
                    {config?.identity_deployed
                      ? 'Agents will appear here once registered on-chain.'
                      : 'Identity Registry not yet deployed.'}
                  </p>
                </div>
              ) : (
                <div className="space-y-3">
                  {agents
                    .filter(a =>
                      !searchQuery ||
                      a.name.toLowerCase().includes(searchQuery.toLowerCase()) ||
                      a.description.toLowerCase().includes(searchQuery.toLowerCase())
                    )
                    .map(agent => {
                      const trust = getTrustBadge(agent.average_score, agent.reputation_count);
                      return (
                        <div
                          key={agent.agent_id}
                          className="p-4 bg-slate-700/50 rounded-lg hover:bg-slate-700 transition-colors"
                        >
                          <div className="flex items-start justify-between">
                            <div>
                              <div className="flex items-center gap-2">
                                <h3 className="text-white font-medium">{agent.name}</h3>
                                {agent.has_x402 && (
                                  <span className="px-2 py-0.5 bg-stark-500/20 text-stark-400 rounded text-xs">
                                    x402
                                  </span>
                                )}
                                <span className={`px-2 py-0.5 rounded text-xs ${trust.color}`}>
                                  {trust.label}
                                </span>
                              </div>
                              <p className="text-slate-400 text-sm mt-1">{agent.description}</p>
                              <p className="text-slate-500 text-xs mt-2">
                                Agent #{agent.agent_id} - Owner: {shortenAddress(agent.owner)}
                              </p>
                            </div>
                            <div className="text-right">
                              <p className="text-slate-400 text-sm">
                                {agent.reputation_count ?? 0} reviews
                              </p>
                              {agent.average_score !== undefined && (
                                <p className="text-white font-mono">
                                  {agent.average_score.toFixed(1)}
                                </p>
                              )}
                            </div>
                          </div>
                        </div>
                      );
                    })}
                </div>
              )}
            </CardContent>
          </Card>
        </div>
      )}

      {/* Config Tab */}
      {activeTab === 'config' && (
        <div className="space-y-6">
          <Card>
            <CardContent>
              <h2 className="text-lg font-semibold text-white mb-4">EIP-8004 Configuration</h2>

              {configLoading ? (
                <div className="text-center py-8 text-slate-400">Loading configuration...</div>
              ) : config ? (
                <div className="space-y-4">
                  <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
                    <div className="p-4 bg-slate-700/50 rounded-lg">
                      <p className="text-slate-400 text-sm mb-1">Chain</p>
                      <p className="text-white font-medium">{config.chain_name} ({config.chain_id})</p>
                    </div>
                    <div className="p-4 bg-slate-700/50 rounded-lg">
                      <p className="text-slate-400 text-sm mb-1">Explorer</p>
                      <a
                        href={config.explorer_url}
                        target="_blank"
                        rel="noopener noreferrer"
                        className="text-stark-400 hover:text-stark-300 flex items-center gap-1"
                      >
                        {config.explorer_url}
                        <ExternalLink className="w-4 h-4" />
                      </a>
                    </div>
                  </div>

                  <div className="space-y-3">
                    <h3 className="text-white font-medium">Registry Contracts</h3>

                    <div className="p-4 bg-slate-700/50 rounded-lg">
                      <div className="flex items-center justify-between mb-2">
                        <p className="text-slate-400 text-sm">Identity Registry</p>
                        <span className={`px-2 py-0.5 rounded text-xs ${
                          config.identity_deployed
                            ? 'bg-green-500/20 text-green-400'
                            : 'bg-amber-500/20 text-amber-400'
                        }`}>
                          {config.identity_deployed ? 'Deployed' : 'Not Deployed'}
                        </span>
                      </div>
                      <div className="flex items-center gap-2">
                        <p className="text-white font-mono text-sm">{config.identity_registry}</p>
                        <button
                          onClick={() => copyToClipboard(config.identity_registry, 'identity')}
                          className="text-slate-400 hover:text-white"
                        >
                          <Copy className="w-4 h-4" />
                        </button>
                        {copied === 'identity' && <span className="text-green-400 text-xs">Copied!</span>}
                      </div>
                    </div>

                    <div className="p-4 bg-slate-700/50 rounded-lg">
                      <div className="flex items-center justify-between mb-2">
                        <p className="text-slate-400 text-sm">Reputation Registry</p>
                        <span className={`px-2 py-0.5 rounded text-xs ${
                          config.reputation_deployed
                            ? 'bg-green-500/20 text-green-400'
                            : 'bg-amber-500/20 text-amber-400'
                        }`}>
                          {config.reputation_deployed ? 'Deployed' : 'Not Deployed'}
                        </span>
                      </div>
                      <div className="flex items-center gap-2">
                        <p className="text-white font-mono text-sm">{config.reputation_registry}</p>
                        <button
                          onClick={() => copyToClipboard(config.reputation_registry, 'reputation')}
                          className="text-slate-400 hover:text-white"
                        >
                          <Copy className="w-4 h-4" />
                        </button>
                        {copied === 'reputation' && <span className="text-green-400 text-xs">Copied!</span>}
                      </div>
                    </div>

                    {config.validation_registry && (
                      <div className="p-4 bg-slate-700/50 rounded-lg">
                        <p className="text-slate-400 text-sm mb-2">Validation Registry</p>
                        <div className="flex items-center gap-2">
                          <p className="text-white font-mono text-sm">{config.validation_registry}</p>
                          <button
                            onClick={() => copyToClipboard(config.validation_registry!, 'validation')}
                            className="text-slate-400 hover:text-white"
                          >
                            <Copy className="w-4 h-4" />
                          </button>
                          {copied === 'validation' && <span className="text-green-400 text-xs">Copied!</span>}
                        </div>
                      </div>
                    )}
                  </div>
                </div>
              ) : (
                <div className="text-center py-8 text-slate-400">
                  Failed to load configuration.
                </div>
              )}
            </CardContent>
          </Card>
        </div>
      )}
    </div>
  );
}
