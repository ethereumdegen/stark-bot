import { useState, useCallback } from 'react';
import { Send, Clock, ChevronDown, ChevronUp } from 'lucide-react';
import Card, { CardContent } from '@/components/ui/Card';
import { apiFetch } from '@/lib/api';
import { useApi } from '@/hooks/useApi';

interface CommandLog {
  id: number;
  capability: string;
  session_id?: string;
  message: string;
  status: string;
  result?: unknown;
  created_at: string;
  updated_at: string;
}

interface CommandOutput {
  type: string;
  results?: unknown[];
  urls?: string[];
  media_type?: string;
  post_url?: string;
  confirmation?: string;
  text?: string;
  data?: unknown;
}

const CAPABILITIES = [
  { value: '', label: 'Auto-detect' },
  { value: 'crypto', label: 'Crypto' },
  { value: 'image_gen', label: 'Image Generation' },
  { value: 'video_gen', label: 'Video Generation' },
  { value: 'social_media', label: 'Social Media' },
  { value: 'general', label: 'General' },
];

export default function CommandCenter() {
  const [message, setMessage] = useState('');
  const [capability, setCapability] = useState('');
  const [hook, setHook] = useState('');
  const [sending, setSending] = useState(false);
  const [lastResult, setLastResult] = useState<CommandOutput | null>(null);
  const [lastError, setLastError] = useState<string | null>(null);
  const [expandedId, setExpandedId] = useState<number | null>(null);

  const { data: commands, refetch: refetchCommands } = useApi<CommandLog[]>('/starflask/commands?limit=50');

  const handleSend = useCallback(async () => {
    if (!message.trim()) return;
    setSending(true);
    setLastResult(null);
    setLastError(null);

    try {
      const body: Record<string, unknown> = { message };
      if (capability) body.capability = capability;
      if (hook) body.hook = hook;

      const result = await apiFetch<CommandOutput>('/starflask/command', {
        method: 'POST',
        body: JSON.stringify(body),
      });
      setLastResult(result);
      setMessage('');
      refetchCommands();
    } catch (e) {
      setLastError(e instanceof Error ? e.message : 'Command failed');
    } finally {
      setSending(false);
    }
  }, [message, capability, hook, refetchCommands]);

  const capabilityColor: Record<string, string> = {
    crypto: 'bg-amber-500/20 text-amber-400',
    image_gen: 'bg-pink-500/20 text-pink-400',
    video_gen: 'bg-purple-500/20 text-purple-400',
    social_media: 'bg-blue-500/20 text-blue-400',
    general: 'bg-green-500/20 text-green-400',
  };

  const statusColor = (status: string) => {
    if (status === 'completed') return 'text-green-400';
    if (status === 'failed') return 'text-red-400';
    return 'text-amber-400';
  };

  return (
    <div className="p-8">
      <div className="mb-8">
        <h1 className="text-2xl font-bold text-white mb-1">Command Center</h1>
        <p className="text-slate-400 text-sm">Send commands to your Starflask agents</p>
      </div>

      {/* Command Input */}
      <Card>
        <CardContent>
          <div className="space-y-4">
            <div>
              <textarea
                value={message}
                onChange={(e) => setMessage(e.target.value)}
                onKeyDown={(e) => { if (e.key === 'Enter' && !e.shiftKey) { e.preventDefault(); handleSend(); } }}
                placeholder="Enter a command... (e.g. 'what is my wallet address', 'generate an image of a sunset')"
                className="w-full h-24 px-4 py-3 rounded-lg bg-slate-700/50 border border-slate-600 text-white placeholder-slate-500 focus:outline-none focus:border-stark-500/50 resize-none text-sm"
              />
            </div>
            <div className="flex items-center gap-3 flex-wrap">
              <select
                value={capability}
                onChange={(e) => setCapability(e.target.value)}
                className="px-3 py-2 rounded-lg bg-slate-700 border border-slate-600 text-slate-300 text-sm focus:outline-none focus:border-stark-500/50"
              >
                {CAPABILITIES.map(c => (
                  <option key={c.value} value={c.value}>{c.label}</option>
                ))}
              </select>
              <input
                type="text"
                value={hook}
                onChange={(e) => setHook(e.target.value)}
                placeholder="Hook (optional)"
                className="px-3 py-2 rounded-lg bg-slate-700 border border-slate-600 text-slate-300 text-sm placeholder-slate-500 focus:outline-none focus:border-stark-500/50 w-40"
              />
              <div className="flex-1" />
              <button
                onClick={handleSend}
                disabled={sending || !message.trim()}
                className="flex items-center gap-2 px-5 py-2 rounded-lg bg-stark-500 hover:bg-stark-600 text-white text-sm font-medium transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
              >
                <Send className="w-4 h-4" />
                {sending ? 'Sending...' : 'Send Command'}
              </button>
            </div>
          </div>
        </CardContent>
      </Card>

      {/* Last Result */}
      {lastResult && (
        <div className="mt-4">
          <Card>
            <CardContent>
              <h3 className="text-sm font-medium text-slate-300 mb-2">Result</h3>
              {lastResult.type === 'TextResponse' && (
                <p className="text-white text-sm whitespace-pre-wrap">{lastResult.text}</p>
              )}
              {lastResult.type === 'MediaGeneration' && lastResult.urls && (
                <div className="space-y-2">
                  <p className="text-slate-400 text-xs">{lastResult.media_type} generated:</p>
                  {lastResult.urls.map((url, i) => (
                    <a key={i} href={url} target="_blank" rel="noopener noreferrer" className="text-stark-400 text-sm hover:underline block truncate">{url}</a>
                  ))}
                </div>
              )}
              {lastResult.type === 'SocialPost' && (
                <div>
                  <p className="text-white text-sm">{lastResult.confirmation}</p>
                  {lastResult.post_url && (
                    <a href={lastResult.post_url} target="_blank" rel="noopener noreferrer" className="text-stark-400 text-sm hover:underline mt-1 block">{lastResult.post_url}</a>
                  )}
                </div>
              )}
              {(lastResult.type === 'CryptoExecution' || lastResult.type === 'Raw') && (
                <pre className="text-xs text-slate-400 overflow-auto max-h-64 whitespace-pre-wrap">
                  {JSON.stringify(lastResult, null, 2)}
                </pre>
              )}
            </CardContent>
          </Card>
        </div>
      )}

      {lastError && (
        <div className="mt-4 p-4 rounded-lg bg-red-500/10 border border-red-500/30 text-red-300 text-sm">
          {lastError}
        </div>
      )}

      {/* Command History */}
      <div className="mt-8">
        <h2 className="text-lg font-semibold text-white mb-4 flex items-center gap-2">
          <Clock className="w-5 h-5 text-slate-400" />
          Command History
        </h2>
        <div className="space-y-2">
          {(!commands || commands.length === 0) ? (
            <p className="text-slate-500 text-sm text-center py-8">No commands sent yet</p>
          ) : (
            commands.map((cmd) => (
              <div key={cmd.id} className="rounded-lg bg-slate-800/50 border border-slate-700">
                <button
                  onClick={() => setExpandedId(expandedId === cmd.id ? null : cmd.id)}
                  className="w-full flex items-center justify-between p-3 text-left"
                >
                  <div className="flex items-center gap-3 min-w-0 flex-1">
                    <span className={`text-xs px-2 py-0.5 rounded-full font-medium ${capabilityColor[cmd.capability] || 'bg-slate-500/20 text-slate-400'}`}>
                      {cmd.capability}
                    </span>
                    <span className="text-sm text-slate-300 truncate">{cmd.message}</span>
                  </div>
                  <div className="flex items-center gap-3 flex-shrink-0 ml-3">
                    <span className={`text-xs ${statusColor(cmd.status)}`}>{cmd.status}</span>
                    <span className="text-xs text-slate-600">{new Date(cmd.created_at).toLocaleString()}</span>
                    {expandedId === cmd.id ? <ChevronUp className="w-4 h-4 text-slate-500" /> : <ChevronDown className="w-4 h-4 text-slate-500" />}
                  </div>
                </button>
                {expandedId === cmd.id && cmd.result != null ? (
                  <div className="px-3 pb-3 border-t border-slate-700/50">
                    <pre className="text-xs text-slate-400 overflow-auto max-h-48 whitespace-pre-wrap mt-2">
                      {JSON.stringify(cmd.result, null, 2)}
                    </pre>
                  </div>
                ) : null}
              </div>
            ))
          )}
        </div>
      </div>
    </div>
  );
}
