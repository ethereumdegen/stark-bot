import { useState, useEffect, useCallback, useMemo, DragEvent } from 'react';
import { useNavigate } from 'react-router-dom';
import {
  Plus,
  Trash2,
  GripVertical,
  ExternalLink,
  ChevronLeft,
  ChevronRight,
  Play,
  Pause,
  Clock,
} from 'lucide-react';
import { useGateway } from '@/hooks/useGateway';
import {
  KanbanItem,
  getKanbanItems,
  createKanbanItem,
  updateKanbanItem,
  deleteKanbanItem,
  CronJobInfo,
  getCronJobs,
  createCronJob,
  deleteCronJob,
  runCronJobNow,
  pauseCronJob,
  resumeCronJob,
  getBotSettings,
  updateBotSettings,
} from '@/lib/api';
import Modal from '@/components/ui/Modal';
import Button from '@/components/ui/Button';
import Input from '@/components/ui/Input';
import UnicodeSpinner from '@/components/ui/UnicodeSpinner';

// ── Kanban Types ──────────────────────────────────────────────────────────────

type KanbanStatus = 'ready' | 'in_progress' | 'complete';

const COLUMNS: { status: KanbanStatus; label: string; color: string; accent: string }[] = [
  { status: 'ready', label: 'To-do', color: 'text-slate-400', accent: 'bg-slate-500' },
  { status: 'in_progress', label: 'In Progress', color: 'text-amber-400', accent: 'bg-amber-500' },
  { status: 'complete', label: 'Completed', color: 'text-emerald-400', accent: 'bg-emerald-500' },
];

const PRIORITY_LABELS: Record<number, { label: string; class: string }> = {
  0: { label: 'Normal', class: 'bg-slate-600 text-slate-300' },
  1: { label: 'High', class: 'bg-orange-600/80 text-orange-100' },
  2: { label: 'Urgent', class: 'bg-red-600/80 text-red-100' },
};

// ── Sprint Timeline Helpers ───────────────────────────────────────────────────

function startOfDay(d: Date): Date {
  const r = new Date(d);
  r.setHours(0, 0, 0, 0);
  return r;
}

function addDays(d: Date, n: number): Date {
  const r = new Date(d);
  r.setDate(r.getDate() + n);
  return r;
}

function formatDayLabel(d: Date): string {
  return d.toLocaleDateString(undefined, { day: 'numeric', month: 'short' });
}

function isSameDay(a: Date, b: Date): boolean {
  return a.getFullYear() === b.getFullYear() && a.getMonth() === b.getMonth() && a.getDate() === b.getDate();
}

/** Color palette for timeline bars */
const TIMELINE_COLORS = [
  { bg: 'bg-amber-700/60', border: 'border-amber-600/40', text: 'text-amber-100' },
  { bg: 'bg-emerald-700/60', border: 'border-emerald-600/40', text: 'text-emerald-100' },
  { bg: 'bg-sky-700/60', border: 'border-sky-600/40', text: 'text-sky-100' },
  { bg: 'bg-rose-700/60', border: 'border-rose-600/40', text: 'text-rose-100' },
  { bg: 'bg-violet-700/60', border: 'border-violet-600/40', text: 'text-violet-100' },
  { bg: 'bg-orange-700/60', border: 'border-orange-600/40', text: 'text-orange-100' },
  { bg: 'bg-teal-700/60', border: 'border-teal-600/40', text: 'text-teal-100' },
  { bg: 'bg-pink-700/60', border: 'border-pink-600/40', text: 'text-pink-100' },
];

// ── Main Component ────────────────────────────────────────────────────────────

export default function Workstream() {
  const navigate = useNavigate();
  const { gateway } = useGateway();

  // Kanban state
  const [items, setItems] = useState<KanbanItem[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  // Cron state
  const [cronJobs, setCronJobs] = useState<CronJobInfo[]>([]);
  const [cronLoading, setCronLoading] = useState(true);

  // Auto-execute toggle
  const [autoExecute, setAutoExecute] = useState(true);

  // Create kanban modal
  const [createOpen, setCreateOpen] = useState(false);
  const [createTitle, setCreateTitle] = useState('');
  const [createDesc, setCreateDesc] = useState('');
  const [createPriority, setCreatePriority] = useState(0);
  const [creating, setCreating] = useState(false);

  // Detail modal (kanban)
  const [detailItem, setDetailItem] = useState<KanbanItem | null>(null);

  // Drag state
  const [dragItemId, setDragItemId] = useState<number | null>(null);

  // Sprint timeline: create task modal
  const [sprintCreateOpen, setSprintCreateOpen] = useState(false);
  const [sprintDate, setSprintDate] = useState<Date | null>(null);
  const [sprintName, setSprintName] = useState('');
  const [sprintMessage, setSprintMessage] = useState('');
  const [sprintTime, setSprintTime] = useState('12:00');
  const [sprintCreating, setSprintCreating] = useState(false);

  // Sprint timeline: detail modal for cron job
  const [sprintDetailJob, setSprintDetailJob] = useState<CronJobInfo | null>(null);

  // Sprint timeline: week offset (0 = current week)
  const [weekOffset, setWeekOffset] = useState(0);

  // ── Data Loading ──────────────────────────────────────────────────────────

  const fetchItems = useCallback(async () => {
    try {
      const data = await getKanbanItems();
      setItems(data);
      setError(null);
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Failed to load kanban items');
    } finally {
      setLoading(false);
    }
  }, []);

  const fetchCronJobs = useCallback(async () => {
    try {
      const jobs = await getCronJobs();
      setCronJobs(jobs);
    } catch {
      // silently fail, cron is supplementary
    } finally {
      setCronLoading(false);
    }
  }, []);

  useEffect(() => {
    fetchItems();
    fetchCronJobs();
    getBotSettings().then((s) => setAutoExecute(s.kanban_auto_execute)).catch(() => {});
  }, [fetchItems, fetchCronJobs]);

  useEffect(() => {
    if (!gateway) return;
    const handleUpdate = () => {
      fetchItems();
      fetchCronJobs();
    };
    gateway.on('kanban_item_updated', handleUpdate);
    return () => { gateway.off('kanban_item_updated', handleUpdate); };
  }, [gateway, fetchItems, fetchCronJobs]);

  // ── Kanban Handlers ───────────────────────────────────────────────────────

  const handleCreate = async () => {
    if (!createTitle.trim()) return;
    setCreating(true);
    try {
      await createKanbanItem({
        title: createTitle.trim(),
        description: createDesc.trim() || undefined,
        priority: createPriority,
      });
      setCreateOpen(false);
      setCreateTitle('');
      setCreateDesc('');
      setCreatePriority(0);
      fetchItems();
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Failed to create item');
    } finally {
      setCreating(false);
    }
  };

  const handleDelete = async (id: number) => {
    try {
      await deleteKanbanItem(id);
      setDetailItem(null);
      fetchItems();
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Failed to delete item');
    }
  };

  const handleStatusChange = async (id: number, status: KanbanStatus) => {
    try {
      await updateKanbanItem(id, { status });
      fetchItems();
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Failed to update item');
    }
  };

  // Drag and drop
  const onDragStart = (e: DragEvent, id: number) => {
    setDragItemId(id);
    e.dataTransfer.effectAllowed = 'move';
    e.dataTransfer.setData('text/plain', String(id));
  };
  const onDragOver = (e: DragEvent) => { e.preventDefault(); e.dataTransfer.dropEffect = 'move'; };
  const onDrop = (e: DragEvent, targetStatus: KanbanStatus) => {
    e.preventDefault();
    const id = dragItemId;
    setDragItemId(null);
    if (id === null) return;
    const item = items.find((i) => i.id === id);
    if (item && item.status !== targetStatus) handleStatusChange(id, targetStatus);
  };
  const onDragEnd = () => { setDragItemId(null); };
  const itemsByStatus = (status: KanbanStatus) => items.filter((i) => i.status === status);

  // ── Sprint Timeline Handlers ──────────────────────────────────────────────

  const handleSprintCreate = async () => {
    if (!sprintName.trim() || !sprintDate || !sprintMessage.trim()) return;
    setSprintCreating(true);
    try {
      const [hours, minutes] = sprintTime.split(':').map(Number);
      const runAt = new Date(sprintDate);
      runAt.setHours(hours, minutes, 0, 0);

      await createCronJob({
        name: sprintName.trim(),
        description: `Sprint task scheduled for ${runAt.toLocaleString()}`,
        schedule_type: 'at',
        schedule_value: runAt.toISOString(),
        session_mode: 'isolated',
        message: sprintMessage.trim(),
        delete_after_run: true,
      });
      setSprintCreateOpen(false);
      setSprintName('');
      setSprintMessage('');
      setSprintTime('12:00');
      setSprintDate(null);
      fetchCronJobs();
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Failed to create sprint task');
    } finally {
      setSprintCreating(false);
    }
  };

  const handleSprintDelete = async (id: number) => {
    try {
      await deleteCronJob(id);
      setSprintDetailJob(null);
      fetchCronJobs();
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Failed to delete sprint task');
    }
  };

  const handleSprintRunNow = async (id: number) => {
    try {
      await runCronJobNow(id);
      fetchCronJobs();
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Failed to run task');
    }
  };

  const handleSprintTogglePause = async (job: CronJobInfo) => {
    try {
      const updated = job.status === 'paused' ? await resumeCronJob(job.id) : await pauseCronJob(job.id);
      setCronJobs((prev) => prev.map((j) => (j.id === job.id ? updated : j)));
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Failed to toggle task');
    }
  };

  // ── Sprint Timeline Data ──────────────────────────────────────────────────

  const today = startOfDay(new Date());
  const weekStart = addDays(today, weekOffset * 7);
  const weekEnd = addDays(weekStart, 7);
  const days = Array.from({ length: 7 }, (_, i) => addDays(weekStart, i));

  // All cron jobs that have a next_run_at or schedule_value in this week's range
  const timelineJobs = useMemo(() => {
    return cronJobs.filter((job) => {
      const dateStr = job.schedule_type === 'at' ? job.schedule_value : job.next_run_at;
      if (!dateStr) return false;
      const d = new Date(dateStr);
      return d >= weekStart && d < weekEnd;
    });
  }, [cronJobs, weekStart, weekEnd]);

  // Position jobs into rows to avoid overlap
  const positionedJobs = useMemo(() => {
    const sorted = [...timelineJobs].sort((a, b) => {
      const da = new Date(a.schedule_type === 'at' ? a.schedule_value : a.next_run_at || '');
      const db = new Date(b.schedule_type === 'at' ? b.schedule_value : b.next_run_at || '');
      return da.getTime() - db.getTime();
    });

    const rows: { job: CronJobInfo; date: Date; dayIndex: number; colorIndex: number }[][] = [];

    sorted.forEach((job, idx) => {
      const dateStr = job.schedule_type === 'at' ? job.schedule_value : job.next_run_at || '';
      const date = new Date(dateStr);
      const dayIndex = Math.floor((date.getTime() - weekStart.getTime()) / 86400000);
      const entry = { job, date, dayIndex: Math.max(0, Math.min(6, dayIndex)), colorIndex: idx % TIMELINE_COLORS.length };

      // Find first row where this job's dayIndex doesn't collide
      let placed = false;
      for (const row of rows) {
        const collides = row.some((e) => e.dayIndex === entry.dayIndex);
        if (!collides) {
          row.push(entry);
          placed = true;
          break;
        }
      }
      if (!placed) {
        rows.push([entry]);
      }
    });

    return rows;
  }, [timelineJobs, weekStart]);

  // ── Loading State ─────────────────────────────────────────────────────────

  if (loading && cronLoading) {
    return (
      <div className="flex items-center justify-center h-64">
        <UnicodeSpinner animation="rain" size="lg" className="text-stark-500" />
      </div>
    );
  }

  // ── Render ────────────────────────────────────────────────────────────────

  return (
    <div className="p-4 md:p-6 h-full flex flex-col gap-4 min-h-0">
      {/* Header */}
      <div className="flex items-center justify-between shrink-0 flex-wrap gap-2">
        <h1 className="text-2xl font-bold text-white">Workstream</h1>
        <div className="flex items-center gap-3 flex-wrap">
          <label className="flex items-center gap-2 cursor-pointer select-none">
            <span className="text-xs text-slate-400">Auto-execute</span>
            <button
              role="switch"
              aria-checked={autoExecute}
              onClick={() => {
                const next = !autoExecute;
                setAutoExecute(next);
                updateBotSettings({ kanban_auto_execute: next }).catch(() => setAutoExecute(!next));
              }}
              className={`relative inline-flex h-5 w-9 items-center rounded-full transition-colors ${
                autoExecute ? 'bg-stark-500' : 'bg-slate-600'
              }`}
            >
              <span
                className={`inline-block h-3.5 w-3.5 rounded-full bg-white transition-transform ${
                  autoExecute ? 'translate-x-[18px]' : 'translate-x-[3px]'
                }`}
              />
            </button>
          </label>
          <Button variant="primary" size="sm" onClick={() => setCreateOpen(true)}>
            <Plus className="w-4 h-4 mr-1" />
            Add Task
          </Button>
        </div>
      </div>

      {error && (
        <div className="shrink-0 p-3 rounded-lg bg-red-500/10 border border-red-500/30 text-red-400 text-sm">
          {error}
          <button onClick={() => setError(null)} className="ml-2 underline">dismiss</button>
        </div>
      )}

      {/* ─── Sprint Timeline (top ~55%) ──────────────────────────────────── */}
      <div className="flex-none md:flex-[3] min-h-[280px] md:min-h-[200px] rounded-xl border border-slate-700/50 bg-slate-800/30 overflow-hidden flex flex-col">
        {/* Timeline header */}
        <div className="flex items-center justify-between px-4 py-3 border-b border-slate-700/50">
          <h2 className="text-lg font-semibold text-white">Sprint Timeline</h2>
          <div className="flex items-center gap-1">
            <button
              onClick={() => setWeekOffset((w) => w - 1)}
              className="p-1.5 text-slate-400 hover:text-white hover:bg-slate-700/50 rounded-lg transition-colors"
            >
              <ChevronLeft className="w-4 h-4" />
            </button>
            <button
              onClick={() => setWeekOffset(0)}
              className="px-3 py-1 text-xs text-slate-400 hover:text-white hover:bg-slate-700/50 rounded-lg transition-colors"
            >
              Today
            </button>
            <button
              onClick={() => setWeekOffset((w) => w + 1)}
              className="p-1.5 text-slate-400 hover:text-white hover:bg-slate-700/50 rounded-lg transition-colors"
            >
              <ChevronRight className="w-4 h-4" />
            </button>
          </div>
        </div>

        {/* Day columns with timeline grid */}
        <div className="relative flex-1 flex flex-col overflow-x-auto">
          {/* Day header row */}
          <div className="grid grid-cols-7 border-b border-slate-700/30 min-w-[600px]">
            {days.map((day, i) => {
              const isToday = isSameDay(day, today);
              return (
                <div
                  key={i}
                  className={`px-2 py-2 text-center text-xs font-medium border-r border-slate-700/20 last:border-r-0
                    ${isToday ? 'text-stark-400 bg-stark-500/5' : 'text-slate-400'}`}
                >
                  {formatDayLabel(day)}
                </div>
              );
            })}
          </div>

          {/* Timeline body - clickable cells + job bars */}
          <div className="relative flex-1 min-h-[160px] min-w-[600px]">
            {/* Click target grid */}
            <div className="grid grid-cols-7 absolute inset-0">
              {days.map((day, i) => {
                const isToday = isSameDay(day, today);
                return (
                  <button
                    key={i}
                    onClick={() => {
                      setSprintDate(day);
                      setSprintCreateOpen(true);
                    }}
                    className={`h-full border-r border-slate-700/20 last:border-r-0 hover:bg-slate-700/20 transition-colors cursor-pointer
                      ${isToday ? 'bg-stark-500/5' : ''}`}
                    title={`Add task on ${formatDayLabel(day)}`}
                  />
                );
              })}
            </div>

            {/* Today marker line */}
            {weekOffset === 0 && (
              <div
                className="absolute top-0 bottom-0 w-0.5 bg-stark-500/60 z-10 pointer-events-none"
                style={{ left: `${((0.5) / 7) * 100}%` }}
              />
            )}

            {/* Job bars */}
            <div className="relative z-20 pointer-events-none p-3 space-y-2">
              {positionedJobs.length === 0 && (
                <div className="flex items-center justify-center h-full min-h-[120px] text-sm text-slate-600">
                  Click a day to schedule a task
                </div>
              )}
              {positionedJobs.map((row, rowIdx) => (
                <div key={rowIdx} className="grid grid-cols-7 gap-1">
                  {Array.from({ length: 7 }, (_, colIdx) => {
                    const entry = row.find((e) => e.dayIndex === colIdx);
                    if (!entry) return <div key={colIdx} />;
                    const c = TIMELINE_COLORS[entry.colorIndex];
                    const isPaused = entry.job.status === 'paused';
                    const isCompleted = entry.job.status === 'completed';
                    return (
                      <button
                        key={colIdx}
                        onClick={() => setSprintDetailJob(entry.job)}
                        className={`pointer-events-auto rounded-md px-2 py-1.5 text-left border truncate text-xs font-medium transition-colors
                          hover:brightness-125 cursor-pointer
                          ${isCompleted ? 'bg-green-800/40 border-green-700/30 text-green-300 line-through opacity-70' :
                            isPaused ? 'bg-yellow-800/40 border-yellow-700/30 text-yellow-300 opacity-70' :
                            `${c.bg} ${c.border} ${c.text}`}`}
                        title={entry.job.name}
                      >
                        <span className="text-[10px] text-slate-400 mr-1">
                          {entry.date.toLocaleTimeString(undefined, { hour: '2-digit', minute: '2-digit' })}
                        </span>
                        {entry.job.name}
                      </button>
                    );
                  })}
                </div>
              ))}
            </div>
          </div>
        </div>
      </div>

      {/* ─── Kanban Board (bottom ~45%) ──────────────────────────────────── */}
      <div className="flex-none md:flex-[2] min-h-0 flex flex-col">
        <div className="grid grid-cols-1 md:grid-cols-3 gap-3 flex-1 min-h-0">
          {COLUMNS.map((col) => (
            <div
              key={col.status}
              className="flex flex-col rounded-xl border border-slate-700/40 bg-slate-850/40 overflow-hidden"
              onDragOver={onDragOver}
              onDrop={(e) => onDrop(e, col.status)}
            >
              {/* Column header */}
              <div className="flex items-center justify-between px-4 py-2.5 border-b border-slate-700/30 bg-slate-800/40">
                <div className="flex items-center gap-2.5">
                  <div className={`w-2 h-2 rounded-full ${col.accent}`} />
                  <h2 className="text-sm font-medium text-slate-300">{col.label}</h2>
                  <span className="text-[11px] text-slate-500 tabular-nums">
                    {itemsByStatus(col.status).length}
                  </span>
                </div>
                {col.status === 'ready' && (
                  <button
                    onClick={() => setCreateOpen(true)}
                    className="p-1 text-slate-600 hover:text-slate-300 hover:bg-slate-700/50 rounded transition-colors"
                  >
                    <Plus className="w-3.5 h-3.5" />
                  </button>
                )}
              </div>

              {/* Cards */}
              <div className="flex-1 overflow-y-auto p-2 space-y-1.5">
                {itemsByStatus(col.status).map((item) => {
                  const accentBorder =
                    item.priority === 2 ? 'border-l-red-500' :
                    item.priority === 1 ? 'border-l-amber-500' :
                    col.status === 'complete' ? 'border-l-emerald-500/50' :
                    col.status === 'in_progress' ? 'border-l-amber-500/50' :
                    'border-l-slate-600';
                  return (
                    <div
                      key={item.id}
                      draggable
                      onDragStart={(e) => onDragStart(e, item.id)}
                      onDragEnd={onDragEnd}
                      onClick={() => setDetailItem(item)}
                      className={`group rounded-lg p-3 border border-slate-700/40 border-l-2 ${accentBorder}
                        bg-slate-800/60 cursor-pointer
                        hover:bg-slate-800 hover:border-slate-600/60 transition-colors
                        ${dragItemId === item.id ? 'opacity-50' : ''}`}
                    >
                      <div className="flex items-start gap-2">
                        <GripVertical className="w-3.5 h-3.5 text-slate-700 mt-0.5 shrink-0 opacity-0 group-hover:opacity-100 transition-opacity cursor-grab" />
                        <div className="flex-1 min-w-0">
                          <div className="flex items-center gap-2 mb-0.5">
                            <span className="text-sm font-medium text-slate-200 truncate">{item.title}</span>
                            {item.priority > 0 && (
                              <span className={`text-[10px] px-1.5 py-0.5 rounded font-medium shrink-0 ${PRIORITY_LABELS[item.priority]?.class}`}>
                                {PRIORITY_LABELS[item.priority]?.label}
                              </span>
                            )}
                          </div>
                          {item.description && (
                            <p className="text-xs text-slate-500 line-clamp-2">{item.description}</p>
                          )}
                          {item.session_id && (
                            <div className="mt-1.5 flex items-center gap-1 text-[10px] text-slate-600">
                              <ExternalLink className="w-3 h-3" />
                              Session #{item.session_id}
                            </div>
                          )}
                        </div>
                      </div>
                    </div>
                  );
                })}

                {itemsByStatus(col.status).length === 0 && (
                  <div className="text-center text-xs text-slate-600 py-6">
                    No tasks
                  </div>
                )}
              </div>
            </div>
          ))}
        </div>
      </div>

      {/* ─── Scheduling Link ──────────────────────────────────────────────── */}
      <div className="shrink-0 flex justify-center py-2">
        <button
          onClick={() => navigate('/scheduling')}
          className="flex items-center gap-2 px-5 py-2 rounded-lg bg-slate-800/60 border border-slate-700/40
            text-slate-400 hover:text-white hover:bg-slate-700/60 hover:border-slate-600/60 transition-colors text-sm font-medium"
        >
          <Clock className="w-4 h-4" />
          Scheduling
        </button>
      </div>

      {/* ─── Create Kanban Task Modal ──────────────────────────────────────── */}
      <Modal isOpen={createOpen} onClose={() => setCreateOpen(false)} title="New Task" size="md">
        <div className="space-y-4">
          <Input
            label="Title"
            value={createTitle}
            onChange={(e) => setCreateTitle(e.target.value)}
            placeholder="What needs to be done?"
            autoFocus
          />
          <div>
            <label className="block text-sm font-medium text-slate-300 mb-1">Description</label>
            <textarea
              value={createDesc}
              onChange={(e) => setCreateDesc(e.target.value)}
              placeholder="Optional details..."
              rows={3}
              className="w-full bg-slate-900 border border-slate-700 rounded-lg px-3 py-2 text-white text-sm
                focus:outline-none focus:ring-2 focus:ring-stark-500 focus:border-transparent resize-none"
            />
          </div>
          <div>
            <label className="block text-sm font-medium text-slate-300 mb-1">Priority</label>
            <select
              value={createPriority}
              onChange={(e) => setCreatePriority(Number(e.target.value))}
              className="w-full bg-slate-900 border border-slate-700 rounded-lg px-3 py-2 text-white text-sm
                focus:outline-none focus:ring-2 focus:ring-stark-500 focus:border-transparent"
            >
              <option value={0}>Normal</option>
              <option value={1}>High</option>
              <option value={2}>Urgent</option>
            </select>
          </div>
          <div className="flex justify-end gap-2 pt-2">
            <Button variant="secondary" size="sm" onClick={() => setCreateOpen(false)}>
              Cancel
            </Button>
            <Button
              variant="primary"
              size="sm"
              onClick={handleCreate}
              isLoading={creating}
              disabled={!createTitle.trim()}
            >
              Create
            </Button>
          </div>
        </div>
      </Modal>

      {/* ─── Kanban Detail Modal ───────────────────────────────────────────── */}
      <Modal
        isOpen={!!detailItem}
        onClose={() => setDetailItem(null)}
        title={detailItem?.title || ''}
        size="lg"
      >
        {detailItem && (
          <div className="space-y-4">
            <div className="flex items-center gap-2">
              <span className={`text-xs px-2 py-1 rounded-full font-medium ${
                detailItem.status === 'ready' ? 'bg-blue-500/20 text-blue-400' :
                detailItem.status === 'in_progress' ? 'bg-yellow-500/20 text-yellow-400' :
                'bg-green-500/20 text-green-400'
              }`}>
                {detailItem.status === 'in_progress' ? 'In Progress' :
                 detailItem.status.charAt(0).toUpperCase() + detailItem.status.slice(1)}
              </span>
              {detailItem.priority > 0 && (
                <span className={`text-xs px-2 py-1 rounded-full font-medium ${PRIORITY_LABELS[detailItem.priority]?.class}`}>
                  {PRIORITY_LABELS[detailItem.priority]?.label}
                </span>
              )}
            </div>

            {detailItem.description && (
              <div>
                <h3 className="text-sm font-medium text-slate-400 mb-1">Description</h3>
                <p className="text-sm text-white whitespace-pre-wrap">{detailItem.description}</p>
              </div>
            )}

            {detailItem.result && (
              <div>
                <h3 className="text-sm font-medium text-slate-400 mb-1">Agent Notes</h3>
                <pre className="text-sm text-slate-300 whitespace-pre-wrap bg-slate-900 rounded-lg p-3 border border-slate-700">
                  {detailItem.result}
                </pre>
              </div>
            )}

            {detailItem.session_id && (
              <div>
                <h3 className="text-sm font-medium text-slate-400 mb-1">Session</h3>
                <button
                  onClick={() => navigate(`/sessions/${detailItem.session_id}`)}
                  className="text-sm text-stark-400 hover:text-stark-300 flex items-center gap-1"
                >
                  <ExternalLink className="w-3.5 h-3.5" />
                  View Session #{detailItem.session_id}
                </button>
              </div>
            )}

            <div className="text-xs text-slate-500 space-y-1">
              <p>Created: {new Date(detailItem.created_at).toLocaleString()}</p>
              <p>Updated: {new Date(detailItem.updated_at).toLocaleString()}</p>
            </div>

            <div className="flex flex-col-reverse sm:flex-row items-stretch sm:items-center justify-between gap-2 pt-2 border-t border-slate-700">
              <Button
                variant="danger"
                size="sm"
                onClick={() => handleDelete(detailItem.id)}
              >
                <Trash2 className="w-3.5 h-3.5 mr-1" />
                Delete
              </Button>

              <div className="flex flex-wrap gap-2">
                {detailItem.status !== 'ready' && (
                  <Button variant="secondary" size="sm" onClick={() => { handleStatusChange(detailItem.id, 'ready'); setDetailItem(null); }}>
                    Move to To-do
                  </Button>
                )}
                {detailItem.status !== 'in_progress' && (
                  <Button variant="secondary" size="sm" onClick={() => { handleStatusChange(detailItem.id, 'in_progress'); setDetailItem(null); }}>
                    Move to In Progress
                  </Button>
                )}
                {detailItem.status !== 'complete' && (
                  <Button variant="primary" size="sm" onClick={() => { handleStatusChange(detailItem.id, 'complete'); setDetailItem(null); }}>
                    Mark Complete
                  </Button>
                )}
              </div>
            </div>
          </div>
        )}
      </Modal>

      {/* ─── Sprint Create Task Modal ──────────────────────────────────────── */}
      <Modal
        isOpen={sprintCreateOpen}
        onClose={() => { setSprintCreateOpen(false); setSprintDate(null); }}
        title={`Schedule Task${sprintDate ? ` - ${formatDayLabel(sprintDate)}` : ''}`}
        size="md"
      >
        <div className="space-y-4">
          <Input
            label="Task Name"
            value={sprintName}
            onChange={(e) => setSprintName(e.target.value)}
            placeholder="e.g. Code Review, Deploy, Sprint Sync"
            autoFocus
          />
          <div>
            <label className="block text-sm font-medium text-slate-300 mb-1">Time</label>
            <input
              type="time"
              value={sprintTime}
              onChange={(e) => setSprintTime(e.target.value)}
              className="w-full bg-slate-900 border border-slate-700 rounded-lg px-3 py-2 text-white text-sm
                focus:outline-none focus:ring-2 focus:ring-stark-500 focus:border-transparent"
            />
          </div>
          <div>
            <label className="block text-sm font-medium text-slate-300 mb-1">Agent Message</label>
            <textarea
              value={sprintMessage}
              onChange={(e) => setSprintMessage(e.target.value)}
              placeholder="What should the agent do when this task runs?"
              rows={3}
              className="w-full bg-slate-900 border border-slate-700 rounded-lg px-3 py-2 text-white text-sm
                focus:outline-none focus:ring-2 focus:ring-stark-500 focus:border-transparent resize-none"
            />
          </div>
          <p className="text-xs text-slate-500">
            This creates a one-time scheduled cron job that auto-deletes after running.
          </p>
          <div className="flex justify-end gap-2 pt-2">
            <Button variant="secondary" size="sm" onClick={() => { setSprintCreateOpen(false); setSprintDate(null); }}>
              Cancel
            </Button>
            <Button
              variant="primary"
              size="sm"
              onClick={handleSprintCreate}
              isLoading={sprintCreating}
              disabled={!sprintName.trim() || !sprintMessage.trim()}
            >
              Schedule
            </Button>
          </div>
        </div>
      </Modal>

      {/* ─── Sprint Detail Modal (cron job) ────────────────────────────────── */}
      <Modal
        isOpen={!!sprintDetailJob}
        onClose={() => setSprintDetailJob(null)}
        title={sprintDetailJob?.name || ''}
        size="md"
      >
        {sprintDetailJob && (
          <div className="space-y-4">
            <div className="flex items-center gap-2 flex-wrap">
              <span className={`text-xs px-2 py-1 rounded-full font-medium ${
                sprintDetailJob.status === 'active' ? 'bg-green-500/20 text-green-400' :
                sprintDetailJob.status === 'paused' ? 'bg-yellow-500/20 text-yellow-400' :
                sprintDetailJob.status === 'completed' ? 'bg-blue-500/20 text-blue-400' :
                'bg-red-500/20 text-red-400'
              }`}>
                {sprintDetailJob.status}
              </span>
              <span className="text-xs text-slate-500 bg-slate-800 px-2 py-1 rounded-md font-mono">
                {sprintDetailJob.schedule_type === 'at'
                  ? new Date(sprintDetailJob.schedule_value).toLocaleString()
                  : sprintDetailJob.schedule_value}
              </span>
              {sprintDetailJob.delete_after_run && (
                <span className="text-[10px] text-slate-500 bg-slate-800 px-1.5 py-0.5 rounded">one-shot</span>
              )}
            </div>

            {sprintDetailJob.description && (
              <div>
                <h3 className="text-sm font-medium text-slate-400 mb-1">Description</h3>
                <p className="text-sm text-white">{sprintDetailJob.description}</p>
              </div>
            )}

            {sprintDetailJob.message && (
              <div>
                <h3 className="text-sm font-medium text-slate-400 mb-1">Agent Message</h3>
                <pre className="text-sm text-slate-300 whitespace-pre-wrap bg-slate-900 rounded-lg p-3 border border-slate-700">
                  {sprintDetailJob.message}
                </pre>
              </div>
            )}

            <div className="text-xs text-slate-500 space-y-1">
              {sprintDetailJob.next_run_at && (
                <p>Next run: {new Date(sprintDetailJob.next_run_at).toLocaleString()}</p>
              )}
              {sprintDetailJob.last_run_at && (
                <p>Last run: {new Date(sprintDetailJob.last_run_at).toLocaleString()}</p>
              )}
              <p>Runs: {sprintDetailJob.run_count} | Errors: {sprintDetailJob.error_count}</p>
              <p>Created: {new Date(sprintDetailJob.created_at).toLocaleString()}</p>
            </div>

            {sprintDetailJob.last_error && (
              <div className="p-3 bg-red-500/10 border border-red-500/20 rounded-lg">
                <p className="text-xs text-red-400">{sprintDetailJob.last_error}</p>
              </div>
            )}

            <div className="flex items-center justify-between pt-2 border-t border-slate-700">
              <Button
                variant="danger"
                size="sm"
                onClick={() => handleSprintDelete(sprintDetailJob.id)}
              >
                <Trash2 className="w-3.5 h-3.5 mr-1" />
                Delete
              </Button>

              <div className="flex gap-2">
                <Button
                  variant="secondary"
                  size="sm"
                  onClick={() => { handleSprintTogglePause(sprintDetailJob); setSprintDetailJob(null); }}
                >
                  {sprintDetailJob.status === 'paused' ? (
                    <><Play className="w-3.5 h-3.5 mr-1" /> Resume</>
                  ) : (
                    <><Pause className="w-3.5 h-3.5 mr-1" /> Pause</>
                  )}
                </Button>
                <Button
                  variant="primary"
                  size="sm"
                  onClick={() => { handleSprintRunNow(sprintDetailJob.id); setSprintDetailJob(null); }}
                >
                  <Play className="w-3.5 h-3.5 mr-1" />
                  Run Now
                </Button>
              </div>
            </div>
          </div>
        )}
      </Modal>
    </div>
  );
}
