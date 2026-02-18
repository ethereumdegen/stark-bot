import { useState, useCallback, useEffect, useMemo } from 'react';
import { MessageSquare, Calendar, Wrench, Zap, Sparkles, Wallet, Copy, Check, Newspaper } from 'lucide-react';
import { useNavigate } from 'react-router-dom';
import Card, { CardContent } from '@/components/ui/Card';
import { useApi } from '@/hooks/useApi';
import { useWallet } from '@/hooks/useWallet';
import { getCortexBulletin, SkillInfo } from '@/lib/api';
import type { CortexBulletin } from '@/types';

interface ServiceCapability {
  id: string;
  label: string;
  icon: string;
  matchNames: string[];
  matchTags: string[];
}

const SERVICE_CAPABILITIES: ServiceCapability[] = [
  { id: 'github', label: 'GitHub', icon: '/icons/github.svg', matchNames: ['github'], matchTags: ['git', 'github'] },
  { id: 'railway', label: 'Railway', icon: '/icons/railway.svg', matchNames: ['railway'], matchTags: ['railway'] },
  { id: 'digitalocean', label: 'DigitalOcean', icon: '/icons/digitalocean.svg', matchNames: ['digitalocean', 'digital_ocean'], matchTags: ['digitalocean'] },
  { id: 'polymarket', label: 'Polymarket', icon: '/icons/polymarket.svg', matchNames: ['polymarket'], matchTags: ['polymarket', 'prediction'] },
  { id: 'cloudflare', label: 'Cloudflare', icon: '/icons/cloudflare.svg', matchNames: ['cloudflare'], matchTags: ['cloudflare', 'dns'] },
  { id: 'x402', label: 'x402 Payments', icon: '/icons/x402.svg', matchNames: ['x402', 'x402_post'], matchTags: ['x402', 'payments'] },
  { id: 'erc20', label: 'ERC-20 Swap', icon: '/icons/erc20.svg', matchNames: ['erc20', 'erc_20', 'swap', 'token_swap'], matchTags: ['swap', 'erc20', 'erc-20', 'token'] },
  { id: 'twitter', label: 'X / Twitter', icon: '/icons/twitter.svg', matchNames: ['twitter', 'x_post', 'tweet'], matchTags: ['twitter', 'x', 'social-media'] },
  { id: 'discord', label: 'Discord', icon: '/icons/discord.svg', matchNames: ['discord'], matchTags: ['discord'] },
  { id: 'telegram', label: 'Telegram', icon: '/icons/telegram.svg', matchNames: ['telegram'], matchTags: ['telegram'] },
  { id: 'uniswap', label: 'Uniswap', icon: '/icons/uniswap.svg', matchNames: ['uniswap'], matchTags: ['uniswap', 'dex'] },
  { id: 'wallet', label: 'Wallet', icon: '/icons/wallet.svg', matchNames: ['wallet', 'transfer', 'send_eth', 'send_usdc'], matchTags: ['wallet', 'transfer', 'crypto'] },
];

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
  const [bulletin, setBulletin] = useState<CortexBulletin | null>(null);

  useEffect(() => {
    getCortexBulletin()
      .then(setBulletin)
      .catch(() => setBulletin(null));
  }, []);

  const { data: sessions } = useApi<Array<unknown>>('/sessions');
  const { data: tools } = useApi<Array<unknown>>('/tools');
  const { data: skills } = useApi<SkillInfo[]>('/skills');
  const { data: versionData } = useApi<{ version: string }>('/version');
  const appVersion = versionData?.version || '...';

  const activeCapabilities = useMemo(() => {
    if (!skills || skills.length === 0) return [];
    const enabledSkills = skills.filter((s) => s.enabled);
    return SERVICE_CAPABILITIES.filter((cap) =>
      enabledSkills.some((skill) => {
        const name = skill.name.toLowerCase();
        const tags = (skill.tags || []).map((t) => t.toLowerCase());
        return (
          cap.matchNames.some((m) => name.includes(m)) ||
          cap.matchTags.some((m) => tags.includes(m))
        );
      })
    );
  }, [skills]);

  const stats = [
    {
      label: 'Active Sessions',
      value: sessions?.length ?? 0,
      icon: Calendar,
      color: 'text-blue-400',
      bgColor: 'bg-blue-500/20',
    },
    {
      label: 'Tools Available',
      value: tools?.length ?? 0,
      icon: Wrench,
      color: 'text-green-400',
      bgColor: 'bg-green-500/20',
    },
    {
      label: 'Skills Loaded',
      value: skills?.length ?? 0,
      icon: Zap,
      color: 'text-amber-400',
      bgColor: 'bg-amber-500/20',
    },
    {
      label: 'Messages Today',
      value: 0,
      icon: MessageSquare,
      color: 'text-purple-400',
      bgColor: 'bg-purple-500/20',
    },
  ];

  return (
    <div className="p-8">
      <div className="mb-8 flex items-start justify-between">
        <div>
          <h1 className="text-2xl font-bold text-white mb-2">Dashboard</h1>
          <p className="text-slate-400">Overview of your StarkBot instance</p>
        </div>

        {isConnected && address && (
          <div className="flex flex-col items-end gap-1">
            <div className="flex items-center gap-2 bg-slate-700/50 px-3 py-1.5 rounded-lg">
              <Wallet className="w-4 h-4 text-slate-400" />
              <span className="text-sm font-mono text-slate-300">
                {address}
              </span>
              <button
                onClick={copyAddress}
                className="text-slate-400 hover:text-slate-200 transition-colors"
                title="Copy address"
              >
                {copied ? (
                  <Check className="w-4 h-4 text-green-400" />
                ) : (
                  <Copy className="w-4 h-4" />
                )}
              </button>
              {walletMode === 'flash' && (
                <span className="text-xs px-1.5 py-0.5 bg-purple-500/20 text-purple-400 rounded font-medium ml-1">
                  Flash
                </span>
              )}
            </div>
            {walletMode === 'flash' && (
              <span className="text-xs text-slate-500">
                Encrypted Privy wallet — private key is not stored in this instance
              </span>
            )}
          </div>
        )}
      </div>

      <div className="mb-8 p-6 bg-slate-800/50 rounded-lg border border-slate-700">
        <h2 className="text-xl font-semibold text-white mb-4">Getting Started</h2>
        <ul className="space-y-2 text-slate-300">
          <li className="flex items-start gap-2">
            <span className="text-stark-400 mt-1">•</span>
            <span>USDC on Base is needed to use the default AI model relay</span>
          </li>
          <li className="flex items-start gap-2">
            <span className="text-stark-400 mt-1">•</span>
            <span>A custom AI model endpoint can be configured in Agent Settings as an alternative</span>
          </li>
          <li className="flex items-start gap-2">
            <span className="text-stark-400 mt-1">•</span>
            <span>StarkBot is stateless so the encrypted backup store must be used to maintain state such as API keys and memories</span>
          </li>
          <li className="flex items-start gap-2">
            <span className="text-stark-400 mt-1">•</span>
            <span>To setup automation, configure a heartbeat and the mindmap or set up scheduled cron jobs</span>
          </li>
        </ul>
      </div>

      {activeCapabilities.length > 0 && (
        <div className="mb-8 p-6 bg-slate-800/50 rounded-lg border border-slate-700">
          <h2 className="text-lg font-semibold text-white mb-4">Active Capabilities</h2>
          <div className="grid grid-cols-2 sm:grid-cols-3 md:grid-cols-4 gap-4">
            {activeCapabilities.map((cap) => (
              <div
                key={cap.id}
                className="flex flex-col items-center gap-2 p-4 rounded-lg bg-slate-700/30 hover:bg-slate-700/50 transition-colors"
              >
                <div className="w-10 h-10 flex items-center justify-center">
                  <img
                    src={cap.icon}
                    alt={cap.label}
                    className="w-8 h-8 opacity-80 brightness-0 invert"
                  />
                </div>
                <span className="text-sm text-slate-300 text-center">{cap.label}</span>
              </div>
            ))}
          </div>
        </div>
      )}

      <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-4 gap-6">
        {stats.map((stat) => (
          <Card key={stat.label}>
            <CardContent>
              <div className="flex items-center gap-4">
                <div className={`p-3 rounded-lg ${stat.bgColor}`}>
                  <stat.icon className={`w-6 h-6 ${stat.color}`} />
                </div>
                <div>
                  <p className="text-2xl font-bold text-white">{stat.value}</p>
                  <p className="text-sm text-slate-400">{stat.label}</p>
                </div>
              </div>
            </CardContent>
          </Card>
        ))}
      </div>

      {bulletin && (
        <div className="mt-6">
          <Card>
            <CardContent>
              <div className="flex items-start gap-3">
                <div className="p-2 rounded-lg bg-indigo-500/20">
                  <Newspaper className="w-5 h-5 text-indigo-400" />
                </div>
                <div className="flex-1 min-w-0">
                  <div className="flex items-center justify-between mb-2">
                    <h2 className="text-lg font-semibold text-white">Cortex Bulletin</h2>
                    <span className="text-xs text-slate-500">
                      {new Date(bulletin.generated_at).toLocaleDateString()}
                    </span>
                  </div>
                  <p className="text-slate-300 text-sm whitespace-pre-wrap">{bulletin.content}</p>
                  {bulletin.topics?.length > 0 && (
                    <div className="flex flex-wrap gap-1.5 mt-3">
                      {bulletin.topics.map((topic) => (
                        <span
                          key={topic}
                          className="px-2 py-0.5 text-xs rounded-full bg-indigo-500/20 text-indigo-300"
                        >
                          {topic}
                        </span>
                      ))}
                    </div>
                  )}
                </div>
              </div>
            </CardContent>
          </Card>
        </div>
      )}

      <div className="mt-8 grid grid-cols-1 lg:grid-cols-2 gap-6">
        <Card>
          <CardContent>
            <h2 className="text-lg font-semibold text-white mb-4">Quick Actions</h2>
            <div className="space-y-3">
              <a
                href="/agent-chat"
                className="flex items-center gap-3 p-3 rounded-lg bg-slate-700/50 hover:bg-slate-700 transition-colors text-slate-300 hover:text-white"
              >
                <MessageSquare className="w-5 h-5 text-stark-400" />
                <span>Start Agent Chat</span>
              </a>
              <a
                href="/tools"
                className="flex items-center gap-3 p-3 rounded-lg bg-slate-700/50 hover:bg-slate-700 transition-colors text-slate-300 hover:text-white"
              >
                <Wrench className="w-5 h-5 text-stark-400" />
                <span>Configure Tools</span>
              </a>
              <a
                href="/skills"
                className="flex items-center gap-3 p-3 rounded-lg bg-slate-700/50 hover:bg-slate-700 transition-colors text-slate-300 hover:text-white"
              >
                <Zap className="w-5 h-5 text-stark-400" />
                <span>Manage Skills</span>
              </a>
            </div>
          </CardContent>
        </Card>

        <Card>
          <CardContent>
            <h2 className="text-lg font-semibold text-white mb-4">System Status</h2>
            <div className="space-y-3">
              <div className="flex items-center justify-between p-3 rounded-lg bg-slate-700/50">
                <span className="text-slate-300">Backend</span>
                <span className="flex items-center gap-2 text-green-400">
                  <span className="w-2 h-2 bg-green-400 rounded-full" />
                  Online
                </span>
              </div>
              <div className="flex items-center justify-between p-3 rounded-lg bg-slate-700/50">
                <span className="text-slate-300">Gateway</span>
                <span className="flex items-center gap-2 text-green-400">
                  <span className="w-2 h-2 bg-green-400 rounded-full" />
                  Connected
                </span>
              </div>
              <div className="flex items-center justify-between p-3 rounded-lg bg-slate-700/50">
                <span className="text-slate-300">Database</span>
                <span className="flex items-center gap-2 text-green-400">
                  <span className="w-2 h-2 bg-green-400 rounded-full" />
                  Healthy
                </span>
              </div>
            </div>
          </CardContent>
        </Card>
      </div>

      <div className="mt-8 flex justify-center">
        <button
          onClick={() => versionData && navigate(`/agent-chat?message=${encodeURIComponent(`What's new in version ${appVersion}?`)}`)}
          disabled={!versionData}
          className="flex items-center gap-2 px-6 py-3 rounded-lg bg-stark-500/20 border border-stark-500/30 text-stark-400 hover:bg-stark-500/30 hover:text-stark-300 transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
        >
          <Sparkles className="w-5 h-5" />
          <span>What's new in version {appVersion}?</span>
        </button>
      </div>
    </div>
  );
}
