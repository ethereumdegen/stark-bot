import { useState, useEffect, FormEvent } from 'react';
import { Save, Settings } from 'lucide-react';
import Card, { CardContent, CardHeader, CardTitle } from '@/components/ui/Card';
import Button from '@/components/ui/Button';
import Input from '@/components/ui/Input';
import { getAgentSettings, updateAgentSettings, getBotSettings, updateBotSettings, getAiEndpointPresets, AiEndpointPreset } from '@/lib/api';

type ModelArchetype = 'kimi' | 'llama' | 'claude' | 'openai' | 'minimax';

interface Settings {
  endpoint?: string;
  model_archetype?: string;
  model?: string | null;
  max_response_tokens?: number;
  max_context_tokens?: number;
  has_secret_key?: boolean;
}

export default function AgentSettings() {
  const [presets, setPresets] = useState<AiEndpointPreset[]>([]);
  const [endpointOption, setEndpointOption] = useState<string>('kimi');
  const [customEndpoint, setCustomEndpoint] = useState('');
  const [modelArchetype, setModelArchetype] = useState<ModelArchetype>('kimi');
  const [maxResponseTokens, setMaxResponseTokens] = useState(40000);
  const [maxContextTokens, setMaxContextTokens] = useState(100000);
  const [secretKey, setSecretKey] = useState('');
  const [hasExistingSecretKey, setHasExistingSecretKey] = useState(false);
  const [maxToolIterations, setMaxToolIterations] = useState(50);
  const [isLoading, setIsLoading] = useState(true);
  const [isSaving, setIsSaving] = useState(false);
  const [isSavingBehavior, setIsSavingBehavior] = useState(false);
  const [message, setMessage] = useState<{ type: 'success' | 'error'; text: string } | null>(null);

  useEffect(() => {
    loadPresets();
    loadBotSettings();
  }, []);

  const loadPresets = async () => {
    let loadedPresets: AiEndpointPreset[] = [];
    try {
      loadedPresets = await getAiEndpointPresets();
      setPresets(loadedPresets);
    } catch (err) {
      console.error('Failed to load AI endpoint presets:', err);
    } finally {
      // Pass presets directly since setPresets hasn't applied yet
      loadSettings(loadedPresets);
    }
  };

  // Lock archetype for preset endpoints
  useEffect(() => {
    const preset = presets.find(p => p.id === endpointOption);
    if (preset) {
      setModelArchetype(preset.model_archetype as ModelArchetype);
    }
  }, [endpointOption, presets]);

  // Archetype is only selectable for custom endpoints
  const isArchetypeLocked = endpointOption !== 'custom';

  const loadSettings = async (loadedPresets: AiEndpointPreset[]) => {
    try {
      const data = await getAgentSettings() as Settings;

      // Determine which endpoint option is being used (match by endpoint + model)
      const matchedPreset = loadedPresets.find(p => p.endpoint === data.endpoint && p.model === data.model)
        ?? loadedPresets.find(p => p.endpoint === data.endpoint);
      if (matchedPreset) {
        setEndpointOption(matchedPreset.id);
      } else if (data.endpoint) {
        setEndpointOption('custom');
        setCustomEndpoint(data.endpoint);
      } else {
        setEndpointOption(loadedPresets.length > 0 ? loadedPresets[0].id : 'custom');
      }

      // Set secret key indicator
      setHasExistingSecretKey(data.has_secret_key ?? false);

      // Set model archetype
      if (data.model_archetype && ['kimi', 'llama', 'claude', 'openai', 'minimax'].includes(data.model_archetype)) {
        setModelArchetype(data.model_archetype as ModelArchetype);
      }

      // Set token limits
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

  const handleBehaviorSubmit = async (e: FormEvent) => {
    e.preventDefault();
    setIsSavingBehavior(true);
    setMessage(null);
    try {
      await updateBotSettings({
        max_tool_iterations: maxToolIterations,
      });
      setMessage({ type: 'success', text: 'Agent behavior settings saved successfully' });
    } catch (err) {
      setMessage({ type: 'error', text: 'Failed to save agent behavior settings' });
    } finally {
      setIsSavingBehavior(false);
    }
  };

  const handleEndpointSubmit = async (e: FormEvent) => {
    e.preventDefault();
    setIsSaving(true);
    setMessage(null);

    let endpoint: string;
    const selectedPreset = presets.find(p => p.id === endpointOption);
    if (selectedPreset) {
      endpoint = selectedPreset.endpoint;
    } else {
      endpoint = customEndpoint;
    }

    if (endpointOption === 'custom' && !customEndpoint.trim()) {
      setMessage({ type: 'error', text: 'Please enter a custom endpoint URL' });
      setIsSaving(false);
      return;
    }

    // Enforce minimum context tokens
    const contextTokens = Math.max(maxContextTokens, 80000);

    try {
      // Enforce archetype for preset endpoints
      const archetype = selectedPreset ? selectedPreset.model_archetype : modelArchetype;

      // Only include secret_key for custom endpoints, and only if provided
      const payload: {
        endpoint: string;
        model_archetype: string;
        model?: string | null;
        max_response_tokens: number;
        max_context_tokens: number;
        secret_key?: string;
      } = {
        endpoint,
        model_archetype: archetype,
        model: selectedPreset?.model ?? null,
        max_response_tokens: maxResponseTokens,
        max_context_tokens: contextTokens,
      };

      if (endpointOption === 'custom' && secretKey.trim()) {
        payload.secret_key = secretKey;
      }

      await updateAgentSettings(payload);
      setMessage({ type: 'success', text: 'Endpoint settings saved successfully' });

      // Update the indicator if we saved a new key
      if (endpointOption === 'custom' && secretKey.trim()) {
        setHasExistingSecretKey(true);
        setSecretKey(''); // Clear the input after saving
      }
    } catch (err) {
      setMessage({ type: 'error', text: 'Failed to save endpoint settings' });
    } finally {
      setIsSaving(false);
    }
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
        <h1 className="text-2xl font-bold text-white mb-2">Agent Settings</h1>
        <p className="text-slate-400">Configure your AI agent endpoint and model type</p>
      </div>

      <div className="grid gap-6 max-w-2xl">
        <Card>
          <CardHeader>
            <CardTitle>Endpoint Configuration</CardTitle>
          </CardHeader>
          <CardContent>
            <form onSubmit={handleEndpointSubmit} className="space-y-4">
              <div>
                <label className="block text-sm font-medium text-slate-300 mb-2">
                  Agent Endpoint
                </label>
                <select
                  value={endpointOption}
                  onChange={(e) => setEndpointOption(e.target.value)}
                  className="w-full px-4 py-3 bg-slate-900/50 border border-slate-600 rounded-lg text-white focus:outline-none focus:ring-2 focus:ring-stark-500 focus:border-transparent"
                >
                  {presets.map(preset => (
                    <option key={preset.id} value={preset.id}>
                      {preset.display_name}{preset.x402_cost ? ` (${(preset.x402_cost / 1_000_000).toFixed(4)} USDC/call)` : ''}
                    </option>
                  ))}
                  <option value="custom">Custom Endpoint</option>
                </select>
                {(() => {
                  const selected = presets.find(p => p.id === endpointOption);
                  if (selected?.x402_cost) {
                    const cost = (selected.x402_cost / 1_000_000).toFixed(4);
                    return (
                      <p className="text-xs text-yellow-400 mt-1">
                        x402 payment: {cost} USDC per API call
                      </p>
                    );
                  }
                  return null;
                })()}
              </div>

              {endpointOption === 'custom' && (
                <>
                  <Input
                    label="Custom Endpoint URL"
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
                      placeholder={hasExistingSecretKey ? "Leave empty to keep existing key" : "Leave empty if using x402 endpoint (defirelay.com)"}
                      className="w-full px-4 py-3 bg-slate-900/50 border border-slate-600 rounded-lg text-white focus:outline-none focus:ring-2 focus:ring-stark-500 focus:border-transparent"
                    />
                    <p className="text-xs text-slate-500 mt-1">
                      Required for standard OpenAI-compatible endpoints. Not needed for x402 endpoints (defirelay.com).
                    </p>
                  </div>
                </>
              )}

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
                    ? `Locked to ${modelArchetype} for this endpoint`
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
                  Context window limit for conversation history (min: 80,000, default: 100,000). Controls when compaction triggers.
                </p>
              </div>

              <Button type="submit" isLoading={isSaving} className="w-fit">
                <Save className="w-4 h-4 mr-2" />
                Save Endpoint Settings
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
