import { useState, useEffect, FormEvent } from 'react';
import { Save, Bot, Server, Shield, Cloud, AlertTriangle, CheckCircle, Info, XCircle, Copy, Check, Wallet, Brain, Palette, Globe } from 'lucide-react';
import Card, { CardContent, CardHeader, CardTitle } from '@/components/ui/Card';
import Button from '@/components/ui/Button';
import Input from '@/components/ui/Input';
import OperatingModeCard from '@/components/OperatingModeCard';
import {
  getBotSettings,
  updateBotSettings,
  getRpcProviders,
  getAutoSyncStatus,
  getConfigStatus,
  BotSettings as BotSettingsType,
  RpcProvider,
  AutoSyncStatus,
} from '@/lib/api';

export default function BotSettings() {
  const [, setSettings] = useState<BotSettingsType | null>(null);
  const [botName, setBotName] = useState('StarkBot');
  const [botEmail, setBotEmail] = useState('starkbot@users.noreply.github.com');
  const [rpcProvider, setRpcProvider] = useState('defirelay');
  const [customRpcBase, setCustomRpcBase] = useState('');
  const [customRpcMainnet, setCustomRpcMainnet] = useState('');
  const [customRpcPolygon, setCustomRpcPolygon] = useState('');
  const [rpcProviders, setRpcProviders] = useState<RpcProvider[]>([]);
  const [rogueModeEnabled, setRogueModeEnabled] = useState(false);
  const [safeModeMaxQueries, setSafeModeMaxQueries] = useState(5);
  const [keystoreUrl, setKeystoreUrl] = useState('');
  const [chatSessionMemoryGeneration, setChatSessionMemoryGeneration] = useState(true);
  const [guestDashboardEnabled, setGuestDashboardEnabled] = useState(false);
  const [autoSyncStatus, setAutoSyncStatus] = useState<AutoSyncStatus | null>(null);
  const [walletAddress, setWalletAddress] = useState<string>('');
  const [walletMode, setWalletMode] = useState<string>('');
  const [walletCopied, setWalletCopied] = useState(false);
  const [proxyUrl, setProxyUrl] = useState('');
  const [themeAccent, setThemeAccent] = useState(() => localStorage.getItem('theme-accent') || '');
  const [isLoading, setIsLoading] = useState(true);
  const [isSaving, setIsSaving] = useState(false);
  const [message, setMessage] = useState<{ type: 'success' | 'error'; text: string } | null>(null);

  const handleThemeChange = (value: string) => {
    setThemeAccent(value);
    if (value) {
      localStorage.setItem('theme-accent', value);
      document.documentElement.dataset.theme = value;
    } else {
      localStorage.removeItem('theme-accent');
      delete document.documentElement.dataset.theme;
    }
  };

  useEffect(() => {
    loadSettings();
    loadRpcProviders();
    loadAutoSyncStatus();
    loadWalletConfig();
  }, []);

  const loadWalletConfig = async () => {
    try {
      const config = await getConfigStatus();
      setWalletAddress(config.wallet_address || '');
      setWalletMode(config.wallet_mode || '');
    } catch (err) {
      console.error('Failed to load wallet config:', err);
    }
  };

  const copyWalletAddress = () => {
    if (walletAddress) {
      navigator.clipboard.writeText(walletAddress);
      setWalletCopied(true);
      setTimeout(() => setWalletCopied(false), 2000);
    }
  };

  const truncateAddress = (addr: string) => {
    if (!addr || addr.length < 10) return addr;
    return `${addr.slice(0, 6)}...${addr.slice(-4)}`;
  };

  const loadAutoSyncStatus = async () => {
    try {
      const status = await getAutoSyncStatus();
      setAutoSyncStatus(status);
    } catch (err) {
      console.error('Failed to load auto-sync status:', err);
    }
  };

  const loadSettings = async () => {
    try {
      const data = await getBotSettings();
      setSettings(data);
      setBotName(data.bot_name);
      setBotEmail(data.bot_email);
      setRpcProvider(data.rpc_provider || 'defirelay');
      setRogueModeEnabled(data.rogue_mode_enabled || false);
      setSafeModeMaxQueries(data.safe_mode_max_queries_per_10min || 5);
      setKeystoreUrl(data.keystore_url || '');
      setChatSessionMemoryGeneration(data.chat_session_memory_generation ?? true);
      setGuestDashboardEnabled(data.guest_dashboard_enabled ?? false);
      setProxyUrl(data.proxy_url || '');
      // Sync theme from backend (backend is source of truth, update localStorage to match)
      const serverTheme = data.theme_accent || '';
      setThemeAccent(serverTheme);
      if (serverTheme) {
        localStorage.setItem('theme-accent', serverTheme);
        document.documentElement.dataset.theme = serverTheme;
      } else {
        localStorage.removeItem('theme-accent');
        delete document.documentElement.dataset.theme;
      }
      if (data.custom_rpc_endpoints) {
        setCustomRpcBase(data.custom_rpc_endpoints.base || '');
        setCustomRpcMainnet(data.custom_rpc_endpoints.mainnet || '');
        setCustomRpcPolygon(data.custom_rpc_endpoints.polygon || '');
      }
    } catch (err) {
      setMessage({ type: 'error', text: 'Failed to load settings' });
    } finally {
      setIsLoading(false);
    }
  };

  const loadRpcProviders = async () => {
    try {
      const providers = await getRpcProviders();
      setRpcProviders(providers);
    } catch (err) {
      console.error('Failed to load RPC providers:', err);
    }
  };

  const handleSubmit = async (e: FormEvent) => {
    e.preventDefault();
    setIsSaving(true);
    setMessage(null);

    try {
      const customEndpoints = rpcProvider === 'custom' ? {
        base: customRpcBase,
        mainnet: customRpcMainnet,
        polygon: customRpcPolygon,
      } : undefined;

      const updated = await updateBotSettings({
        bot_name: botName,
        bot_email: botEmail,
        rpc_provider: rpcProvider,
        custom_rpc_endpoints: customEndpoints,
        safe_mode_max_queries_per_10min: safeModeMaxQueries,
        keystore_url: keystoreUrl,
        chat_session_memory_generation: chatSessionMemoryGeneration,
        guest_dashboard_enabled: guestDashboardEnabled,
        theme_accent: themeAccent || '',
        proxy_url: proxyUrl,
      });
      setSettings(updated);
      setMessage({ type: 'success', text: 'Settings saved successfully' });
    } catch (err) {
      setMessage({ type: 'error', text: 'Failed to save settings' });
    } finally {
      setIsSaving(false);
    }
  };

  const selectedProvider = rpcProviders.find(p => p.id === rpcProvider);

  // Render auto-sync status banner
  const renderAutoSyncBanner = () => {
    if (!autoSyncStatus || autoSyncStatus.status === null || autoSyncStatus.status === 'skipped') {
      return null;
    }

    const statusConfig: Record<string, { icon: React.ReactNode; bgClass: string; textClass: string; borderClass: string }> = {
      success: {
        icon: <CheckCircle className="w-5 h-5 text-green-400" />,
        bgClass: 'bg-green-500/10',
        textClass: 'text-green-300',
        borderClass: 'border-green-500/30',
      },
      no_backup: {
        icon: <Info className="w-5 h-5 text-blue-400" />,
        bgClass: 'bg-blue-500/10',
        textClass: 'text-blue-300',
        borderClass: 'border-blue-500/30',
      },
      server_error: {
        icon: <AlertTriangle className="w-5 h-5 text-yellow-400" />,
        bgClass: 'bg-yellow-500/10',
        textClass: 'text-yellow-300',
        borderClass: 'border-yellow-500/30',
      },
      error: {
        icon: <XCircle className="w-5 h-5 text-red-400" />,
        bgClass: 'bg-red-500/10',
        textClass: 'text-red-300',
        borderClass: 'border-red-500/30',
      },
    };

    const config = statusConfig[autoSyncStatus.status] || statusConfig.error;

    return (
      <div className={`mb-6 p-4 rounded-lg border ${config.bgClass} ${config.borderClass}`}>
        <div className="flex items-start gap-3">
          {config.icon}
          <div className="flex-1">
            <div className={`font-medium ${config.textClass}`}>
              {autoSyncStatus.status === 'success' ? 'Cloud Backup Restored' :
               autoSyncStatus.status === 'no_backup' ? 'No Cloud Backup Found' :
               autoSyncStatus.status === 'server_error' ? 'Keystore Server Unreachable' :
               'Auto-Sync Error'}
            </div>
            <p className="text-sm text-slate-400 mt-1">
              {autoSyncStatus.message}
            </p>
            {autoSyncStatus.synced_at && (
              <p className="text-xs text-slate-500 mt-2">
                {new Date(autoSyncStatus.synced_at).toLocaleString()}
              </p>
            )}
            {autoSyncStatus.status === 'no_backup' && (
              <p className="text-xs text-slate-400 mt-2">
                Go to <a href="/api-keys" className="text-stark-400 hover:text-stark-300 underline">API Keys</a> to backup your settings to the cloud.
              </p>
            )}
          </div>
        </div>
      </div>
    );
  };

  if (isLoading) {
    return (
      <div className="p-8 flex items-center justify-center">
        <div className="flex items-center gap-3">
          <div className="w-6 h-6 border-2 border-stark-500 border-t-transparent rounded-full animate-spin" />
          <span className="text-slate-400">Loading settings...</span>
        </div>
      </div>
    );
  }

  return (
    <div className="p-8">
      <div className="mb-8">
        <h1 className="text-2xl font-bold text-white mb-2">Bot Settings</h1>
        <p className="text-slate-400">Configure bot identity and RPC settings</p>
      </div>

      {/* Auto-sync status banner */}
      {renderAutoSyncBanner()}

      <form onSubmit={handleSubmit} className="grid gap-6 max-w-2xl">
        {/* Bot Identity Section */}
        <Card>
          <CardHeader>
            <CardTitle className="flex items-center gap-2">
              <Bot className="w-5 h-5 text-stark-400" />
              Bot Identity
            </CardTitle>
          </CardHeader>
          <CardContent className="space-y-4">
            <Input
              label="Bot Name"
              value={botName}
              onChange={(e) => setBotName(e.target.value)}
              placeholder="StarkBot"
            />
            <p className="text-xs text-slate-500 -mt-2">
              Used for git commits and identification
            </p>

            <Input
              label="Bot Email"
              value={botEmail}
              onChange={(e) => setBotEmail(e.target.value)}
              placeholder="starkbot@users.noreply.github.com"
              type="email"
            />
            <p className="text-xs text-slate-500 -mt-2">
              Used for git commit author email
            </p>

            {/* Wallet Address (read-only) */}
            <div className="pt-4 border-t border-slate-700">
              <label className="block text-sm font-medium text-slate-300 mb-2">
                Wallet Address
              </label>
              {walletAddress ? (
                <div className="flex items-center gap-3">
                  <div className="flex items-center gap-2 bg-slate-800 border border-slate-700 px-3 py-2 rounded-lg flex-1">
                    <Wallet className="w-4 h-4 text-slate-400" />
                    <span className="font-mono text-sm text-white">{truncateAddress(walletAddress)}</span>
                    <span className="font-mono text-xs text-slate-500 hidden sm:inline">({walletAddress})</span>
                  </div>
                  <button
                    type="button"
                    onClick={copyWalletAddress}
                    className="p-2 bg-slate-700 hover:bg-slate-600 rounded-lg transition-colors"
                    title="Copy full address"
                  >
                    {walletCopied ? (
                      <Check className="w-4 h-4 text-green-400" />
                    ) : (
                      <Copy className="w-4 h-4 text-slate-400" />
                    )}
                  </button>
                  {walletMode === 'flash' && (
                    <span className="text-xs px-2 py-1 rounded font-medium bg-purple-500/20 text-purple-400">
                      Flash
                    </span>
                  )}
                </div>
              ) : (
                <div className="flex items-center gap-2 bg-amber-500/10 border border-amber-500/30 px-3 py-2 rounded-lg">
                  <Wallet className="w-4 h-4 text-amber-400" />
                  <span className="text-sm text-amber-400">No wallet configured</span>
                </div>
              )}
              <p className="text-xs text-slate-500 mt-1">
                The wallet address used by the bot for transactions. Configure in environment variables.
              </p>
            </div>
          </CardContent>
        </Card>

        {/* Theme Accent Section */}
        <Card>
          <CardHeader>
            <CardTitle className="flex items-center gap-2">
              <Palette className="w-5 h-5 text-stark-400" />
              Theme Accent
            </CardTitle>
          </CardHeader>
          <CardContent className="space-y-4">
            <div>
              <label className="block text-sm font-medium text-slate-300 mb-2">
                Accent Color
              </label>
              <select
                value={themeAccent}
                onChange={(e) => handleThemeChange(e.target.value)}
                className="w-full px-3 py-2 bg-slate-800 border border-slate-700 rounded-lg text-white focus:border-stark-500 focus:outline-none"
              >
                <option value="">Spark Orange (Default)</option>
                <option value="blue">Future Blue</option>
                <option value="white">Wicked White</option>
                <option value="green">Circuit Green</option>
              </select>
              <p className="text-xs text-slate-500 mt-1">
                Changes the accent color used throughout the dashboard. Stored locally in your browser.
              </p>
            </div>
          </CardContent>
        </Card>

        {/* RPC Configuration Section */}
        <Card>
          <CardHeader>
            <CardTitle className="flex items-center gap-2">
              <Server className="w-5 h-5 text-stark-400" />
              RPC Configuration
            </CardTitle>
          </CardHeader>
          <CardContent className="space-y-4">
            <div>
              <label className="block text-sm font-medium text-slate-300 mb-2">
                RPC Provider
              </label>
              <select
                value={rpcProvider}
                onChange={(e) => setRpcProvider(e.target.value)}
                className="w-full px-3 py-2 bg-slate-800 border border-slate-700 rounded-lg text-white focus:border-stark-500 focus:outline-none"
              >
                {rpcProviders.map((provider) => (
                  <option key={provider.id} value={provider.id}>
                    {provider.display_name}
                  </option>
                ))}
              </select>
              {selectedProvider && (
                <p className="text-xs text-slate-500 mt-1">
                  {selectedProvider.description}
                  {selectedProvider.x402 && (
                    <span className="ml-2 text-stark-400">(x402 payment enabled)</span>
                  )}
                </p>
              )}
            </div>

            {rpcProvider === 'custom' && (
              <div className="space-y-4 p-4 bg-slate-800/50 rounded-lg">
                <p className="text-sm text-slate-400 mb-2">
                  Enter your custom RPC endpoints. These will be used without x402 payment.
                </p>
                <Input
                  label="Base Network RPC URL"
                  value={customRpcBase}
                  onChange={(e) => setCustomRpcBase(e.target.value)}
                  placeholder="https://mainnet.base.org"
                />
                <Input
                  label="Mainnet RPC URL"
                  value={customRpcMainnet}
                  onChange={(e) => setCustomRpcMainnet(e.target.value)}
                  placeholder="https://eth-mainnet.g.alchemy.com/v2/..."
                />
                <Input
                  label="Polygon RPC URL"
                  value={customRpcPolygon}
                  onChange={(e) => setCustomRpcPolygon(e.target.value)}
                  placeholder="https://polygon-mainnet.g.alchemy.com/v2/..."
                />
              </div>
            )}
          </CardContent>
        </Card>

        {/* Safe Mode Rate Limiting Section */}
        <Card>
          <CardHeader>
            <CardTitle className="flex items-center gap-2">
              <Shield className="w-5 h-5 text-stark-400" />
              Safe Mode Rate Limiting
            </CardTitle>
          </CardHeader>
          <CardContent className="space-y-4">
            <Input
              label="Max Queries per User (per 10 minutes)"
              type="number"
              min={1}
              max={100}
              value={safeModeMaxQueries}
              onChange={(e) => setSafeModeMaxQueries(parseInt(e.target.value) || 5)}
            />
            <p className="text-xs text-slate-500 -mt-2">
              Maximum number of safe mode queries each user can make within a 10-minute window.
              Applies to non-admin users on Discord, Twitter mentions, etc.
            </p>
          </CardContent>
        </Card>

        {/* Session Memory Section */}
        <Card>
          <CardHeader>
            <CardTitle className="flex items-center gap-2">
              <Brain className="w-5 h-5 text-stark-400" />
              Session Memory
            </CardTitle>
          </CardHeader>
          <CardContent className="space-y-4">
            <label className="flex items-center gap-3 cursor-pointer">
              <input
                type="checkbox"
                checked={chatSessionMemoryGeneration}
                onChange={(e) => setChatSessionMemoryGeneration(e.target.checked)}
                className="w-4 h-4 rounded border-slate-600 bg-slate-800 text-stark-500 focus:ring-stark-500"
              />
              <span className="text-sm text-slate-300">
                Log completed sessions to daily memory
              </span>
            </label>
            <p className="text-xs text-slate-500">
              When enabled, the user's input and the bot's final response are appended to the daily memory log
              when a chat session completes. Safe mode sessions are logged under the safemode identity.
            </p>
          </CardContent>
        </Card>

        {/* Guest Dashboard Section */}
        <Card>
          <CardHeader>
            <CardTitle className="flex items-center gap-2">
              <Shield className="w-5 h-5 text-stark-400" />
              Guest Dashboard
            </CardTitle>
          </CardHeader>
          <CardContent className="space-y-4">
            <label className="flex items-center gap-3 cursor-pointer">
              <input
                type="checkbox"
                checked={guestDashboardEnabled}
                onChange={(e) => setGuestDashboardEnabled(e.target.checked)}
                className="w-4 h-4 rounded border-slate-600 bg-slate-800 text-stark-500 focus:ring-stark-500"
              />
              <span className="text-sm text-slate-300">
                Enable Guest Dashboard
              </span>
            </label>
            <p className="text-xs text-slate-500">
              Allow unauthenticated users to view the mind map dashboard without logging in.
            </p>
          </CardContent>
        </Card>

        {/* Cloud Backup Configuration Section */}
        <Card>
          <CardHeader>
            <CardTitle className="flex items-center gap-2">
              <Cloud className="w-5 h-5 text-stark-400" />
              Cloud Backup Configuration
            </CardTitle>
          </CardHeader>
          <CardContent className="space-y-4">
            <Input
              label="Keystore Server URL"
              value={keystoreUrl}
              onChange={(e) => setKeystoreUrl(e.target.value)}
              placeholder="https://keystore.defirelay.com"
            />
            <p className="text-xs text-slate-500 -mt-2">
              Custom keystore server URL for cloud backups. Leave empty to use the default server
              (https://keystore.defirelay.com). Requires x402 payment support.
            </p>
          </CardContent>
        </Card>

        {/* HTTP Proxy Section */}
        <Card>
          <CardHeader>
            <CardTitle className="flex items-center gap-2">
              <Globe className="w-5 h-5 text-stark-400" />
              HTTP Proxy
            </CardTitle>
          </CardHeader>
          <CardContent className="space-y-4">
            <Input
              label="Proxy URL"
              value={proxyUrl}
              onChange={(e) => setProxyUrl(e.target.value)}
              placeholder="http://proxy:8080"
            />
            <p className="text-xs text-slate-500 -mt-2">
              Optional HTTP proxy URL for tool requests (e.g. http://proxy:8080).
              Leave empty to connect directly. Does not affect AI model API calls.
            </p>
          </CardContent>
        </Card>

        {/* Operating Mode Section */}
        <OperatingModeCard
          rogueModeEnabled={rogueModeEnabled}
          onModeChange={setRogueModeEnabled}
          onMessage={setMessage}
        />

        <Button type="submit" isLoading={isSaving} className="w-fit">
          <Save className="w-4 h-4 mr-2" />
          Save Settings
        </Button>

        {message && (
          <div
            className={`px-4 py-3 rounded-lg ${
              message.type === 'success'
                ? 'bg-green-500/20 border border-green-500/50 text-green-400'
                : 'bg-red-500/20 border border-red-500/50 text-red-400'
            }`}
          >
            {message.text}
          </div>
        )}
      </form>
    </div>
  );
}
