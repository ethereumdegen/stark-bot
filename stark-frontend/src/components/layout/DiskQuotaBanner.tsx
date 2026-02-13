import { useState, useEffect, useCallback } from 'react';
import { Link } from 'react-router-dom';
import { AlertTriangle, X } from 'lucide-react';
import { useGateway } from '@/hooks/useGateway';
import { useApi } from '@/hooks/useApi';

interface DiskQuotaWarning {
  percentage: number;
  used_bytes: number;
  quota_bytes: number;
  remaining_bytes: number;
  level: 'ok' | 'warning' | 'high' | 'critical';
  message: string;
}

interface SystemInfo {
  disk: {
    enabled: boolean;
    used_bytes: number;
    quota_bytes: number;
    remaining_bytes: number;
    percentage: number;
  };
}

const LEVEL_PRIORITY: Record<string, number> = {
  ok: 0,
  warning: 1,
  high: 2,
  critical: 3,
};

const LEVEL_STYLES: Record<string, string> = {
  warning: 'bg-amber-500/10 border-amber-500/30 text-amber-400',
  high: 'bg-orange-500/10 border-orange-500/30 text-orange-400',
  critical: 'bg-red-500/10 border-red-500/30 text-red-400',
};

const LEVEL_MESSAGES: Record<string, (pct: number) => string> = {
  warning: (pct) => `Storage is ${pct}% full. Consider cleaning up old files.`,
  high: (pct) => `Storage is ${pct}% full. Writes may start failing soon.`,
  critical: (pct) => `Storage is critically full (${pct}%). Clean up now to avoid write failures.`,
};

export default function DiskQuotaBanner() {
  const { on, off } = useGateway();
  const { data: sysInfo } = useApi<SystemInfo>('/system/info');
  const [warning, setWarning] = useState<DiskQuotaWarning | null>(null);
  const [dismissedLevel, setDismissedLevel] = useState<string | null>(null);

  // Seed initial state from /api/system/info
  useEffect(() => {
    if (!sysInfo?.disk?.enabled) return;
    const pct = sysInfo.disk.percentage;
    let level: DiskQuotaWarning['level'] = 'ok';
    if (pct >= 95) level = 'critical';
    else if (pct >= 85) level = 'high';
    else if (pct >= 70) level = 'warning';

    if (level === 'ok') return;
    setWarning({
      percentage: pct,
      used_bytes: sysInfo.disk.used_bytes,
      quota_bytes: sysInfo.disk.quota_bytes,
      remaining_bytes: sysInfo.disk.remaining_bytes,
      level,
      message: LEVEL_MESSAGES[level](pct),
    });
  }, [sysInfo]);

  // Listen for gateway events
  const handleEvent = useCallback((data: unknown) => {
    const event = data as DiskQuotaWarning;
    if (event.level === 'ok') {
      setWarning(null);
      setDismissedLevel(null);
      return;
    }
    setWarning(event);
    // If a higher threshold is crossed, clear the dismissed state
    if (dismissedLevel && LEVEL_PRIORITY[event.level] > LEVEL_PRIORITY[dismissedLevel]) {
      setDismissedLevel(null);
    }
  }, [dismissedLevel]);

  useEffect(() => {
    on('disk_quota.warning', handleEvent);
    return () => off('disk_quota.warning', handleEvent);
  }, [on, off, handleEvent]);

  // Don't show if no warning, level is ok, or user dismissed this level
  if (!warning || warning.level === 'ok') return null;
  if (dismissedLevel && LEVEL_PRIORITY[warning.level] <= LEVEL_PRIORITY[dismissedLevel]) return null;

  const styles = LEVEL_STYLES[warning.level] || LEVEL_STYLES.warning;

  return (
    <div className={`border-b px-4 py-2 flex items-center gap-3 text-sm ${styles}`}>
      <AlertTriangle className="w-4 h-4 flex-shrink-0" />
      <span className="flex-1">
        {warning.message}{' '}
        <Link to="/system" className="underline font-medium hover:opacity-80">
          Manage Storage
        </Link>
      </span>
      <button
        onClick={() => setDismissedLevel(warning.level)}
        className="p-1 rounded hover:bg-white/10 flex-shrink-0"
        aria-label="Dismiss"
      >
        <X className="w-4 h-4" />
      </button>
    </div>
  );
}
