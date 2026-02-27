import { useState, useEffect, FormEvent } from 'react';
import { Save, Settings, Ban, CreditCard, Coins, Globe, Info, ExternalLink } from 'lucide-react';
import Card, { CardContent, CardHeader, CardTitle } from '@/components/ui/Card';
import Button from '@/components/ui/Button';
import Input from '@/components/ui/Input';
import UnicodeSpinner from '@/components/ui/UnicodeSpinner';
import { getAgentSettings, updateAgentSettings, getBotSettings, updateBotSettings, getAiEndpointPresets, AiEndpointPreset, getCreditBalance } from '@/lib/api';
import { useWallet } from '@/hooks/useWallet';

type ModelArchetype = 'kimi' | 'llama' | 'claude' | 'openai' | 'minimax';
type PaymentMode = 'none' | 'credits' | 'x402' | 'custom';

interface SettingsData {
  endpoint_name?: string | null;
  endpoint?: string;
  model_archetype?: string;
  model?: string | null;
  max_response_tokens?: number;
  max_context_tokens?: number;
  has_secret_key?: boolean;
  enabled?: boolean;
  payment_mode?: string;
}

const MODE_CARDS: { mode: PaymentMode; title: string; subtitle: string; icon: typeof Ban }[] = [
  { mode: 'none', title: 'None', subtitle: 'AI disabled', icon: Ban },
  { mode: 'credits', title: 'DefiRelay Credits', subtitle: 'ERC-8128 credits', icon: CreditCard },
  { mode: 'x402', title: 'DefiRelay x402', subtitle: 'Pay-per-call USDC', icon: Coins },
  { mode: 'custom', title: 'Custom', subtitle: 'Your own endpoint', icon: Globe },
];

export default function AgentSettings() {
  const { usdcBalance } = useWallet();
  const [presets, setPresets] = useState<AiEndpointPreset[]>([]);
  const [paymentMode, setPaymentMode] = useState<PaymentMode>('none');
  const [endpointOption, setEndpointOption] = useState<string>('minimax');
  const [customEndpoint, setCustomEndpoint] = useState('');
  const [modelArchetype, setModelArchetype] = useState<ModelArchetype>('minimax');
  const [maxResponseTokens, setMaxResponseTokens] = useState(40000);
  const [maxContextTokens, setMaxContextTokens] = useState(100000);
  const [secretKey, setSecretKey] = useState('');
  const [hasExistingSecretKey, setHasExistingSecretKey] = useState(false);
  const [maxToolIterations, setMaxToolIterations] = useState(50);
  const [isLoading, setIsLoading] = useState(true);
  const [isSaving, setIsSaving] = useState(false);
  const [isSavingBehavior, setIsSavingBehavior] = useState(false);
  const [message, setMessage] = useState<{ type: 'success' | 'error'; text: string } | null>(null);
  const [creditBalance, setCreditBalance] = useState<number | null>(null);
  const [creditBalanceLoading, setCreditBalanceLoading] = useState(false);

  useEffect(() => {
    loadPresets();
    loadBotSettings();
    loadCreditBalance();
  }, []);

  const loadPresets = async () => {
    let loadedPresets: AiEndpointPreset[] = [];
    try {
      loadedPresets = await getAiEndpointPresets();
      setPresets(loadedPresets);
    } catch (err) {
      console.error('Failed to load AI endpoint presets:', err);
    } finally {
      loadSettings(loadedPresets);
    }
  };

  // Lock archetype for preset endpoints
  useEffect(() => {
    if (paymentMode === 'credits' || paymentMode === 'x402') {
      const preset = presets.find(p => p.id === endpointOption);
      if (preset) {
        setModelArchetype(preset.model_archetype as ModelArchetype);
      }
    }
  }, [endpointOption, presets, paymentMode]);

  const isArchetypeLocked = paymentMode !== 'custom';

  const loadSettings = async (loadedPresets: AiEndpointPreset[]) => {
    try {
      const data = await getAgentSettings() as SettingsData;

      // Detect payment_mode from API response
      if (data.payment_mode && ['none', 'credits', 'x402', 'custom'].includes(data.payment_mode)) {
        setPaymentMode(data.payment_mode as PaymentMode);
      } else if (data.enabled === false) {
        setPaymentMode('none');
      } else if (data.endpoint && !data.endpoint.includes('defirelay.com')) {
        setPaymentMode('custom');
      } else {
        setPaymentMode('x402');
      }

      // Match dropdown by endpoint_name
      if (data.endpoint_name && loadedPresets.some(p => p.id === data.endpoint_name)) {
        setEndpointOption(data.endpoint_name);
      } else if (data.endpoint) {
        if (data.endpoint.includes('defirelay.com')) {
          setEndpointOption(loadedPresets.length > 0 ? loadedPresets[0].id : 'custom');
        } else {
          setCustomEndpoint(data.endpoint);
        }
      } else {
        setEndpointOption(loadedPresets.length > 0 ? loadedPresets[0].id : 'custom');
      }

      setHasExistingSecretKey(data.has_secret_key ?? false);

      if (data.model_archetype && ['kimi', 'llama', 'claude', 'openai', 'minimax'].includes(data.model_archetype)) {
        setModelArchetype(data.model_archetype as ModelArchetype);
      }
      if (data.max_response_tokens && data.max_response_tokens > 0) {
        setMaxResponseTokens(data.max_response_tokens);
      }
      if (data.max_context_tokens && data.max_context_tokens > 0) {
        setMaxContextTokens(data.max_context_tokens);
      }
    } catch (err) {
      setMessage({ type: 'error', text: 'Failed to load settings' });
    } finally {
      setIsLoading(false);
    }
  };

  const loadBotSettings = async () => {
    try {
      const data = await getBotSettings();
      setMaxToolIterations(data.max_tool_iterations || 50);
    } catch (err) {
      console.error('Failed to load bot settings:', err);
    }
  };

  const loadCreditBalance = async () => {
    setCreditBalanceLoading(true);
    try {
      const data = await getCreditBalance();
      if (data.error) {
        console.warn('Credit balance error:', data.error);
      } else if (data.credits !== undefined) {
        setCreditBalance(data.credits);
      }
    } catch (err) {
      console.error('Failed to load credit balance:', err);
    } finally {
      setCreditBalanceLoading(false);
    }
  };

  const handleBehaviorSubmit = async (e: FormEvent) => {
    e.preventDefault();
    setIsSavingBehavior(true);
    setMessage(null);
    try {
      await updateBotSettings({ max_tool_iterations: maxToolIterations });
      setMessage({ type: 'success', text: 'Agent behavior settings saved successfully' });
    } catch (err) {
      setMessage({ type: 'error', text: 'Failed to save agent behavior settings' });
    } finally {
      setIsSavingBehavior(false);
    }
  };

  const saveSettings = async (overrides?: { paymentMode?: PaymentMode; endpointOption?: string }) => {
    const mode = overrides?.paymentMode ?? paymentMode;
    const selectedEndpointId = overrides?.endpointOption ?? endpointOption;

    setIsSaving(true);
    setMessage(null);

    // For "none" mode, just send payment_mode
    if (mode === 'none') {
      try {
        await updateAgentSettings({ payment_mode: 'none', endpoint: '', model_archetype: 'kimi', max_response_tokens: maxResponseTokens, max_context_tokens: maxContextTokens });
        setMessage({ type: 'success', text: 'AI capabilities disabled' });
      } catch (err) {
        setMessage({ type: 'error', text: 'Failed to save settings' });
      } finally {
        setIsSaving(false);
      }
      return;
    }

    const selectedPreset = (mode === 'credits' || mode === 'x402')
      ? presets.find(p => p.id === selectedEndpointId)
      : null;

    let endpoint: string;
    if (selectedPreset) {
      endpoint = selectedPreset.endpoint;
    } else {
      endpoint = customEndpoint;
    }

    if (mode === 'custom' && !customEndpoint.trim()) {
      setMessage({ type: 'error', text: 'Please enter a custom endpoint URL' });
      setIsSaving(false);
      return;
    }

    const contextTokens = Math.max(maxContextTokens, 80000);
    const archetype = selectedPreset ? selectedPreset.model_archetype : modelArchetype;

    try {
      const payload: Record<string, unknown> = {
        payment_mode: mode,
        endpoint_name: selectedPreset ? selectedPreset.id : null,
        endpoint,
        model_archetype: archetype,
        model: selectedPreset?.model ?? null,
        max_response_tokens: maxResponseTokens,
        max_context_tokens: contextTokens,
      };

      if (mode === 'custom' && secretKey.trim()) {
        payload.secret_key = secretKey;
      }

      await updateAgentSettings(payload);
      setMessage({ type: 'success', text: 'Settings saved' });

      if (mode === 'custom' && secretKey.trim()) {
        setHasExistingSecretKey(true);
        setSecretKey('');
      }
    } catch (err) {
      setMessage({ type: 'error', text: 'Failed to save settings' });
    } finally {
      setIsSaving(false);
    }
  };

  const handleEndpointSubmit = async (e: FormEvent) => {
    e.preventDefault();
    await saveSettings();
  };

  if (isLoading) {
    return (
      <div className="p-8 flex items-center justify-center">
        <div className="flex items-center gap-3">
          <UnicodeSpinner animation="rain" size="lg" className="text-stark-500" />
          <span className="text-slate-400">Loading settings...</span>
        </div>
      </div>
    );
  }

  return (
    <div className="p-8">
      <div className="mb-8">
        <h1 className="text-2xl font-bold text-white mb-2">Agent Settings</h1>
        <p className="text-slate-400">Configure how your AI agent connects to a model provider</p>
      </div>

      <div className="grid gap-6 max-w-2xl">
        {/* Payment Mode Selector */}
        <Card>
          <CardHeader>
            <CardTitle>Payment Mode</CardTitle>
          </CardHeader>
          <CardContent>
            {/* Payment mode info banner */}
            <div className={`flex items-start gap-3 px-4 py-3 rounded-lg mb-4 ${
              paymentMode === 'none'
                ? 'bg-slate-700/30 border border-slate-600/50'
                : paymentMode === 'credits'
                ? 'bg-green-500/10 border border-green-500/30'
                : paymentMode === 'x402'
                ? 'bg-blue-500/10 border border-blue-500/30'
                : 'bg-amber-500/10 border border-amber-500/30'
            }`}>
              <Info className={`w-4 h-4 mt-0.5 flex-shrink-0 ${
                paymentMode === 'none' ? 'text-slate-400' :
                paymentMode === 'credits' ? 'text-green-400' :
                paymentMode === 'x402' ? 'text-blue-400' :
                'text-amber-400'
              }`} />
              <div className="text-sm">
                {paymentMode === 'none' && (
                  <span className="text-slate-400">Agentic intelligence is disabled. Select a payment mode to enable AI capabilities.</span>
                )}
                {paymentMode === 'credits' && (
                  <span className="text-green-300">
                    {creditBalanceLoading ? (
                      'Loading credit balance...'
                    ) : creditBalance !== null ? (
                      <>Credit balance: <span className="font-mono font-semibold text-white">{(creditBalance / 1_000_000).toFixed(2)} USDC</span> in credits. Refill at starkbot.cloud.</>
                    ) : (
                      'Unable to fetch credit balance'
                    )}
                  </span>
                )}
                {paymentMode === 'x402' && (
                  <span className="text-blue-300">
                    {usdcBalance !== null ? (
                      <>USDC balance on Base: <span className="font-mono font-semibold text-white">{parseFloat(usdcBalance).toFixed(2)} USDC</span>. Each API call is paid directly via x402.</>
                    ) : (
                      'Loading USDC balance...'
                    )}
                  </span>
                )}
                {paymentMode === 'custom' && (
                  <span className="text-amber-300">
                    Get an API key and chat completions endpoint from{' '}
                    <a href="https://minimax.io" target="_blank" rel="noopener noreferrer" className="underline hover:text-amber-200 inline-flex items-center gap-0.5">MiniMax<ExternalLink className="w-3 h-3" /></a>,{' '}
                    <a href="https://moonshot.ai" target="_blank" rel="noopener noreferrer" className="underline hover:text-amber-200 inline-flex items-center gap-0.5">Moonshot AI<ExternalLink className="w-3 h-3" /></a>, or{' '}
                    <a href="https://platform.openai.com" target="_blank" rel="noopener noreferrer" className="underline hover:text-amber-200 inline-flex items-center gap-0.5">OpenAI<ExternalLink className="w-3 h-3" /></a>.
                  </span>
                )}
              </div>
            </div>

            <div className="grid grid-cols-2 lg:grid-cols-4 gap-3">
              {MODE_CARDS.map(({ mode, title, subtitle, icon: Icon }) => (
                <button
                  key={mode}
                  type="button"
                  onClick={() => { setPaymentMode(mode); saveSettings({ paymentMode: mode }); }}
                  className={`relative flex flex-col items-start p-4 rounded-lg border-2 transition-all text-left ${
                    paymentMode === mode
                      ? 'border-stark-500 bg-stark-500/10'
                      : 'border-slate-700 bg-slate-800/50 hover:border-slate-600'
                  }`}
                >
                  <div className={`absolute top-3 right-3 w-3 h-3 rounded-full border-2 ${
                    paymentMode === mode
                      ? 'border-stark-500 bg-stark-500'
                      : 'border-slate-600'
                  }`} />
                  <Icon className={`w-5 h-5 mb-2 ${paymentMode === mode ? 'text-stark-400' : 'text-slate-500'}`} />
                  <span className={`text-sm font-medium ${paymentMode === mode ? 'text-white' : 'text-slate-300'}`}>
                    {title}
                  </span>
                  <span className="text-xs text-slate-500">{subtitle}</span>
                </button>
              ))}
            </div>
          </CardContent>
        </Card>

        {/* Mode-specific content */}
        <form onSubmit={handleEndpointSubmit} className="grid gap-6">
          {paymentMode === 'none' && (
            <Card>
              <CardContent>
                <div className="flex items-center gap-3 py-4">
                  <Ban className="w-6 h-6 text-slate-500" />
                  <div>
                    <p className="text-slate-300 font-medium">AI capabilities are disabled</p>
                    <p className="text-sm text-slate-500">Select a payment mode above to enable AI features</p>
                  </div>
                </div>
              </CardContent>
            </Card>
          )}

          {(paymentMode === 'credits' || paymentMode === 'x402') && (
            <Card>
              <CardHeader>
                <CardTitle>DefiRelay Endpoint</CardTitle>
              </CardHeader>
              <CardContent>
                <div className="space-y-4">
                  <div>
                    <label className="block text-sm font-medium text-slate-300 mb-2">
                      Model Preset
                    </label>
                    <select
                      value={endpointOption}
                      onChange={(e) => { const val = e.target.value; setEndpointOption(val); saveSettings({ endpointOption: val }); }}
                      className="w-full px-4 py-3 bg-slate-900/50 border border-slate-600 rounded-lg text-white focus:outline-none focus:ring-2 focus:ring-stark-500 focus:border-transparent"
                    >
                      {presets.map(preset => (
                        <option key={preset.id} value={preset.id}>
                          {preset.display_name}{preset.x402_cost != null && preset.x402_cost > 0 ? ` (${(preset.x402_cost / 1_000_000).toFixed(4)} USDC/call)` : preset.x402_cost === 0 ? ' (free)' : ''}
                        </option>
                      ))}
                    </select>
                    {(() => {
                      const selected = presets.find(p => p.id === endpointOption);
                      if (selected?.x402_cost != null && selected.x402_cost > 0) {
                        const cost = (selected.x402_cost / 1_000_000).toFixed(4);
                        return (
                          <p className="text-xs text-yellow-400 mt-1">
                            {paymentMode === 'credits' ? 'ERC-8128 credits' : 'x402 payment'}: {cost} USDC per API call
                          </p>
                        );
                      } else if (selected?.x402_cost === 0) {
                        return (
                          <p className="text-xs text-green-400 mt-1">
                            Free — no payment required
                          </p>
                        );
                      }
                      return null;
                    })()}
                  </div>

                  {paymentMode === 'x402' && usdcBalance !== null && (
                    <div className="flex items-center gap-2 px-3 py-2 bg-slate-700/30 rounded-lg">
                      <Coins className="w-4 h-4 text-blue-400" />
                      <span className="text-sm text-slate-300">USDC Balance (Base):</span>
                      <span className="text-sm font-mono text-white">{parseFloat(usdcBalance).toFixed(2)}</span>
                    </div>
                  )}

                  {paymentMode === 'credits' && creditBalance !== null && (
                    <div className="flex items-center gap-2 px-3 py-2 bg-slate-700/30 rounded-lg">
                      <CreditCard className="w-4 h-4 text-green-400" />
                      <span className="text-sm text-slate-300">Credit Balance:</span>
                      <span className="text-sm font-mono text-white">{(creditBalance / 1_000_000).toFixed(2)} USDC</span>
                    </div>
                  )}
                </div>
              </CardContent>
            </Card>
          )}

          {paymentMode === 'custom' && (
            <Card>
              <CardHeader>
                <CardTitle>Custom Endpoint</CardTitle>
              </CardHeader>
              <CardContent>
                <div className="space-y-4">
                  <Input
                    label="Endpoint URL"
                    value={customEndpoint}
                    onChange={(e) => setCustomEndpoint(e.target.value)}
                    placeholder="https://your-endpoint.com/v1/chat/completions"
                  />
                  <div>
                    <label className="block text-sm font-medium text-slate-300 mb-2">
                      API Secret Key
                      {hasExistingSecretKey && (
                        <span className="ml-2 text-xs text-green-400">(configured)</span>
                      )}
                    </label>
                    <input
                      type="password"
                      value={secretKey}
                      onChange={(e) => setSecretKey(e.target.value)}
                      placeholder={hasExistingSecretKey ? 'Leave empty to keep existing key' : 'Enter API key'}
                      className="w-full px-4 py-3 bg-slate-900/50 border border-slate-600 rounded-lg text-white focus:outline-none focus:ring-2 focus:ring-stark-500 focus:border-transparent"
                    />
                    <p className="text-xs text-slate-500 mt-1">
                      Required for standard OpenAI-compatible endpoints
                    </p>
                  </div>
                </div>
              </CardContent>
            </Card>
          )}

          {/* Common settings (shown for all active modes) */}
          {paymentMode !== 'none' && (
            <Card>
              <CardHeader>
                <CardTitle>Model Configuration</CardTitle>
              </CardHeader>
              <CardContent>
                <div className="space-y-4">
                  <div>
                    <label className="block text-sm font-medium text-slate-300 mb-2">
                      Model Archetype
                    </label>
                    <select
                      value={modelArchetype}
                      onChange={(e) => setModelArchetype(e.target.value as ModelArchetype)}
                      disabled={isArchetypeLocked}
                      className={`w-full px-4 py-3 bg-slate-900/50 border border-slate-600 rounded-lg text-white focus:outline-none focus:ring-2 focus:ring-stark-500 focus:border-transparent ${isArchetypeLocked ? 'opacity-60 cursor-not-allowed' : ''}`}
                    >
                      <option value="kimi">Kimi</option>
                      <option value="llama">Llama</option>
                      <option value="claude">Claude</option>
                      <option value="openai">OpenAI</option>
                      <option value="minimax">MiniMax</option>
                    </select>
                    <p className="text-xs text-slate-500 mt-1">
                      {isArchetypeLocked
                        ? `Locked to ${modelArchetype} for this preset`
                        : 'Select the model family to optimize prompt formatting'}
                    </p>
                  </div>

                  <div>
                    <label className="block text-sm font-medium text-slate-300 mb-2">
                      Max Response Tokens
                    </label>
                    <input
                      type="number"
                      value={maxResponseTokens}
                      onChange={(e) => setMaxResponseTokens(parseInt(e.target.value) || 40000)}
                      min={1000}
                      max={200000}
                      className="w-full px-4 py-3 bg-slate-900/50 border border-slate-600 rounded-lg text-white focus:outline-none focus:ring-2 focus:ring-stark-500 focus:border-transparent"
                    />
                    <p className="text-xs text-slate-500 mt-1">
                      Maximum tokens for AI response output (default: 40,000)
                    </p>
                  </div>

                  <div>
                    <label className="block text-sm font-medium text-slate-300 mb-2">
                      Max Context Tokens
                    </label>
                    <input
                      type="number"
                      value={maxContextTokens}
                      onChange={(e) => setMaxContextTokens(parseInt(e.target.value) || 100000)}
                      min={80000}
                      max={200000}
                      className="w-full px-4 py-3 bg-slate-900/50 border border-slate-600 rounded-lg text-white focus:outline-none focus:ring-2 focus:ring-stark-500 focus:border-transparent"
                    />
                    <p className="text-xs text-slate-500 mt-1">
                      Context window limit for conversation history (min: 80,000, default: 100,000)
                    </p>
                  </div>

                  <Button type="submit" isLoading={isSaving} className="w-fit">
                    <Save className="w-4 h-4 mr-2" />
                    Save Settings
                  </Button>
                </div>
              </CardContent>
            </Card>
          )}

          {paymentMode === 'none' && (
            <Button type="submit" isLoading={isSaving} className="w-fit">
              <Save className="w-4 h-4 mr-2" />
              Save Settings
            </Button>
          )}
        </form>

        {/* Agent Behavior Section */}
        <Card>
          <CardHeader>
            <CardTitle className="flex items-center gap-2">
              <Settings className="w-5 h-5 text-stark-400" />
              Agent Behavior
            </CardTitle>
          </CardHeader>
          <CardContent>
            <form onSubmit={handleBehaviorSubmit} className="space-y-4">
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

              <Button type="submit" isLoading={isSavingBehavior} className="w-fit">
                <Save className="w-4 h-4 mr-2" />
                Save Behavior Settings
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
