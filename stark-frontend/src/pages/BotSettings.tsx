import { useState, useEffect, FormEvent } from 'react';
import { Save, Bot, Server, Settings } from 'lucide-react';
import Card, { CardContent, CardHeader, CardTitle } from '@/components/ui/Card';
import Button from '@/components/ui/Button';
import Input from '@/components/ui/Input';
import { getBotSettings, updateBotSettings, getRpcProviders, BotSettings as BotSettingsType, RpcProvider } from '@/lib/api';

export default function BotSettings() {
  const [, setSettings] = useState<BotSettingsType | null>(null);
  const [botName, setBotName] = useState('StarkBot');
  const [botEmail, setBotEmail] = useState('starkbot@users.noreply.github.com');
  const [rpcProvider, setRpcProvider] = useState('defirelay');
  const [customRpcBase, setCustomRpcBase] = useState('');
  const [customRpcMainnet, setCustomRpcMainnet] = useState('');
  const [maxToolIterations, setMaxToolIterations] = useState(50);
  const [rpcProviders, setRpcProviders] = useState<RpcProvider[]>([]);
  const [isLoading, setIsLoading] = useState(true);
  const [isSaving, setIsSaving] = useState(false);
  const [message, setMessage] = useState<{ type: 'success' | 'error'; text: string } | null>(null);

  useEffect(() => {
    loadSettings();
    loadRpcProviders();
  }, []);

  const loadSettings = async () => {
    try {
      const data = await getBotSettings();
      setSettings(data);
      setBotName(data.bot_name);
      setBotEmail(data.bot_email);
      setRpcProvider(data.rpc_provider || 'defirelay');
      setMaxToolIterations(data.max_tool_iterations || 50);
      if (data.custom_rpc_endpoints) {
        setCustomRpcBase(data.custom_rpc_endpoints.base || '');
        setCustomRpcMainnet(data.custom_rpc_endpoints.mainnet || '');
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
      const updated = await updateBotSettings({
        bot_name: botName,
        bot_email: botEmail,
      });
      setSettings(updated);
      setMessage({ type: 'success', text: 'Settings saved successfully' });
    } catch (err) {
      setMessage({ type: 'error', text: 'Failed to save settings' });
    } finally {
      setIsSaving(false);
    }
  };

  const handleRpcSubmit = async (e: FormEvent) => {
    e.preventDefault();
    setIsSaving(true);
    setMessage(null);

    try {
      const customEndpoints = rpcProvider === 'custom' ? {
        base: customRpcBase,
        mainnet: customRpcMainnet,
      } : undefined;

      const updated = await updateBotSettings({
        rpc_provider: rpcProvider,
        custom_rpc_endpoints: customEndpoints,
      });
      setSettings(updated);
      setMessage({ type: 'success', text: 'RPC settings saved successfully' });
    } catch (err) {
      setMessage({ type: 'error', text: 'Failed to save RPC settings' });
    } finally {
      setIsSaving(false);
    }
  };

  const selectedProvider = rpcProviders.find(p => p.id === rpcProvider);

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

      <div className="grid gap-6 max-w-2xl">
        {/* Bot Identity Section */}
        <Card>
          <CardHeader>
            <CardTitle className="flex items-center gap-2">
              <Bot className="w-5 h-5 text-stark-400" />
              Bot Identity
            </CardTitle>
          </CardHeader>
          <CardContent>
            <form onSubmit={handleSubmit} className="space-y-4">
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

              <Button type="submit" isLoading={isSaving} className="w-fit">
                <Save className="w-4 h-4 mr-2" />
                Save Identity
              </Button>
            </form>
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
          <CardContent>
            <form onSubmit={handleRpcSubmit} className="space-y-4">
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
                </div>
              )}

              <Button type="submit" isLoading={isSaving} className="w-fit">
                <Save className="w-4 h-4 mr-2" />
                Save RPC Settings
              </Button>
            </form>
          </CardContent>
        </Card>

        {/* Agent Behavior Section */}
        <Card>
          <CardHeader>
            <CardTitle className="flex items-center gap-2">
              <Settings className="w-5 h-5 text-stark-400" />
              Agent Behavior
            </CardTitle>
          </CardHeader>
          <CardContent>
            <form onSubmit={async (e: FormEvent) => {
              e.preventDefault();
              setIsSaving(true);
              setMessage(null);
              try {
                const updated = await updateBotSettings({
                  max_tool_iterations: maxToolIterations,
                });
                setSettings(updated);
                setMessage({ type: 'success', text: 'Agent settings saved successfully' });
              } catch (err) {
                setMessage({ type: 'error', text: 'Failed to save agent settings' });
              } finally {
                setIsSaving(false);
              }
            }} className="space-y-4">
              <div>
                <label className="block text-sm font-medium text-slate-300 mb-2">
                  Max Tool Iterations
                </label>
                <input
                  type="number"
                  min={10}
                  max={200}
                  value={maxToolIterations}
                  onChange={(e) => setMaxToolIterations(parseInt(e.target.value) || 50)}
                  className="w-full px-3 py-2 bg-slate-800 border border-slate-700 rounded-lg text-white focus:border-stark-500 focus:outline-none"
                />
                <p className="text-xs text-slate-500 mt-1">
                  Maximum number of tool calls per request (10-200). Higher values allow for more complex tasks but may take longer.
                </p>
              </div>

              <Button type="submit" isLoading={isSaving} className="w-fit">
                <Save className="w-4 h-4 mr-2" />
                Save Agent Settings
              </Button>
            </form>
          </CardContent>
        </Card>

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
      </div>
    </div>
  );
}
