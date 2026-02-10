import { useState, useRef, useEffect } from 'react';
import { Users, X, Loader2 } from 'lucide-react';
import { cancelSubagent } from '@/lib/api';
import { Subagent, SubagentStatus } from '@/lib/subagent-types';

// Re-export types for backwards compatibility
export type { Subagent } from '@/lib/subagent-types';
export { SubagentStatus } from '@/lib/subagent-types';

interface SubagentBadgeProps {
  subagents: Subagent[];
  onSubagentCancelled?: (id: string) => void;
}

export default function SubagentBadge({ subagents, onSubagentCancelled }: SubagentBadgeProps) {
  const [isOpen, setIsOpen] = useState(false);
  const [cancellingId, setCancellingId] = useState<string | null>(null);
  const dropdownRef = useRef<HTMLDivElement>(null);

  // Only show running subagents
  const runningSubagents = subagents.filter(s => s.status === SubagentStatus.Running || s.status === SubagentStatus.Pending);

  // Close dropdown when clicking outside
  useEffect(() => {
    function handleClickOutside(event: MouseEvent) {
      if (dropdownRef.current && !dropdownRef.current.contains(event.target as Node)) {
        setIsOpen(false);
      }
    }
    document.addEventListener('mousedown', handleClickOutside);
    return () => document.removeEventListener('mousedown', handleClickOutside);
  }, []);

  // Don't render if no running subagents
  if (runningSubagents.length === 0) {
    return null;
  }

  const handleCancel = async (id: string) => {
    setCancellingId(id);
    try {
      const result = await cancelSubagent(id);
      if (result.success) {
        onSubagentCancelled?.(id);
      }
    } catch (error) {
      console.error('Failed to cancel subagent:', error);
    } finally {
      setCancellingId(null);
    }
  };

  const formatTime = (isoString: string) => {
    const date = new Date(isoString);
    const now = new Date();
    const diffMs = now.getTime() - date.getTime();
    const diffSecs = Math.floor(diffMs / 1000);
    if (diffSecs < 60) return `${diffSecs}s ago`;
    const diffMins = Math.floor(diffSecs / 60);
    if (diffMins < 60) return `${diffMins}m ago`;
    return `${Math.floor(diffMins / 60)}h ago`;
  };

  return (
    <div className="relative" ref={dropdownRef}>
      {/* Badge button */}
      <button
        onClick={() => setIsOpen(!isOpen)}
        className="flex items-center gap-1.5 px-2.5 py-1 bg-purple-500/20 hover:bg-purple-500/30 text-purple-400 rounded-lg text-sm font-medium transition-colors"
      >
        <Users className="w-3.5 h-3.5" />
        <span>subagents</span>
        <span className="bg-purple-500/40 px-1.5 py-0.5 rounded text-xs font-bold">
          {runningSubagents.length}
        </span>
      </button>

      {/* Dropdown panel */}
      {isOpen && (
        <div className="absolute right-0 top-full mt-2 w-80 max-w-[calc(100vw-1.5rem)] bg-slate-800 border border-slate-700 rounded-lg shadow-xl z-50 overflow-hidden">
          <div className="px-3 py-2 border-b border-slate-700 bg-slate-800/80">
            <h3 className="text-sm font-semibold text-white">Running Subagents</h3>
          </div>
          <div className="max-h-64 overflow-y-auto">
            {runningSubagents.map((subagent) => (
              <div
                key={subagent.id}
                className="px-3 py-2 border-b border-slate-700/50 last:border-b-0 hover:bg-slate-700/30"
              >
                <div className="flex items-start justify-between gap-2">
                  <div className="flex-1 min-w-0">
                    <div className="flex items-center gap-2">
                      <span className="text-sm font-medium text-white truncate">
                        {subagent.label}
                      </span>
                      <span className={`text-xs px-1.5 py-0.5 rounded ${
                        subagent.status === SubagentStatus.Running
                          ? 'bg-green-500/20 text-green-400'
                          : 'bg-yellow-500/20 text-yellow-400'
                      }`}>
                        {subagent.status}
                      </span>
                    </div>
                    <p className="text-xs text-slate-400 mt-0.5 line-clamp-2">
                      {subagent.task}
                    </p>
                    <p className="text-xs text-slate-500 mt-1">
                      Started {formatTime(subagent.started_at)}
                    </p>
                  </div>
                  <button
                    onClick={() => handleCancel(subagent.id)}
                    disabled={cancellingId === subagent.id}
                    className="flex-shrink-0 p-1 text-slate-400 hover:text-red-400 hover:bg-red-500/10 rounded transition-colors disabled:opacity-50"
                    title="Cancel subagent"
                  >
                    {cancellingId === subagent.id ? (
                      <Loader2 className="w-4 h-4 animate-spin" />
                    ) : (
                      <X className="w-4 h-4" />
                    )}
                  </button>
                </div>
              </div>
            ))}
          </div>
          {runningSubagents.length === 0 && (
            <div className="px-3 py-4 text-center text-sm text-slate-400">
              No subagents running
            </div>
          )}
        </div>
      )}
    </div>
  );
}
