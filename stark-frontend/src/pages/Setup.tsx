import { useState, useCallback } from 'react';
import { Key, ArrowRight, Loader2, CheckCircle, AlertCircle, Rocket, Bot } from 'lucide-react';
import { apiFetch } from '@/lib/api';

interface SetupProps {
  initialStep?: 'api_key' | 'deploy';
  onComplete: () => void;
}

interface AgentInfo {
  capability: string;
  name: string;
  agent_id: string;
  status: string;
}

export default function Setup({ initialStep = 'api_key', onComplete }: SetupProps) {
  const [phase, setPhase] = useState<'api_key' | 'deploy'>(initialStep);
  const [apiKey, setApiKey] = useState('');
  const [keyStep, setKeyStep] = useState<'input' | 'saving' | 'initializing' | 'done' | 'error'>('input');
  const [deployStep, setDeployStep] = useState<'ready' | 'deploying' | 'done' | 'error'>('ready');
  const [error, setError] = useState('');
  const [agents, setAgents] = useState<AgentInfo[]>([]);

  // ── Step 1: API Key ──────────────────────────────────

  const handleKeySubmit = useCallback(async () => {
    if (!apiKey.trim()) return;
    setKeyStep('saving');
    setError('');

    try {
      await apiFetch('/keys', {
        method: 'POST',
        body: JSON.stringify({ key_name: 'STARFLASK_API_KEY', api_key: apiKey.trim() }),
      });

      setKeyStep('initializing');
      const result = await apiFetch<{ status?: string; error?: string }>('/starflask/init', {
        method: 'POST',
      });

      if (result.error) {
        setError(result.error);
        setKeyStep('error');
        return;
      }

      setKeyStep('done');
      setTimeout(() => setPhase('deploy'), 800);
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Failed to initialize');
      setKeyStep('error');
    }
  }, [apiKey]);

  // ── Step 2: Deploy Agents ────────────────────────────

  const handleDeploy = useCallback(async () => {
    setDeployStep('deploying');
    setError('');

    try {
      const result = await apiFetch<{
        status?: string;
        error?: string;
        agents?: AgentInfo[];
        provisioned?: string[];
      }>('/starflask/provision', { method: 'POST' });

      if (result.error) {
        setError(result.error);
        setDeployStep('error');
        return;
      }

      setAgents(result.agents ?? []);
      setDeployStep('done');
      setTimeout(onComplete, 1500);
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Failed to deploy agents');
      setDeployStep('error');
    }
  }, [onComplete]);

  // ── Render ───────────────────────────────────────────

  return (
    <div className="min-h-screen bg-slate-900 flex items-center justify-center p-4">
      <div className="w-full max-w-lg">
        {/* Header */}
        <div className="text-center mb-8">
          <h1 className="text-4xl font-bold text-stark-400 mb-2" style={{ fontFamily: "'Orbitron', sans-serif" }}>
            StarkBot
          </h1>
          <p className="text-slate-400">Command & Control Center</p>
        </div>

        {/* Step indicator */}
        <div className="flex items-center justify-center gap-3 mb-6">
          <StepDot active={phase === 'api_key'} done={phase === 'deploy'} label="1" />
          <div className="w-8 h-px bg-slate-600" />
          <StepDot active={phase === 'deploy'} done={deployStep === 'done'} label="2" />
        </div>

        {/* Step 1: API Key */}
        {phase === 'api_key' && (
          <div className="bg-slate-800 rounded-xl border border-slate-700 p-8">
            <div className="flex items-center gap-3 mb-6">
              <div className="p-3 rounded-lg bg-stark-500/20">
                <Key className="w-6 h-6 text-stark-400" />
              </div>
              <div>
                <h2 className="text-lg font-semibold text-white">Connect to Starflask</h2>
                <p className="text-sm text-slate-400">Enter your Starflask API key to get started</p>
              </div>
            </div>

            {keyStep === 'done' ? (
              <div className="flex items-center gap-3 p-4 rounded-lg bg-green-500/10 border border-green-500/30">
                <CheckCircle className="w-5 h-5 text-green-400" />
                <span className="text-green-300">Connected! Setting up agents...</span>
              </div>
            ) : (
              <>
                <div className="space-y-4">
                  <div>
                    <label className="block text-sm text-slate-400 mb-2">API Key</label>
                    <input
                      type="password"
                      value={apiKey}
                      onChange={(e) => setApiKey(e.target.value)}
                      onKeyDown={(e) => { if (e.key === 'Enter') handleKeySubmit(); }}
                      placeholder="sk_..."
                      disabled={keyStep !== 'input' && keyStep !== 'error'}
                      className="w-full px-4 py-3 rounded-lg bg-slate-700/50 border border-slate-600 text-white placeholder-slate-500 focus:outline-none focus:border-stark-500/50 font-mono text-sm disabled:opacity-50"
                      autoFocus
                    />
                  </div>

                  <ErrorBanner error={error} />

                  <button
                    onClick={handleKeySubmit}
                    disabled={!apiKey.trim() || (keyStep !== 'input' && keyStep !== 'error')}
                    className="w-full flex items-center justify-center gap-2 px-4 py-3 rounded-lg bg-stark-500 hover:bg-stark-600 text-white font-medium transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
                  >
                    {keyStep === 'saving' || keyStep === 'initializing' ? (
                      <>
                        <Loader2 className="w-4 h-4 animate-spin" />
                        {keyStep === 'saving' ? 'Saving...' : 'Connecting...'}
                      </>
                    ) : (
                      <>
                        Connect
                        <ArrowRight className="w-4 h-4" />
                      </>
                    )}
                  </button>
                </div>

                <p className="mt-6 text-xs text-slate-500 text-center">
                  Get your API key at{' '}
                  <a href="https://starflask.com" target="_blank" rel="noopener noreferrer" className="text-stark-400 hover:underline">
                    starflask.com
                  </a>
                </p>
              </>
            )}
          </div>
        )}

        {/* Step 2: Deploy Agents */}
        {phase === 'deploy' && (
          <div className="bg-slate-800 rounded-xl border border-slate-700 p-8">
            <div className="flex items-center gap-3 mb-6">
              <div className="p-3 rounded-lg bg-stark-500/20">
                <Rocket className="w-6 h-6 text-stark-400" />
              </div>
              <div>
                <h2 className="text-lg font-semibold text-white">Deploy Agents</h2>
                <p className="text-sm text-slate-400">
                  Provision AI agents to your Starflask account
                </p>
              </div>
            </div>

            {deployStep === 'done' ? (
              <div className="space-y-4">
                <div className="flex items-center gap-3 p-4 rounded-lg bg-green-500/10 border border-green-500/30">
                  <CheckCircle className="w-5 h-5 text-green-400" />
                  <span className="text-green-300">
                    {agents.length} agent{agents.length !== 1 ? 's' : ''} deployed! Launching dashboard...
                  </span>
                </div>
                {agents.length > 0 && (
                  <div className="space-y-2">
                    {agents.map((agent) => (
                      <div key={agent.capability} className="flex items-center gap-3 p-3 rounded-lg bg-slate-700/50">
                        <Bot className="w-4 h-4 text-stark-400" />
                        <div className="flex-1 min-w-0">
                          <div className="text-sm text-white font-medium">{agent.name}</div>
                          <div className="text-xs text-slate-400">{agent.capability}</div>
                        </div>
                        <CheckCircle className="w-4 h-4 text-green-400" />
                      </div>
                    ))}
                  </div>
                )}
              </div>
            ) : (
              <div className="space-y-4">
                <p className="text-sm text-slate-300">
                  This will sync your existing Starflask agents and deploy any new agents
                  from the Starkbot agent pack configuration.
                </p>

                <ErrorBanner error={error} />

                <button
                  onClick={handleDeploy}
                  disabled={deployStep === 'deploying'}
                  className="w-full flex items-center justify-center gap-2 px-4 py-3 rounded-lg bg-stark-500 hover:bg-stark-600 text-white font-medium transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
                >
                  {deployStep === 'deploying' ? (
                    <>
                      <Loader2 className="w-4 h-4 animate-spin" />
                      Deploying agents...
                    </>
                  ) : (
                    <>
                      <Rocket className="w-4 h-4" />
                      Deploy Agents
                    </>
                  )}
                </button>
              </div>
            )}
          </div>
        )}
      </div>
    </div>
  );
}

function StepDot({ active, done, label }: { active: boolean; done: boolean; label: string }) {
  return (
    <div className={`
      w-8 h-8 rounded-full flex items-center justify-center text-sm font-medium transition-colors
      ${done ? 'bg-green-500/20 text-green-400 border border-green-500/30' :
        active ? 'bg-stark-500/20 text-stark-400 border border-stark-500/30' :
        'bg-slate-700/50 text-slate-500 border border-slate-600'}
    `}>
      {done ? <CheckCircle className="w-4 h-4" /> : label}
    </div>
  );
}

function ErrorBanner({ error }: { error: string }) {
  if (!error) return null;
  return (
    <div className="flex items-center gap-2 p-3 rounded-lg bg-red-500/10 border border-red-500/30">
      <AlertCircle className="w-4 h-4 text-red-400 flex-shrink-0" />
      <span className="text-red-300 text-sm">{error}</span>
    </div>
  );
}
