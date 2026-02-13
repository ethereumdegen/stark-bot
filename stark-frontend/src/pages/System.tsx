import { useState } from 'react';
import { HardDrive, Trash2, FileText, FolderOpen } from 'lucide-react';
import Card, { CardContent, CardHeader, CardTitle } from '@/components/ui/Card';
import Button from '@/components/ui/Button';
import Modal from '@/components/ui/Modal';
import { useApi } from '@/hooks/useApi';
import { apiFetch } from '@/lib/api';

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

interface CleanupResult {
  success: boolean;
  deleted_count: number;
  freed_bytes: number;
  error?: string;
}

const CATEGORY_COLORS: Record<string, { bg: string; text: string; label: string }> = {
  workspace: { bg: 'bg-blue-500', text: 'text-blue-400', label: 'Workspace' },
  memory: { bg: 'bg-purple-500', text: 'text-purple-400', label: 'Memory' },
  journal: { bg: 'bg-green-500', text: 'text-green-400', label: 'Journal' },
  soul: { bg: 'bg-amber-500', text: 'text-amber-400', label: 'Soul' },
  database: { bg: 'bg-slate-400', text: 'text-slate-300', label: 'Database' },
};

const CATEGORY_ORDER = ['workspace', 'memory', 'journal', 'soul', 'database'];

function formatBytes(bytes: number): string {
  if (bytes === 0) return '0 B';
  const units = ['B', 'KB', 'MB', 'GB'];
  let value = bytes;
  let unitIndex = 0;
  while (value >= 1024 && unitIndex < units.length - 1) {
    value /= 1024;
    unitIndex++;
  }
  return `${value.toFixed(1)} ${units[unitIndex]}`;
}

export default function System() {
  const { data: info, isLoading, refetch } = useApi<SystemInfo>('/system/info');

  const [memoryDays, setMemoryDays] = useState(30);
  const [showMemoryModal, setShowMemoryModal] = useState(false);
  const [showWorkspaceModal, setShowWorkspaceModal] = useState(false);
  const [cleanupResult, setCleanupResult] = useState<CleanupResult | null>(null);
  const [cleaning, setCleaning] = useState(false);

  const handleCleanupMemories = async () => {
    setCleaning(true);
    setCleanupResult(null);
    try {
      const result = await apiFetch<CleanupResult>('/system/cleanup/memories', {
        method: 'POST',
        body: JSON.stringify({ older_than_days: memoryDays }),
      });
      setCleanupResult(result);
      refetch();
    } catch (e) {
      setCleanupResult({ success: false, deleted_count: 0, freed_bytes: 0, error: String(e) });
    } finally {
      setCleaning(false);
      setShowMemoryModal(false);
    }
  };

  const handleCleanupWorkspace = async () => {
    setCleaning(true);
    setCleanupResult(null);
    try {
      const result = await apiFetch<CleanupResult>('/system/cleanup/workspace', {
        method: 'POST',
        body: JSON.stringify({ confirm: true }),
      });
      setCleanupResult(result);
      refetch();
    } catch (e) {
      setCleanupResult({ success: false, deleted_count: 0, freed_bytes: 0, error: String(e) });
    } finally {
      setCleaning(false);
      setShowWorkspaceModal(false);
    }
  };

  const disk = info?.disk;

  return (
    <div className="p-8">
      <div className="mb-8">
        <h1 className="text-2xl font-bold text-white mb-2">System</h1>
        <p className="text-slate-400">Storage management and system information</p>
      </div>

      {isLoading && !info ? (
        <div className="text-slate-400">Loading system info...</div>
      ) : !info ? (
        <div className="text-red-400">Failed to load system info</div>
      ) : (
        <div className="space-y-6">
          {/* Storage Overview */}
          <Card>
            <CardHeader>
              <CardTitle className="flex items-center gap-2">
                <HardDrive className="w-5 h-5 text-blue-400" />
                Storage
              </CardTitle>
            </CardHeader>
            <CardContent>
              <div className="space-y-4">
                {/* Usage label */}
                <div className="flex items-baseline justify-between">
                  <span className="text-white font-medium">
                    {formatBytes(disk!.used_bytes)} / {formatBytes(disk!.quota_bytes)}
                  </span>
                  <span className="text-slate-400 text-sm">
                    {disk!.percentage}% used
                  </span>
                </div>

                {/* Segmented bar */}
                <div className="w-full h-6 bg-slate-700 rounded-full overflow-hidden flex">
                  {disk!.quota_bytes > 0 && CATEGORY_ORDER.map((key) => {
                    const bytes = disk!.breakdown[key] || 0;
                    const pct = (bytes / disk!.quota_bytes) * 100;
                    if (pct < 0.3) return null;
                    return (
                      <div
                        key={key}
                        className={`${CATEGORY_COLORS[key].bg} h-full transition-all duration-500`}
                        style={{ width: `${pct}%` }}
                        title={`${CATEGORY_COLORS[key].label}: ${formatBytes(bytes)}`}
                      />
                    );
                  })}
                </div>

                {/* Category legend */}
                <div className="grid grid-cols-2 sm:grid-cols-3 gap-3 pt-2">
                  {CATEGORY_ORDER.map((key) => {
                    const bytes = disk!.breakdown[key] || 0;
                    const colors = CATEGORY_COLORS[key];
                    return (
                      <div key={key} className="flex items-center gap-2">
                        <span className={`w-3 h-3 rounded-full ${colors.bg} flex-shrink-0`} />
                        <span className="text-slate-300 text-sm">{colors.label}</span>
                        <span className="text-slate-500 text-sm ml-auto">{formatBytes(bytes)}</span>
                      </div>
                    );
                  })}
                  {/* Free space */}
                  <div className="flex items-center gap-2">
                    <span className="w-3 h-3 rounded-full bg-slate-700 flex-shrink-0 ring-1 ring-slate-600" />
                    <span className="text-slate-300 text-sm">Free</span>
                    <span className="text-slate-500 text-sm ml-auto">
                      {formatBytes(disk!.remaining_bytes)}
                    </span>
                  </div>
                </div>
              </div>
            </CardContent>
          </Card>

          {/* Cleanup actions */}
          <div className="grid gap-6 lg:grid-cols-2">
            {/* Clear Old Memories */}
            <Card>
              <CardHeader>
                <CardTitle className="flex items-center gap-2 text-base">
                  <FileText className="w-5 h-5 text-purple-400" />
                  Clear Old Memories
                </CardTitle>
              </CardHeader>
              <CardContent>
                <p className="text-slate-400 text-sm mb-4">
                  Delete daily log files older than a threshold. Long-term memory (MEMORY.md) is never deleted.
                </p>
                <div className="flex items-center gap-3 mb-4">
                  <label className="text-slate-300 text-sm">Older than</label>
                  <select
                    value={memoryDays}
                    onChange={(e) => setMemoryDays(Number(e.target.value))}
                    className="bg-slate-700 text-white rounded-lg px-3 py-2 text-sm border border-slate-600 focus:outline-none focus:ring-2 focus:ring-stark-500"
                  >
                    <option value={7}>7 days</option>
                    <option value={14}>14 days</option>
                    <option value={30}>30 days</option>
                    <option value={60}>60 days</option>
                    <option value={90}>90 days</option>
                  </select>
                </div>
                <Button
                  variant="danger"
                  onClick={() => setShowMemoryModal(true)}
                  className="w-full"
                >
                  <Trash2 className="w-4 h-4 mr-2" />
                  Clean Up
                </Button>
              </CardContent>
            </Card>

            {/* Clear Workspace */}
            <Card>
              <CardHeader>
                <CardTitle className="flex items-center gap-2 text-base">
                  <FolderOpen className="w-5 h-5 text-blue-400" />
                  Clear Workspace Files
                </CardTitle>
              </CardHeader>
              <CardContent>
                <p className="text-slate-400 text-sm mb-4">
                  Delete all files from the workspace directory. This frees up the most space but removes all saved files.
                </p>
                <div className="mb-4">
                  <div className="flex items-center justify-between p-3 rounded-lg bg-slate-700/50">
                    <span className="text-slate-300 text-sm">Current size</span>
                    <span className="text-white text-sm font-mono">
                      {formatBytes(disk!.breakdown.workspace)}
                    </span>
                  </div>
                </div>
                <Button
                  variant="danger"
                  onClick={() => setShowWorkspaceModal(true)}
                  className="w-full"
                >
                  <Trash2 className="w-4 h-4 mr-2" />
                  Clear All
                </Button>
              </CardContent>
            </Card>
          </div>

          {/* Cleanup result message */}
          {cleanupResult && (
            <div className={`p-4 rounded-lg border ${
              cleanupResult.success
                ? 'bg-green-500/10 border-green-500/30 text-green-400'
                : 'bg-red-500/10 border-red-500/30 text-red-400'
            }`}>
              {cleanupResult.success
                ? `Deleted ${cleanupResult.deleted_count} file${cleanupResult.deleted_count !== 1 ? 's' : ''}, freed ${formatBytes(cleanupResult.freed_bytes)}`
                : `Cleanup failed: ${cleanupResult.error}`}
            </div>
          )}

          {/* System info summary */}
          <Card>
            <CardHeader>
              <CardTitle className="text-base">System Info</CardTitle>
            </CardHeader>
            <CardContent>
              <div className="grid grid-cols-2 gap-3">
                <div className="flex items-center justify-between p-3 rounded-lg bg-slate-700/50">
                  <span className="text-slate-300 text-sm">Version</span>
                  <span className="text-white font-mono text-sm">{info.version}</span>
                </div>
                <div className="flex items-center justify-between p-3 rounded-lg bg-slate-700/50">
                  <span className="text-slate-300 text-sm">Uptime</span>
                  <span className="text-white text-sm">{formatUptime(info.uptime_secs)}</span>
                </div>
              </div>
            </CardContent>
          </Card>
        </div>
      )}

      {/* Confirm: memories cleanup */}
      <Modal
        isOpen={showMemoryModal}
        onClose={() => setShowMemoryModal(false)}
        title="Confirm Cleanup"
        size="sm"
      >
        <p className="text-slate-300 mb-6">
          Delete daily log files older than <strong className="text-white">{memoryDays} days</strong>?
          Long-term memory (MEMORY.md) will not be affected.
        </p>
        <div className="flex gap-3">
          <Button
            variant="ghost"
            onClick={() => setShowMemoryModal(false)}
            className="flex-1"
          >
            Cancel
          </Button>
          <Button
            variant="danger"
            onClick={handleCleanupMemories}
            disabled={cleaning}
            className="flex-1"
          >
            {cleaning ? 'Deleting...' : 'Delete'}
          </Button>
        </div>
      </Modal>

      {/* Confirm: workspace cleanup */}
      <Modal
        isOpen={showWorkspaceModal}
        onClose={() => setShowWorkspaceModal(false)}
        title="Confirm Cleanup"
        size="sm"
      >
        <p className="text-slate-300 mb-6">
          This will <strong className="text-red-400">permanently delete all workspace files</strong>.
          This action cannot be undone.
        </p>
        <div className="flex gap-3">
          <Button
            variant="ghost"
            onClick={() => setShowWorkspaceModal(false)}
            className="flex-1"
          >
            Cancel
          </Button>
          <Button
            variant="danger"
            onClick={handleCleanupWorkspace}
            disabled={cleaning}
            className="flex-1"
          >
            {cleaning ? 'Deleting...' : 'Delete All'}
          </Button>
        </div>
      </Modal>
    </div>
  );
}

function formatUptime(seconds: number): string {
  const days = Math.floor(seconds / 86400);
  const hours = Math.floor((seconds % 86400) / 3600);
  const minutes = Math.floor((seconds % 3600) / 60);
  if (days > 0) return `${days}d ${hours}h ${minutes}m`;
  if (hours > 0) return `${hours}h ${minutes}m`;
  return `${minutes}m`;
}
