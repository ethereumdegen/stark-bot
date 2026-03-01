import { useState, useEffect, useRef, useCallback } from 'react';
import { Wifi, WifiOff, Server, HardDrive, ScrollText, Trash2, ChevronDown, ChevronRight, AlertTriangle, Clock, Anchor } from 'lucide-react';
import Card, { CardContent, CardHeader, CardTitle } from '@/components/ui/Card';
import Button from '@/components/ui/Button';
import { useGateway } from '@/hooks/useGateway';
import { useApi } from '@/hooks/useApi';
import clsx from 'clsx';

interface SystemInfo {
  disk: {
    enabled: boolean;
    used_bytes: number;
    quota_bytes: number;
    remaining_bytes: number;
    percentage: number;
    breakdown: Record<string, number>;
  };
  uptime_secs: number;
  version: string;
}

// ── Live Logs types & helpers ───────────────────────────────────────

interface LogEntry {
  id: string;
  event: string;
  data: Record<string, unknown>;
  timestamp: Date;
  replayed?: boolean;
}

const categoryConfig: Record<string, { color: string; label: string }> = {
  'channel': { color: 'text-blue-400 bg-blue-500/20 border-blue-500/30', label: 'Channel' },
  'agent': { color: 'text-emerald-400 bg-emerald-500/20 border-emerald-500/30', label: 'Agent' },
  'tool': { color: 'text-purple-400 bg-purple-500/20 border-purple-500/30', label: 'Tool' },
  'skill': { color: 'text-pink-400 bg-pink-500/20 border-pink-500/30', label: 'Skill' },
  'execution': { color: 'text-cyan-400 bg-cyan-500/20 border-cyan-500/30', label: 'Execution' },
  'tx': { color: 'text-amber-400 bg-amber-500/20 border-amber-500/30', label: 'Transaction' },
  'stream': { color: 'text-indigo-400 bg-indigo-500/20 border-indigo-500/30', label: 'Stream' },
  'task': { color: 'text-teal-400 bg-teal-500/20 border-teal-500/30', label: 'Task' },
  'cron': { color: 'text-orange-400 bg-orange-500/20 border-orange-500/30', label: 'Cron' },
  'ai': { color: 'text-yellow-400 bg-yellow-500/20 border-yellow-500/30', label: 'AI' },
  'process': { color: 'text-lime-400 bg-lime-500/20 border-lime-500/30', label: 'Process' },
  'exec': { color: 'text-lime-400 bg-lime-500/20 border-lime-500/30', label: 'Exec' },
  'confirmation': { color: 'text-amber-400 bg-amber-500/20 border-amber-500/30', label: 'Confirm' },
  'telemetry': { color: 'text-slate-400 bg-slate-500/20 border-slate-500/30', label: 'Telemetry' },
  'hook': { color: 'text-fuchsia-400 bg-fuchsia-500/20 border-fuchsia-500/30', label: 'Hook' },
  'system': { color: 'text-sky-400 bg-sky-500/20 border-sky-500/30', label: 'System' },
};

const NOISE_EVENTS = new Set([
  'stream.content_delta',
  'stream.tool_delta',
  'stream.thinking_delta',
  'agent.context_update',
]);

function getCategory(event: string): string {
  return event.split('.')[0] || 'unknown';
}

function getCategoryColor(event: string): string {
  if (event.includes('error')) return 'text-red-400 bg-red-500/20 border-red-500/30';
  if (event.includes('warning')) return 'text-yellow-400 bg-yellow-500/20 border-yellow-500/30';
  const cat = getCategory(event);
  return categoryConfig[cat]?.color || 'text-slate-400 bg-slate-500/20 border-slate-500/30';
}

function summarize(event: string, data: Record<string, unknown>): string {
  try {
    switch (event) {
      case 'channel.message':
        return `[${data.channel_type || '?'}] ${data.from || '?'}: ${truncate(String(data.text || ''), 120)}`;
      case 'channel.started':
        return `${data.channel_type || '?'} "${data.name || ''}" started`;
      case 'channel.stopped':
        return `${data.channel_type || '?'} "${data.name || ''}" stopped`;
      case 'channel.error':
        return `${data.channel_id || '?'}: ${truncate(String(data.error || data.message || ''), 200)}`;
      case 'agent.response':
        return `→ ${data.to || '?'}: ${truncate(String(data.text || ''), 120)}`;
      case 'agent.tool_call':
        return `${data.tool_name || '?'}(${formatParams(data.parameters)})`;
      case 'agent.mode_change':
        return `${data.label || data.mode || '?'} — ${data.reason || ''}`;
      case 'agent.subtype_change':
        return `→ ${data.label || data.subtype || '?'}`;
      case 'agent.thinking':
        return truncate(String(data.message || ''), 120);
      case 'agent.error':
        return truncate(String(data.error || ''), 200);
      case 'agent.warning':
        return `[${data.warning_type || 'warn'}] ${truncate(String(data.message || ''), 160)}`;
      case 'tool.execution':
        return `${data.tool_name || '?'}(${formatParams(data.parameters)})`;
      case 'tool.result': {
        const ok = data.success ? 'OK' : 'FAIL';
        const dur = data.duration_ms != null ? ` ${data.duration_ms}ms` : '';
        const content = data.content ? ` — ${truncate(String(data.content), 120)}` : '';
        return `${data.tool_name || '?'} ${ok}${dur}${content}`;
      }
      case 'tool.waiting':
        return `${data.tool_name || '?'} retrying in ${data.wait_seconds || '?'}s`;
      case 'skill.invoked':
        return String(data.skill_name || '?');
      case 'execution.started':
        return `${data.mode || ''}: ${data.description || data.active_form || ''}`;
      case 'execution.completed':
        return `Done — ${JSON.stringify(data.total_metrics || {})}`;
      case 'ai.retrying':
        return `Attempt ${data.attempt}/${data.max_attempts} (${data.provider || '?'}) — ${truncate(String(data.error || ''), 100)}`;
      case 'tx.pending':
        return `${data.network || '?'} tx ${truncate(String(data.tx_hash || ''), 20)}`;
      case 'tx.confirmed':
        return `${data.network || '?'} tx ${truncate(String(data.tx_hash || ''), 20)} — ${data.status || '?'}`;
      case 'cron.execution_started_on_channel':
        return `Job "${data.job_name || '?'}" on ${data.channel_id || '?'}`;
      case 'cron.execution_stopped_on_channel':
        return `Job stopped: ${data.reason || '?'}`;
      case 'stream.start':
        return `Session ${truncate(String(data.session_id || ''), 12)}`;
      case 'stream.end': {
        const usage = data.usage as Record<string, unknown> | undefined;
        if (usage) return `${data.stop_reason || 'done'} — ${usage.input_tokens || 0}→${usage.output_tokens || 0} tokens`;
        return String(data.stop_reason || 'done');
      }
      case 'stream.error':
        return `${data.code || ''} ${truncate(String(data.error || ''), 160)}`;
      case 'task.queue_update':
        return `${(data.tasks as unknown[])?.length || 0} tasks, current: ${data.current_task_id || 'none'}`;
      case 'task.status_change':
        return `Task ${data.task_id || '?'}: ${data.status || '?'} — ${truncate(String(data.description || ''), 100)}`;
      case 'hook.fired':
        return `${data.agent_key || '?'} → ${data.event || '?'}`;
      case 'hook.completed': {
        const s = String(data.status || '?');
        const err = data.error ? ` — ${truncate(String(data.error), 100)}` : '';
        return `${data.agent_key || '?'} → ${data.event || '?'} [${s}]${err}`;
      }
      case 'system.keystore_started':
      case 'system.keystore_success':
      case 'system.keystore_error':
      case 'system.keystore_no_backup':
      case 'system.keystore_skipped':
      case 'system.keystore_server_error':
        return truncate(String(data.message || ''), 200);
      case 'system.boot':
        return truncate(String(data.message || ''), 200);
      default:
        return genericSummary(data);
    }
  } catch {
    return genericSummary(data);
  }
}

function genericSummary(data: Record<string, unknown>): string {
  const skip = new Set(['timestamp', 'channel_id', 'session_id', 'chat_id']);
  const parts: string[] = [];
  for (const [k, v] of Object.entries(data)) {
    if (skip.has(k)) continue;
    if (v == null || v === '') continue;
    if (typeof v === 'object') continue;
    parts.push(`${k}=${truncate(String(v), 60)}`);
    if (parts.length >= 4) break;
  }
  return parts.join(', ');
}

function formatParams(params: unknown): string {
  if (!params || typeof params !== 'object') return '';
  const obj = params as Record<string, unknown>;
  const parts: string[] = [];
  for (const [k, v] of Object.entries(obj)) {
    if (v == null) continue;
    const val = typeof v === 'string' ? truncate(v, 30) : JSON.stringify(v);
    parts.push(`${k}: ${val}`);
    if (parts.length >= 3) break;
  }
  return parts.join(', ');
}

function truncate(s: string, max: number): string {
  if (s.length <= max) return s;
  return s.slice(0, max) + '…';
}

function formatTime(d: Date): string {
  return d.toLocaleTimeString('en-US', {
    hour12: false,
    hour: '2-digit',
    minute: '2-digit',
    second: '2-digit',
  });
}

const MAX_LOGS = 500;

// ── Debug Page ──────────────────────────────────────────────────────

export default function Debug() {
  const { connected, gateway, connect, disconnect } = useGateway();
  const { data: sysInfo } = useApi<SystemInfo>('/system/info');

  // Live logs state
  const [logs, setLogs] = useState<LogEntry[]>([]);
  const [filter, setFilter] = useState<string>('all');
  const [autoScroll, setAutoScroll] = useState(true);
  const [showNoise, setShowNoise] = useState(false);
  const [expandedIds, setExpandedIds] = useState<Set<string>>(new Set());
  const logContainerRef = useRef<HTMLDivElement>(null);

  const handleEvent = useCallback((payload: unknown) => {
    const { event, data } = payload as { event: string; data: Record<string, unknown> };
    const newLog: LogEntry = {
      id: crypto.randomUUID(),
      event,
      data: data || {},
      timestamp: data?.timestamp ? new Date(data.timestamp as string) : new Date(),
    };
    setLogs((prev) => [...prev.slice(-(MAX_LOGS - 1)), newLog]);
  }, []);

  useEffect(() => {
    gateway.on('*', handleEvent);
    return () => {
      gateway.off('*', handleEvent);
    };
  }, [gateway, handleEvent]);

  useEffect(() => {
    if (autoScroll && logContainerRef.current) {
      logContainerRef.current.scrollTop = logContainerRef.current.scrollHeight;
    }
  }, [logs, autoScroll]);

  const clearLogs = () => {
    setLogs([]);
    setExpandedIds(new Set());
  };

  const toggleExpand = (id: string) => {
    setExpandedIds(prev => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  };

  const isError = (event: string) => event.includes('error');
  const isWarning = (event: string) => event.includes('warning') || event === 'ai.retrying';

  const filteredLogs = logs.filter(log => {
    if (!showNoise && NOISE_EVENTS.has(log.event)) return false;
    if (filter === 'all') return true;
    if (filter === 'error') return isError(log.event) || isWarning(log.event);
    return log.event.startsWith(filter + '.');
  });

  const hookLogs = logs.filter(l => l.event.startsWith('hook.'));

  const filters = ['all', 'channel', 'agent', 'tool', 'execution', 'system', 'error'];
  const errorCount = logs.filter(l => isError(l.event)).length;

  const formatUptime = (seconds?: number) => {
    if (!seconds) return 'N/A';
    const days = Math.floor(seconds / 86400);
    const hours = Math.floor((seconds % 86400) / 3600);
    const minutes = Math.floor((seconds % 3600) / 60);
    if (days > 0) return `${days}d ${hours}h ${minutes}m`;
    if (hours > 0) return `${hours}h ${minutes}m`;
    return `${minutes}m`;
  };

  const formatBytes = (bytes?: number) => {
    if (bytes === undefined || bytes === null) return 'N/A';
    if (bytes === 0) return '0 B';
    const units = ['B', 'KB', 'MB', 'GB'];
    let value = bytes;
    let unitIndex = 0;
    while (value >= 1024 && unitIndex < units.length - 1) {
      value /= 1024;
      unitIndex++;
    }
    return `${value.toFixed(1)} ${units[unitIndex]}`;
  };

  const diskPct = sysInfo?.disk.percentage ?? 0;

  return (
    <div className="p-8">
      <div className="mb-8">
        <h1 className="text-2xl font-bold text-white mb-2">Debug</h1>
        <p className="text-slate-400">System diagnostics, debugging tools, and live event logs</p>
      </div>

      <div className="grid gap-6 lg:grid-cols-2">
        {/* Gateway Status */}
        <Card>
          <CardHeader>
            <CardTitle className="flex items-center gap-2">
              {connected ? (
                <Wifi className="w-5 h-5 text-green-400" />
              ) : (
                <WifiOff className="w-5 h-5 text-red-400" />
              )}
              Gateway Connection
            </CardTitle>
          </CardHeader>
          <CardContent>
            <div className="space-y-4">
              <div className="flex items-center justify-between p-3 rounded-lg bg-slate-700/50">
                <span className="text-slate-300">Status</span>
                <span
                  className={`flex items-center gap-2 ${
                    connected ? 'text-green-400' : 'text-red-400'
                  }`}
                >
                  <span
                    className={`w-2 h-2 rounded-full ${
                      connected ? 'bg-green-400' : 'bg-red-400'
                    }`}
                  />
                  {connected ? 'Connected' : 'Disconnected'}
                </span>
              </div>
              <Button
                variant={connected ? 'danger' : 'primary'}
                onClick={connected ? disconnect : connect}
                className="w-full"
              >
                {connected ? 'Disconnect' : 'Connect'}
              </Button>
            </div>
          </CardContent>
        </Card>

        {/* System Info */}
        <Card>
          <CardHeader>
            <CardTitle className="flex items-center gap-2">
              <Server className="w-5 h-5 text-blue-400" />
              System Information
            </CardTitle>
          </CardHeader>
          <CardContent>
            <div className="space-y-3">
              <div className="flex items-center justify-between p-3 rounded-lg bg-slate-700/50">
                <span className="text-slate-300">Version</span>
                <span className="text-white font-mono">
                  {sysInfo?.version ?? '...'}
                </span>
              </div>
              <div className="flex items-center justify-between p-3 rounded-lg bg-slate-700/50">
                <span className="text-slate-300">Uptime</span>
                <span className="text-white">
                  {formatUptime(sysInfo?.uptime_secs)}
                </span>
              </div>
              <div className="flex items-center justify-between p-3 rounded-lg bg-slate-700/50">
                <span className="text-slate-300 flex items-center gap-2">
                  <HardDrive className="w-4 h-4" />
                  Disk Quota
                </span>
                <div className="flex items-center gap-3">
                  <span className="text-white text-sm">
                    {formatBytes(sysInfo?.disk.used_bytes)} / {formatBytes(sysInfo?.disk.quota_bytes)}
                  </span>
                  <div className="w-20 h-2 bg-slate-600 rounded-full overflow-hidden">
                    <div
                      className={`h-full rounded-full transition-all ${
                        diskPct >= 90 ? 'bg-red-500' : diskPct >= 70 ? 'bg-amber-500' : 'bg-blue-500'
                      }`}
                      style={{ width: `${Math.min(diskPct, 100)}%` }}
                    />
                  </div>
                  <span className="text-slate-400 text-xs w-8 text-right">{diskPct}%</span>
                </div>
              </div>
            </div>
          </CardContent>
        </Card>
      </div>

      {/* ── Live Logs ────────────────────────────────────────────── */}
      <div className="mt-8">
        <div className="flex items-center justify-between mb-4">
          <div className="flex items-center gap-3">
            <h2 className="text-lg font-semibold text-white flex items-center gap-2">
              <ScrollText className="w-5 h-5 text-amber-400" />
              Live Logs
            </h2>
            <span className="text-sm text-slate-500">Recent history replayed on connect</span>
            {errorCount > 0 && (
              <div className="flex items-center gap-1.5 px-2.5 py-1 rounded-full bg-red-500/20">
                <AlertTriangle className="w-3.5 h-3.5 text-red-400" />
                <span className="text-xs text-red-400 font-medium">{errorCount} error{errorCount !== 1 ? 's' : ''}</span>
              </div>
            )}
          </div>
          <div className="flex items-center gap-2">
            <Button variant="secondary" size="sm" onClick={() => setAutoScroll(!autoScroll)}>
              Auto-scroll: {autoScroll ? 'ON' : 'OFF'}
            </Button>
            <Button variant="secondary" size="sm" onClick={() => setShowNoise(!showNoise)}>
              Verbose: {showNoise ? 'ON' : 'OFF'}
            </Button>
            <Button variant="ghost" size="sm" onClick={clearLogs}>
              <Trash2 className="w-4 h-4 mr-2" />
              Clear
            </Button>
          </div>
        </div>

        {/* Filters */}
        <div className="flex items-center gap-2 mb-3">
          <span className="text-sm text-slate-400">Filter:</span>
          {filters.map((f) => (
            <button
              key={f}
              onClick={() => setFilter(f)}
              className={clsx(
                'px-3 py-1 rounded-full text-sm transition-colors',
                filter === f
                  ? f === 'error'
                    ? 'bg-red-500/30 text-red-300'
                    : 'bg-stark-500 text-white'
                  : 'bg-slate-700 text-slate-300 hover:bg-slate-600'
              )}
            >
              {f.charAt(0).toUpperCase() + f.slice(1)}
              {f === 'error' && errorCount > 0 && (
                <span className="ml-1.5 text-xs bg-red-500/40 px-1.5 py-0.5 rounded-full">{errorCount}</span>
              )}
            </button>
          ))}
        </div>

        <Card>
          <CardContent className="p-0">
            {filteredLogs.length > 0 ? (
              <div
                ref={logContainerRef}
                className="max-h-[60vh] overflow-y-auto font-mono text-sm"
              >
                {filteredLogs.map((log) => {
                  const expanded = expandedIds.has(log.id);
                  const error = isError(log.event);
                  const warning = isWarning(log.event);
                  return (
                    <div
                      key={log.id}
                      className={clsx(
                        'px-3 py-2 border-b border-slate-700/50 hover:bg-slate-700/30 transition-colors cursor-pointer',
                        error && 'bg-red-500/5 border-l-2 border-l-red-500',
                        warning && !error && 'bg-yellow-500/5 border-l-2 border-l-yellow-500/50',
                      )}
                      onClick={() => toggleExpand(log.id)}
                    >
                      <div className="flex items-center gap-2">
                        <span className="text-slate-600 w-4 flex-shrink-0">
                          {expanded
                            ? <ChevronDown className="w-3.5 h-3.5" />
                            : <ChevronRight className="w-3.5 h-3.5" />
                          }
                        </span>
                        <span className="text-slate-500 text-xs whitespace-nowrap flex items-center gap-1">
                          {log.replayed ? <Clock className="w-3 h-3 text-slate-600" /> : null}
                          {formatTime(log.timestamp)}
                        </span>
                        <span
                          className={clsx(
                            'px-2 py-0.5 text-xs font-medium rounded whitespace-nowrap border',
                            getCategoryColor(log.event)
                          )}
                        >
                          {log.event}
                        </span>
                        {log.data.channel_id != null && (
                          <span className="text-slate-600 text-xs">
                            ch:{truncate(String(log.data.channel_id), 8)}
                          </span>
                        )}
                        <span className={clsx(
                          'text-xs truncate',
                          error ? 'text-red-300' : warning ? 'text-yellow-300' : 'text-slate-300'
                        )}>
                          {summarize(log.event, log.data)}
                        </span>
                      </div>
                      {expanded && (
                        <pre
                          className="text-slate-400 text-xs whitespace-pre-wrap break-all mt-2 ml-6 p-3 bg-slate-800/50 rounded border border-slate-700/50"
                          style={{ wordBreak: 'break-word', overflowWrap: 'anywhere' }}
                          onClick={(e) => e.stopPropagation()}
                        >
                          {JSON.stringify(log.data, null, 2)}
                        </pre>
                      )}
                    </div>
                  );
                })}
              </div>
            ) : (
              <div className="flex flex-col items-center justify-center h-48">
                <ScrollText className="w-10 h-10 text-slate-600 mb-3" />
                <p className="text-slate-400 text-sm">
                  {connected
                    ? 'Waiting for events... Events will appear here in real-time.'
                    : 'Not connected to Gateway. Events will appear when connected.'
                  }
                </p>
              </div>
            )}
          </CardContent>
        </Card>

        {/* Stats Footer */}
        <div className="mt-3 flex items-center justify-between text-sm text-slate-400">
          <div className="flex items-center gap-6">
            <span>Total: <span className="text-white font-medium">{filteredLogs.length}</span>{filter !== 'all' && <span className="text-slate-500">/{logs.length}</span>}</span>
            <span>Channels: <span className="text-blue-400 font-medium">{logs.filter(l => l.event.startsWith('channel.')).length}</span></span>
            <span>Agent: <span className="text-emerald-400 font-medium">{logs.filter(l => l.event.startsWith('agent.')).length}</span></span>
            <span>Tools: <span className="text-purple-400 font-medium">{logs.filter(l => l.event.startsWith('tool.')).length}</span></span>
            <span>Errors: <span className="text-red-400 font-medium">{errorCount}</span></span>
          </div>
          {logs.length > 0 && (
            <span className="text-slate-500">
              Last event: {logs[logs.length - 1]?.timestamp.toLocaleTimeString()}
            </span>
          )}
        </div>
      </div>

      {/* ── Hook Events ──────────────────────────────────────────── */}
      <div className="mt-8">
        <div className="flex items-center justify-between mb-4">
          <h2 className="text-lg font-semibold text-white flex items-center gap-2">
            <Anchor className="w-5 h-5 text-fuchsia-400" />
            Hook Events
            {hookLogs.length > 0 && (
              <span className="text-sm font-normal text-slate-500 ml-1">({hookLogs.length})</span>
            )}
          </h2>
        </div>

        <Card>
          <CardContent className="p-0">
            {hookLogs.length > 0 ? (
              <div className="max-h-[40vh] overflow-y-auto font-mono text-sm divide-y divide-slate-700/50">
                {hookLogs.map((log) => {
                  const isFired = log.event === 'hook.fired';
                  const status = String(log.data.status || '');
                  const isFailure = status === 'failed' || status === 'timeout';
                  return (
                    <div
                      key={log.id}
                      className={clsx(
                        'px-4 py-2.5 flex items-center gap-3',
                        isFailure && 'bg-red-500/5',
                      )}
                    >
                      {/* status indicator */}
                      <span className={clsx(
                        'w-2 h-2 rounded-full shrink-0',
                        isFired ? 'bg-fuchsia-400 animate-pulse' :
                        status === 'success' ? 'bg-emerald-400' :
                        status === 'timeout' ? 'bg-amber-400' :
                        'bg-red-400',
                      )} />

                      {/* timestamp */}
                      <span className="text-xs text-slate-500 whitespace-nowrap">
                        {formatTime(log.timestamp)}
                      </span>

                      {/* event badge */}
                      <span className={clsx(
                        'px-2 py-0.5 text-xs font-medium rounded border whitespace-nowrap',
                        isFired
                          ? 'text-fuchsia-400 bg-fuchsia-500/20 border-fuchsia-500/30'
                          : status === 'success'
                            ? 'text-emerald-400 bg-emerald-500/20 border-emerald-500/30'
                            : 'text-red-400 bg-red-500/20 border-red-500/30',
                      )}>
                        {isFired ? 'fired' : status}
                      </span>

                      {/* agent key */}
                      <span className="text-xs text-slate-300 font-medium">
                        {String(log.data.agent_key ?? '?')}
                      </span>

                      {/* arrow + event name */}
                      <span className="text-slate-600 text-xs">&rarr;</span>
                      <span className="text-xs text-slate-400">
                        {String(log.data.event ?? '?')}
                      </span>

                      {/* error message if present */}
                      {log.data.error != null && (
                        <span className="text-xs text-red-400 truncate ml-auto max-w-[40%]">
                          {truncate(String(log.data.error), 120)}
                        </span>
                      )}
                    </div>
                  );
                })}
              </div>
            ) : (
              <div className="flex flex-col items-center justify-center h-32">
                <Anchor className="w-8 h-8 text-slate-600 mb-2" />
                <p className="text-slate-500 text-sm">
                  {connected
                    ? 'No hook events yet. Events appear when persona hooks fire.'
                    : 'Connect to Gateway to see hook events.'
                  }
                </p>
              </div>
            )}
          </CardContent>
        </Card>
      </div>
    </div>
  );
}
