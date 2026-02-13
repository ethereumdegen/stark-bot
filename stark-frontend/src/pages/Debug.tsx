import { useState, useEffect } from 'react';
import { Bug, Wifi, WifiOff, Server, HardDrive } from 'lucide-react';
import Card, { CardContent, CardHeader, CardTitle } from '@/components/ui/Card';
import Button from '@/components/ui/Button';
import { useGateway } from '@/hooks/useGateway';
import { useApi } from '@/hooks/useApi';

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

export default function Debug() {
  const { connected, gateway, connect, disconnect } = useGateway();
  const [events, setEvents] = useState<Array<{ event: string; data: unknown; time: Date }>>([]);
  const { data: sysInfo } = useApi<SystemInfo>('/system/info');

  useEffect(() => {
    const handleEvent = (payload: unknown) => {
      const { event, data } = payload as { event: string; data: unknown };
      setEvents((prev) => [
        { event, data, time: new Date() },
        ...prev.slice(0, 99), // Keep last 100 events
      ]);
    };

    gateway.on('*', handleEvent);

    return () => {
      gateway.off('*', handleEvent);
    };
  }, [gateway]);

  const clearEvents = () => {
    setEvents([]);
  };

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
        <p className="text-slate-400">System diagnostics and debugging tools</p>
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

        {/* Event Log */}
        <Card className="lg:col-span-2">
          <CardHeader>
            <div className="flex items-center justify-between">
              <CardTitle className="flex items-center gap-2">
                <Bug className="w-5 h-5 text-amber-400" />
                Gateway Events
              </CardTitle>
              <Button variant="ghost" size="sm" onClick={clearEvents}>
                Clear
              </Button>
            </div>
          </CardHeader>
          <CardContent>
            {events.length > 0 ? (
              <div className="space-y-2 max-h-96 overflow-y-auto font-mono text-sm">
                {events.map((event, index) => (
                  <div
                    key={index}
                    className="p-3 rounded-lg bg-slate-700/50 hover:bg-slate-700 transition-colors"
                  >
                    <div className="flex items-center justify-between mb-1">
                      <span className="text-stark-400">{event.event}</span>
                      <span className="text-xs text-slate-500">
                        {event.time.toLocaleTimeString()}
                      </span>
                    </div>
                    <pre className="text-xs text-slate-400 whitespace-pre-wrap break-all" style={{ wordBreak: 'break-word', overflowWrap: 'anywhere' }}>
                      {JSON.stringify(event.data, null, 2)}
                    </pre>
                  </div>
                ))}
              </div>
            ) : (
              <div className="text-center py-8">
                <Bug className="w-8 h-8 text-slate-600 mx-auto mb-2" />
                <p className="text-slate-400 text-sm">
                  No events captured yet. Events will appear here in real-time.
                </p>
              </div>
            )}
          </CardContent>
        </Card>
      </div>
    </div>
  );
}
