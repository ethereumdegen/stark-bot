import { useState, useCallback, useEffect } from 'react';
import { Bot, Wallet, Copy, Check, AlertTriangle, Key, Send, Activity } from 'lucide-react';
import { useNavigate } from 'react-router-dom';
import Card, { CardContent } from '@/components/ui/Card';
import { useApi } from '@/hooks/useApi';
import { useWallet } from '@/hooks/useWallet';



interface StarflaskAgent {
  id: number;
  capability: string;
  agent_id: string;
  name: string;
  description: string;
  status: string;
}

interface ConfigStatus {
  starflask_configured: boolean;
  starflask_api_key_set: boolean;
  starflask_agents_provisioned: number;
  wallet_configured: boolean;
  wallet_address?: string;
  wallet_mode?: string;
}

export default function Dashboard() {
  const navigate = useNavigate();
  const { address, isConnected, walletMode } = useWallet();
  const [copied, setCopied] = useState(false);

  const copyAddress = useCallback(() => {
    if (address) {
      navigator.clipboard.writeText(address);
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    }
  }, [address]);

  const { data: agents } = useApi<StarflaskAgent[]>('/starflask/agents');
  const { data: versionData } = useApi<{ version: string }>('/version');
  const [config, setConfig] = useState<ConfigStatus | null>(null);

  useEffect(() => {
    fetch('/api/health/config').then(r => r.json()).then(setConfig).catch(() => {});
  }, []);

  const starflaskReady = config?.starflask_configured;
  const apiKeySet = config?.starflask_api_key_set;
  const agentCount = agents?.length ?? 0;

  const capabilityIcon: Record<string, string> = {
    crypto: '/icons/wallet.svg',
    image_gen: '/icons/image_generation.svg',
    video_gen: '/icons/video_generation.svg',
    discord_moderator: '/icons/erc20.svg',
    telegram_moderator: '/icons/erc20.svg',
    general: '/icons/erc20.svg',
  };

  const capabilityColor: Record<string, string> = {
    crypto: 'text-amber-400 bg-amber-500/20',
    image_gen: 'text-pink-400 bg-pink-500/20',
    video_gen: 'text-purple-400 bg-purple-500/20',
    discord_moderator: 'text-indigo-400 bg-indigo-500/20',
    telegram_moderator: 'text-sky-400 bg-sky-500/20',
    general: 'text-green-400 bg-green-500/20',
  };

  return (
    <div className="p-8">
      {/* Header */}
      <div className="mb-8 flex flex-wrap items-start justify-between gap-4">
        <div>
          <h1 className="text-2xl font-bold text-white mb-1">Starkbot Command Center</h1>
          <p className="text-slate-400 text-sm">v{versionData?.version || '...'}</p>
        </div>
        {isConnected && address && (
          <div className="flex items-center gap-2 bg-slate-700/50 px-3 py-1.5 rounded-lg">
            <Wallet className="w-4 h-4 text-slate-400" />
            <span className="text-sm font-mono text-slate-300 truncate max-w-[200px]">{address}</span>
            <button onClick={copyAddress} className="text-slate-400 hover:text-slate-200 transition-colors">
              {copied ? <Check className="w-4 h-4 text-green-400" /> : <Copy className="w-4 h-4" />}
            </button>
            {walletMode === 'flash' && (
              <span className="text-xs px-1.5 py-0.5 bg-purple-500/20 text-purple-400 rounded font-medium">Flash</span>
            )}
          </div>
        )}
      </div>

      {/* Setup Banner */}
      {!apiKeySet && (
        <div className="mb-6 p-6 bg-amber-500/10 rounded-lg border border-amber-500/30">
          <div className="flex items-start gap-3">
            <AlertTriangle className="w-6 h-6 text-amber-400 mt-0.5 flex-shrink-0" />
            <div>
              <h2 className="text-lg font-semibold text-amber-300 mb-1">Starflask API Key Required</h2>
              <p className="text-slate-300 text-sm mb-3">
                Add your Starflask API key to connect to your agents. This is the first step to get Starkbot running.
              </p>
              <button
                onClick={() => navigate('/api-keys')}
                className="inline-flex items-center gap-2 px-4 py-2 rounded-lg bg-amber-500/20 border border-amber-500/40 text-amber-300 hover:bg-amber-500/30 transition-colors text-sm font-medium"
              >
                <Key className="w-4 h-4" />
                Add API Key
              </button>
            </div>
          </div>
        </div>
      )}

      {apiKeySet && !starflaskReady && (
        <div className="mb-6 p-4 bg-blue-500/10 rounded-lg border border-blue-500/30">
          <p className="text-blue-300 text-sm">Starflask API key saved. Restart Starkbot to initialize the connection.</p>
        </div>
      )}

      {/* Stats */}
      <div className="grid grid-cols-1 md:grid-cols-3 gap-4 mb-8">
        <Card>
          <CardContent>
            <div className="flex items-center gap-4">
              <div className="p-3 rounded-lg bg-stark-500/20">
                <Bot className="w-6 h-6 text-stark-400" />
              </div>
              <div>
                <p className="text-2xl font-bold text-white">{agentCount}</p>
                <p className="text-sm text-slate-400">Agents Provisioned</p>
              </div>
            </div>
          </CardContent>
        </Card>
        <Card>
          <CardContent>
            <div className="flex items-center gap-4">
              <div className="p-3 rounded-lg bg-green-500/20">
                <Activity className="w-6 h-6 text-green-400" />
              </div>
              <div>
                <p className="text-2xl font-bold text-white">{starflaskReady ? 'Connected' : 'Offline'}</p>
                <p className="text-sm text-slate-400">Starflask Status</p>
              </div>
            </div>
          </CardContent>
        </Card>
        <Card>
          <CardContent>
            <div className="flex items-center gap-4">
              <div className="p-3 rounded-lg bg-blue-500/20">
                <Wallet className="w-6 h-6 text-blue-400" />
              </div>
              <div>
                <p className="text-2xl font-bold text-white">{config?.wallet_configured ? 'Active' : 'None'}</p>
                <p className="text-sm text-slate-400">Wallet</p>
              </div>
            </div>
          </CardContent>
        </Card>
      </div>

      {/* Agents Grid */}
      {agentCount > 0 && (
        <div className="mb-8">
          <div className="flex items-center justify-between mb-4">
            <h2 className="text-lg font-semibold text-white">Your Agents</h2>
            <button
              onClick={() => navigate('/starflask-agents')}
              className="text-sm text-stark-400 hover:text-stark-300 transition-colors"
            >
              View All
            </button>
          </div>
          <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-4">
            {(agents || []).map((agent) => (
              <button
                key={agent.capability}
                onClick={() => navigate('/starflask-agents')}
                className="text-left p-5 rounded-lg bg-slate-800/50 border border-slate-700 hover:bg-slate-700/50 hover:border-slate-600 transition-colors"
              >
                <div className="flex items-center gap-3 mb-3">
                  <div className={`p-2 rounded-lg ${capabilityColor[agent.capability]?.split(' ')[1] || 'bg-slate-500/20'}`}>
                    {capabilityIcon[agent.capability] ? (
                      <img src={capabilityIcon[agent.capability]} alt="" className="w-5 h-5 brightness-0 invert opacity-80" />
                    ) : (
                      <Bot className="w-5 h-5 text-slate-400" />
                    )}
                  </div>
                  <div>
                    <h3 className="text-white font-medium text-sm">{agent.name}</h3>
                    <span className={`text-xs ${capabilityColor[agent.capability]?.split(' ')[0] || 'text-slate-400'}`}>
                      {agent.capability}
                    </span>
                  </div>
                </div>
                <p className="text-slate-400 text-xs">{agent.description}</p>
              </button>
            ))}
          </div>
        </div>
      )}

      {/* Quick Actions */}
      <div className="grid grid-cols-1 lg:grid-cols-2 gap-6">
        <Card>
          <CardContent>
            <h2 className="text-lg font-semibold text-white mb-4">Quick Actions</h2>
            <div className="space-y-3">
              <button
                onClick={() => navigate('/command-center')}
                className="w-full flex items-center gap-3 p-3 rounded-lg bg-slate-700/50 hover:bg-slate-700 transition-colors text-slate-300 hover:text-white text-left"
              >
                <Send className="w-5 h-5 text-stark-400" />
                <span>Send Command</span>
              </button>
              <button
                onClick={() => navigate('/starflask-agents')}
                className="w-full flex items-center gap-3 p-3 rounded-lg bg-slate-700/50 hover:bg-slate-700 transition-colors text-slate-300 hover:text-white text-left"
              >
                <Bot className="w-5 h-5 text-stark-400" />
                <span>Manage Agents</span>
              </button>
              <button
                onClick={() => navigate('/api-keys')}
                className="w-full flex items-center gap-3 p-3 rounded-lg bg-slate-700/50 hover:bg-slate-700 transition-colors text-slate-300 hover:text-white text-left"
              >
                <Key className="w-5 h-5 text-stark-400" />
                <span>API Keys</span>
              </button>
            </div>
          </CardContent>
        </Card>

        <Card>
          <CardContent>
            <h2 className="text-lg font-semibold text-white mb-4">System Status</h2>
            <div className="space-y-3">
              <div className="flex items-center justify-between p-3 rounded-lg bg-slate-700/50">
                <span className="text-slate-300">Backend</span>
                <span className="flex items-center gap-2 text-green-400 text-sm">
                  <span className="w-2 h-2 bg-green-400 rounded-full" />
                  Online
                </span>
              </div>
              <div className="flex items-center justify-between p-3 rounded-lg bg-slate-700/50">
                <span className="text-slate-300">Starflask</span>
                <span className={`flex items-center gap-2 text-sm ${starflaskReady ? 'text-green-400' : 'text-slate-500'}`}>
                  <span className={`w-2 h-2 rounded-full ${starflaskReady ? 'bg-green-400' : 'bg-slate-500'}`} />
                  {starflaskReady ? 'Connected' : 'Not configured'}
                </span>
              </div>
              <div className="flex items-center justify-between p-3 rounded-lg bg-slate-700/50">
                <span className="text-slate-300">Wallet</span>
                <span className={`flex items-center gap-2 text-sm ${config?.wallet_configured ? 'text-green-400' : 'text-slate-500'}`}>
                  <span className={`w-2 h-2 rounded-full ${config?.wallet_configured ? 'bg-green-400' : 'bg-slate-500'}`} />
                  {config?.wallet_configured ? (config?.wallet_mode || 'Active') : 'Not configured'}
                </span>
              </div>
            </div>
          </CardContent>
        </Card>
      </div>
    </div>
  );
}
