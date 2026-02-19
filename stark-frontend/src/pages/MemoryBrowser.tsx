import { useState, useEffect, useMemo, lazy, Suspense } from 'react';
import {
  Search,
  Calendar,
  Brain,
  User,
  FolderOpen,
  ChevronLeft,
  ChevronRight,
  BarChart3,
  Plus,
  X,
  Share2,
  Hash,
} from 'lucide-react';
import Card, { CardContent } from '@/components/ui/Card';
import Button from '@/components/ui/Button';
import { apiFetch } from '@/lib/api';

// Lazy-load the graph visualization (heavy D3 dependency)
const MemoryGraph = lazy(() => import('./MemoryGraph'));

// ============================================================================
// API Types (matching new DB-backed backend)
// ============================================================================

interface MemoryEntry {
  path: string;
  memory_type: 'daily_log' | 'long_term';
  date: string | null;
  identity_id: string | null;
  entry_count: number;
}

interface ListFilesResponse {
  success: boolean;
  files: MemoryEntry[];
  error?: string;
}

interface MemoryItem {
  id: number;
  content: string;
  memory_type: string;
  importance: number;
  identity_id?: string | null;
  log_date?: string | null;
  source_type?: string | null;
  created_at: string;
}

interface ReadMemoriesResponse {
  success: boolean;
  memory_type: string;
  date?: string | null;
  identity_id?: string | null;
  memories: MemoryItem[];
  error?: string;
}

interface SearchResult {
  memory_id: number;
  content: string;
  memory_type: string;
  importance: number;
  score: number;
  identity_id?: string | null;
  log_date?: string | null;
}

interface SearchResponse {
  success: boolean;
  query: string;
  results: SearchResult[];
  error?: string;
}

interface MemoryStatsResponse {
  success: boolean;
  total_memories: number;
  daily_log_count: number;
  long_term_count: number;
  identity_count: number;
  identities: string[];
  earliest_date?: string | null;
  latest_date?: string | null;
  error?: string;
}

// ============================================================================
// Helpers
// ============================================================================

function identityLabel(id: string | null | undefined): string {
  if (!id) return 'Standard';
  if (id === 'safemode') return 'Safe Mode';
  return id.length > 12 ? id.slice(0, 8) + '...' : id;
}

function ModeBadge({ identityId }: { identityId: string | null | undefined }) {
  if (identityId === 'safemode') {
    return (
      <span className="text-[10px] px-1.5 py-0.5 rounded bg-amber-500/20 text-amber-400 font-medium">
        Safe
      </span>
    );
  }
  return null;
}

function importanceBadge(importance: number) {
  const color = importance >= 8 ? 'text-red-400' : importance >= 5 ? 'text-yellow-400' : 'text-slate-500';
  return <span className={`text-[10px] ${color}`} title={`Importance: ${importance}`}>{'*'.repeat(Math.min(importance, 10))}</span>;
}

// ============================================================================
// API Functions
// ============================================================================

async function getMemoryEntries(identityId?: string): Promise<ListFilesResponse> {
  const params = identityId ? `?identity_id=${encodeURIComponent(identityId)}` : '';
  return apiFetch(`/memory/files${params}`);
}

async function searchMemory(query: string, limit = 20, identityId?: string): Promise<SearchResponse> {
  const params = new URLSearchParams({ query, limit: String(limit) });
  if (identityId) params.set('identity_id', identityId);
  return apiFetch(`/memory/search?${params.toString()}`);
}

async function getMemoryStats(): Promise<MemoryStatsResponse> {
  return apiFetch('/memory/stats');
}

async function getDailyLog(date?: string, identityId?: string): Promise<ReadMemoriesResponse> {
  const params = new URLSearchParams();
  if (date) params.set('date', date);
  if (identityId) params.set('identity_id', identityId);
  const query = params.toString();
  return apiFetch(`/memory/daily${query ? `?${query}` : ''}`);
}

async function getLongTermMemories(identityId?: string): Promise<ReadMemoriesResponse> {
  const params = identityId ? `?identity_id=${encodeURIComponent(identityId)}` : '';
  return apiFetch(`/memory/long-term${params}`);
}

async function appendToDailyLog(content: string, identityId?: string): Promise<{ success: boolean; error?: string }> {
  return apiFetch('/memory/daily', {
    method: 'POST',
    body: JSON.stringify({ content, identity_id: identityId }),
  });
}

async function appendToLongTerm(content: string, identityId?: string): Promise<{ success: boolean; error?: string }> {
  return apiFetch('/memory/long-term', {
    method: 'POST',
    body: JSON.stringify({ content, identity_id: identityId }),
  });
}

// ============================================================================
// Components
// ============================================================================

type ViewMode = 'browse' | 'calendar' | 'search' | 'graph' | 'stats';

function FileTypeIcon({ type }: { type: string }) {
  if (type === 'long_term') return <Brain className="w-4 h-4 text-purple-400" />;
  if (type === 'daily_log') return <Calendar className="w-4 h-4 text-blue-400" />;
  return <Hash className="w-4 h-4 text-slate-400" />;
}

function CalendarView({
  entries,
  selectedDate,
  onSelectDate,
}: {
  entries: MemoryEntry[];
  selectedDate: string | null;
  onSelectDate: (date: string) => void;
}) {
  const [currentMonth, setCurrentMonth] = useState(() => {
    const now = new Date();
    return new Date(now.getFullYear(), now.getMonth(), 1);
  });

  const dateEntryCounts = useMemo(() => {
    const counts = new Map<string, number>();
    entries.forEach((e) => {
      if (e.date) counts.set(e.date, (counts.get(e.date) || 0) + e.entry_count);
    });
    return counts;
  }, [entries]);

  const calendarDays = useMemo(() => {
    const year = currentMonth.getFullYear();
    const month = currentMonth.getMonth();
    const firstDay = new Date(year, month, 1);
    const lastDay = new Date(year, month + 1, 0);
    const startPadding = firstDay.getDay();
    const totalDays = lastDay.getDate();

    const days: { date: string | null; day: number | null; hasEntry: boolean; entryCount: number }[] = [];
    for (let i = 0; i < startPadding; i++) {
      days.push({ date: null, day: null, hasEntry: false, entryCount: 0 });
    }
    for (let d = 1; d <= totalDays; d++) {
      const dateStr = `${year}-${String(month + 1).padStart(2, '0')}-${String(d).padStart(2, '0')}`;
      const entryCount = dateEntryCounts.get(dateStr) || 0;
      days.push({ date: dateStr, day: d, hasEntry: entryCount > 0, entryCount });
    }
    return days;
  }, [currentMonth, dateEntryCounts]);

  const monthName = currentMonth.toLocaleString('default', { month: 'long', year: 'numeric' });
  const today = new Date().toISOString().split('T')[0];

  return (
    <div className="bg-slate-800/50 rounded-lg p-4">
      <div className="flex items-center justify-between mb-4">
        <Button variant="ghost" size="sm" onClick={() => setCurrentMonth(new Date(currentMonth.getFullYear(), currentMonth.getMonth() - 1, 1))}>
          <ChevronLeft className="w-4 h-4" />
        </Button>
        <span className="text-white font-medium">{monthName}</span>
        <Button variant="ghost" size="sm" onClick={() => setCurrentMonth(new Date(currentMonth.getFullYear(), currentMonth.getMonth() + 1, 1))}>
          <ChevronRight className="w-4 h-4" />
        </Button>
      </div>
      <div className="grid grid-cols-7 gap-1 mb-2">
        {['Sun', 'Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat'].map((day) => (
          <div key={day} className="text-center text-xs text-slate-500 py-1">{day}</div>
        ))}
      </div>
      <div className="grid grid-cols-7 gap-1">
        {calendarDays.map((d, i) => (
          <button
            key={i}
            disabled={!d.date}
            onClick={() => d.date && onSelectDate(d.date)}
            title={d.hasEntry ? `${d.entryCount} entr${d.entryCount === 1 ? 'y' : 'ies'}` : undefined}
            className={`
              relative aspect-square flex flex-col items-center justify-center text-sm rounded transition-colors
              ${!d.date ? 'cursor-default' : 'cursor-pointer'}
              ${d.date === selectedDate ? 'bg-stark-500 text-white font-bold' : ''}
              ${d.date === today && d.date !== selectedDate ? 'ring-2 ring-stark-400 font-medium' : ''}
              ${d.hasEntry && d.date !== selectedDate ? 'bg-blue-500/20 text-blue-300 hover:bg-blue-500/30' : ''}
              ${!d.hasEntry && d.date && d.date !== selectedDate ? 'text-slate-500 hover:bg-slate-700' : ''}
            `}
          >
            <span>{d.day}</span>
            {d.hasEntry && d.entryCount > 0 && (
              <span className="text-[9px] leading-none opacity-70">{d.entryCount}</span>
            )}
          </button>
        ))}
      </div>
    </div>
  );
}

function SearchView({ onSelectMemory, identityFilter }: { onSelectMemory: (id: number) => void; identityFilter?: string }) {
  const [query, setQuery] = useState('');
  const [results, setResults] = useState<SearchResult[]>([]);
  const [isSearching, setIsSearching] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const handleSearch = async () => {
    if (!query.trim()) return;
    setIsSearching(true);
    setError(null);
    try {
      const response = await searchMemory(query, 30, identityFilter);
      if (response.success) {
        setResults(response.results);
      } else {
        setError(response.error || 'Search failed');
      }
    } catch {
      setError('Search request failed');
    } finally {
      setIsSearching(false);
    }
  };

  return (
    <div className="space-y-4">
      <div className="flex gap-2">
        <div className="flex-1 relative">
          <Search className="w-4 h-4 absolute left-3 top-1/2 -translate-y-1/2 text-slate-500" />
          <input
            type="text"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            onKeyDown={(e) => e.key === 'Enter' && handleSearch()}
            placeholder="Search memories (BM25 full-text)..."
            className="w-full pl-10 pr-4 py-2 bg-slate-800 border border-slate-700 rounded-lg text-white text-sm focus:outline-none focus:border-stark-500"
          />
        </div>
        <Button onClick={handleSearch} disabled={isSearching || !query.trim()}>
          {isSearching ? 'Searching...' : 'Search'}
        </Button>
      </div>

      {error && (
        <div className="text-red-400 text-sm bg-red-500/10 px-3 py-2 rounded flex items-center justify-between">
          <span>{error}</span>
          <button onClick={handleSearch} className="text-red-300 underline text-xs ml-2">Retry</button>
        </div>
      )}

      {results.length > 0 ? (
        <div className="space-y-2">
          <div className="text-sm text-slate-400">{results.length} results found</div>
          {results.map((result) => (
            <button
              key={result.memory_id}
              onClick={() => onSelectMemory(result.memory_id)}
              className="w-full text-left p-3 bg-slate-800/50 hover:bg-slate-700/50 rounded-lg transition-colors"
            >
              <div className="flex items-center gap-2 mb-1">
                <FileTypeIcon type={result.memory_type} />
                <span className="text-xs text-slate-500">#{result.memory_id}</span>
                <span className="text-xs text-slate-500">{result.memory_type}</span>
                {result.log_date && <span className="text-xs text-slate-500">{result.log_date}</span>}
                {importanceBadge(result.importance)}
                <ModeBadge identityId={result.identity_id} />
                <span className="text-xs text-slate-500 ml-auto flex items-center gap-1" title={`BM25 score: ${result.score.toFixed(4)}`}>
                  <span className="inline-block w-12 h-1.5 bg-slate-700 rounded-full overflow-hidden">
                    <span className="block h-full bg-stark-400 rounded-full" style={{ width: `${Math.min(100, result.score * 8)}%` }} />
                  </span>
                </span>
              </div>
              <p className="text-sm text-slate-300 line-clamp-3">{result.content}</p>
            </button>
          ))}
        </div>
      ) : query && !isSearching ? (
        <div className="text-center text-slate-500 py-8">
          <Search className="w-8 h-8 mx-auto mb-2 opacity-40" />
          <p>No results found for "{query}"</p>
          <p className="text-xs mt-1">Try different keywords or shorter search terms</p>
        </div>
      ) : !query ? (
        <div className="text-center text-slate-500 py-8">
          <Search className="w-8 h-8 mx-auto mb-2 opacity-30" />
          <p className="text-sm">Search across all memories using full-text search</p>
          <p className="text-xs mt-1 text-slate-600">Press Enter or click Search to find results</p>
        </div>
      ) : null}
    </div>
  );
}

function MemoryItemView({ item }: { item: MemoryItem }) {
  return (
    <div className="px-3 py-2 border-l-2 border-slate-600 hover:border-stark-500 transition-colors">
      <div className="flex items-center gap-2 mb-1">
        <FileTypeIcon type={item.memory_type} />
        <span className="text-xs text-slate-500">#{item.id}</span>
        {importanceBadge(item.importance)}
        {item.source_type && <span className="text-[10px] px-1 py-0.5 rounded bg-slate-700 text-slate-400">{item.source_type}</span>}
        <ModeBadge identityId={item.identity_id} />
        <span className="text-xs text-slate-600 ml-auto">{item.created_at.split('.')[0]}</span>
      </div>
      <div className="text-sm text-slate-300 whitespace-pre-wrap">{item.content}</div>
    </div>
  );
}

function StatsView({ stats }: { stats: MemoryStatsResponse | null }) {
  if (!stats) return <div className="text-slate-500 text-center py-8">Loading stats...</div>;

  return (
    <div className="space-y-4">
      <div className="grid grid-cols-2 gap-3">
        <Card>
          <CardContent className="py-3 text-center">
            <div className="text-2xl font-bold text-white">{stats.total_memories}</div>
            <div className="text-xs text-slate-400">Total Memories</div>
          </CardContent>
        </Card>
        <Card>
          <CardContent className="py-3 text-center">
            <div className="text-2xl font-bold text-blue-400">{stats.daily_log_count}</div>
            <div className="text-xs text-slate-400">Daily Logs</div>
          </CardContent>
        </Card>
        <Card>
          <CardContent className="py-3 text-center">
            <div className="text-2xl font-bold text-purple-400">{stats.long_term_count}</div>
            <div className="text-xs text-slate-400">Long-term</div>
          </CardContent>
        </Card>
        <Card>
          <CardContent className="py-3 text-center">
            <div className="text-2xl font-bold text-white">{stats.identity_count}</div>
            <div className="text-xs text-slate-400">Identities</div>
          </CardContent>
        </Card>
      </div>

      {(stats.earliest_date || stats.latest_date) && (
        <Card>
          <CardContent className="py-3">
            <div className="text-sm text-slate-400 mb-2">Date Range</div>
            <div className="text-white text-sm">
              {stats.earliest_date} to {stats.latest_date}
            </div>
          </CardContent>
        </Card>
      )}

      {stats.identities.length > 0 && (
        <Card>
          <CardContent className="py-3">
            <div className="text-sm text-slate-400 mb-2">Identities</div>
            <div className="flex flex-wrap gap-2">
              {stats.identities.map((id) => (
                <span key={id} className="text-xs px-2 py-1 rounded bg-slate-700 text-slate-300">
                  {identityLabel(id)}
                </span>
              ))}
            </div>
          </CardContent>
        </Card>
      )}
    </div>
  );
}

function AddEntryModal({
  type,
  identityId,
  onClose,
  onSuccess,
}: {
  type: 'daily' | 'long_term';
  identityId: string | null;
  onClose: () => void;
  onSuccess: () => void;
}) {
  const [content, setContent] = useState('');
  const [isSubmitting, setIsSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const handleSubmit = async () => {
    if (!content.trim()) return;
    setIsSubmitting(true);
    setError(null);
    try {
      const response = type === 'daily'
        ? await appendToDailyLog(content, identityId || undefined)
        : await appendToLongTerm(content, identityId || undefined);
      if (response.success) {
        onSuccess();
        onClose();
      } else {
        setError(response.error || 'Failed to add entry');
      }
    } catch {
      setError('Request failed');
    } finally {
      setIsSubmitting(false);
    }
  };

  return (
    <div className="fixed inset-0 bg-black/50 flex items-center justify-center z-50">
      <div className="bg-slate-900 rounded-lg p-6 max-w-lg w-full mx-4">
        <div className="flex items-center justify-between mb-4">
          <h2 className="text-lg font-bold text-white">
            Add to {type === 'daily' ? 'Daily Log' : 'Long-term Memory'}
          </h2>
          <button onClick={onClose} className="text-slate-400 hover:text-white">
            <X className="w-5 h-5" />
          </button>
        </div>
        {identityId && (
          <div className="text-sm text-slate-400 mb-3">
            Identity: <span className="text-stark-400">{identityId}</span>
          </div>
        )}
        <textarea
          value={content}
          onChange={(e) => setContent(e.target.value)}
          placeholder={type === 'daily' ? 'What happened today?' : 'Add a fact, preference, or important information...'}
          className="w-full bg-slate-800 border border-slate-600 rounded-lg px-3 py-2 text-white text-sm resize-none h-32 mb-4"
        />
        {error && <div className="text-red-400 text-sm mb-4">{error}</div>}
        <div className="flex justify-end gap-2">
          <Button variant="secondary" onClick={onClose}>Cancel</Button>
          <Button onClick={handleSubmit} disabled={isSubmitting || !content.trim()}>
            {isSubmitting ? 'Adding...' : 'Add Entry'}
          </Button>
        </div>
      </div>
    </div>
  );
}

// ============================================================================
// Main Component
// ============================================================================

export default function MemoryBrowser() {
  const [entries, setEntries] = useState<MemoryEntry[]>([]);
  const [stats, setStats] = useState<MemoryStatsResponse | null>(null);
  const [isLoading, setIsLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  // View state
  const [viewMode, setViewMode] = useState<ViewMode>('graph');
  const [selectedMemories, setSelectedMemories] = useState<MemoryItem[]>([]);
  const [selectedLabel, setSelectedLabel] = useState<string | null>(null);
  const [loadingContent, setLoadingContent] = useState(false);

  // Filter state
  const [modeFilter, setModeFilter] = useState<string>('all');
  const [selectedDate, setSelectedDate] = useState<string | null>(null);

  // Add entry modal
  const [addEntryType, setAddEntryType] = useState<'daily' | 'long_term' | null>(null);

  const identityFilter = modeFilter === 'safemode' ? 'safemode' : undefined;

  // Load entries and stats
  const loadData = async () => {
    setIsLoading(true);
    setError(null);
    try {
      const [entriesRes, statsRes] = await Promise.all([
        getMemoryEntries(identityFilter),
        getMemoryStats(),
      ]);
      if (entriesRes.success) setEntries(entriesRes.files);
      else setError(entriesRes.error || 'Failed to load entries');
      if (statsRes.success) setStats(statsRes);
    } catch {
      setError('Failed to load memory data');
    } finally {
      setIsLoading(false);
    }
  };

  useEffect(() => { loadData(); }, [modeFilter]);

  // Load memories for a specific date
  const loadDailyLogByDate = async (date: string) => {
    setLoadingContent(true);
    setSelectedDate(date);
    try {
      const response = await getDailyLog(date, identityFilter);
      if (response.success) {
        setSelectedMemories(response.memories);
        setSelectedLabel(`Daily Log: ${date}`);
      } else {
        setSelectedMemories([]);
        setSelectedLabel(`Daily Log: ${date}`);
      }
    } catch {
      setSelectedMemories([]);
      setSelectedLabel(`Daily Log: ${date}`);
    } finally {
      setLoadingContent(false);
    }
  };

  // Load long-term memories
  const loadLongTerm = async () => {
    setLoadingContent(true);
    try {
      const response = await getLongTermMemories(identityFilter);
      if (response.success) {
        setSelectedMemories(response.memories);
        setSelectedLabel('Long-term Memory');
      }
    } catch {
      setSelectedMemories([]);
    } finally {
      setLoadingContent(false);
    }
  };

  // Handle clicking an entry in the browse list
  const handleEntryClick = (entry: MemoryEntry) => {
    if (entry.memory_type === 'long_term') {
      loadLongTerm();
    } else if (entry.date) {
      loadDailyLogByDate(entry.date);
    }
  };

  // Filter entries by mode
  const filteredEntries = useMemo(() => {
    if (modeFilter === 'all') return entries;
    if (modeFilter === 'safemode') return entries.filter((e) => e.identity_id === 'safemode');
    return entries.filter((e) => e.identity_id !== 'safemode');
  }, [entries, modeFilter]);

  const groupedEntries = useMemo(() => {
    const longTerm = filteredEntries.filter((e) => e.memory_type === 'long_term');
    const dailyLogs = filteredEntries.filter((e) => e.memory_type === 'daily_log');
    return { longTerm, dailyLogs };
  }, [filteredEntries]);

  if (isLoading && entries.length === 0) {
    return (
      <div className="p-8 flex items-center justify-center">
        <div className="flex items-center gap-3">
          <div className="w-6 h-6 border-2 border-stark-500 border-t-transparent rounded-full animate-spin" />
          <span className="text-slate-400">Loading memories...</span>
        </div>
      </div>
    );
  }

  const tabs: { key: ViewMode; icon: React.ReactNode; label: string }[] = [
    { key: 'graph', icon: <Share2 className="w-4 h-4 inline mr-1.5" />, label: 'Graph' },
    { key: 'browse', icon: <FolderOpen className="w-4 h-4 inline mr-1.5" />, label: 'Browse' },
    { key: 'calendar', icon: <Calendar className="w-4 h-4 inline mr-1.5" />, label: 'Calendar' },
    { key: 'search', icon: <Search className="w-4 h-4 inline mr-1.5" />, label: 'Search' },
    { key: 'stats', icon: <BarChart3 className="w-4 h-4 inline mr-1.5" />, label: 'Stats' },
  ];

  // Graph tab takes full width
  if (viewMode === 'graph') {
    return (
      <div className="h-full flex flex-col">
        {/* Header */}
        <div className="px-8 pt-8 pb-4">
          <div className="flex items-center justify-between mb-4">
            <div>
              <h1 className="text-2xl font-bold text-white mb-1">Memory System</h1>
              <p className="text-slate-400 text-sm">
                {stats ? `${stats.total_memories} memories | ${stats.daily_log_count} daily | ${stats.long_term_count} long-term` : 'Unified DB-backed memory system'}
              </p>
            </div>
          </div>
          <div className="flex items-center gap-4 border-b border-slate-700 pb-3">
            <div className="flex gap-1">
              {tabs.map((tab) => (
                <button
                  key={tab.key}
                  onClick={() => setViewMode(tab.key)}
                  className={`px-3 py-1.5 rounded text-sm transition-colors ${
                    viewMode === tab.key ? 'bg-stark-500 text-white' : 'text-slate-400 hover:text-white hover:bg-slate-700'
                  }`}
                >
                  {tab.icon}
                  {tab.label}
                </button>
              ))}
            </div>
          </div>
        </div>
        <div className="flex-1 min-h-0">
          <Suspense fallback={
            <div className="flex items-center justify-center h-64">
              <div className="w-6 h-6 border-2 border-stark-500 border-t-transparent rounded-full animate-spin" />
            </div>
          }>
            <MemoryGraph />
          </Suspense>
        </div>
      </div>
    );
  }

  return (
    <div className="p-8">
      {/* Header */}
      <div className="mb-6">
        <div className="flex items-center justify-between mb-4">
          <div>
            <h1 className="text-2xl font-bold text-white mb-1">Memory System</h1>
            <p className="text-slate-400 text-sm">
              {stats ? `${stats.total_memories} memories | ${stats.daily_log_count} daily | ${stats.long_term_count} long-term` : 'Unified DB-backed memory system'}
            </p>
          </div>
        </div>

        <div className="flex items-center gap-4 border-b border-slate-700 pb-3">
          <div className="flex gap-1">
            {tabs.map((tab) => (
              <button
                key={tab.key}
                onClick={() => setViewMode(tab.key)}
                className={`px-3 py-1.5 rounded text-sm transition-colors ${
                  viewMode === tab.key ? 'bg-stark-500 text-white' : 'text-slate-400 hover:text-white hover:bg-slate-700'
                }`}
              >
                {tab.icon}
                {tab.label}
              </button>
            ))}
          </div>

          {/* Mode filter */}
          {viewMode !== 'stats' && (
            <div className="flex items-center gap-2 ml-auto">
              <User className="w-4 h-4 text-slate-500" />
              <select
                value={modeFilter}
                onChange={(e) => setModeFilter(e.target.value)}
                className="bg-slate-800 border border-slate-600 rounded px-2 py-1 text-sm text-white"
              >
                <option value="all">All Memories</option>
                <option value="standard">Standard</option>
                <option value="safemode">Safe Mode</option>
              </select>
            </div>
          )}
        </div>
      </div>

      {error && (
        <div className="mb-6 bg-red-500/20 border border-red-500/50 text-red-400 px-4 py-3 rounded-lg flex items-center justify-between">
          <span>{error}</span>
          <div className="flex items-center gap-3">
            <button onClick={loadData} className="text-red-300 hover:text-white text-sm font-medium">Retry</button>
            <button onClick={() => setError(null)} className="text-red-300/60 hover:text-red-300 text-sm">Dismiss</button>
          </div>
        </div>
      )}

      {/* Stats tab (full width) */}
      {viewMode === 'stats' && (
        <StatsView stats={stats} />
      )}

      {/* Browse/Calendar/Search layouts (two column) */}
      {viewMode !== 'stats' && (
        <div className="grid grid-cols-1 lg:grid-cols-3 gap-6">
          {/* Left panel */}
          <div className="lg:col-span-1">
            {viewMode === 'browse' && (
              <div className="space-y-4">
                {/* Long-term memories */}
                {groupedEntries.longTerm.length > 0 && (
                  <div>
                    <div className="flex items-center gap-2 mb-2">
                      <Brain className="w-4 h-4 text-purple-400" />
                      <span className="text-sm font-medium text-purple-400">Long-term Memory</span>
                      <Button variant="ghost" size="sm" onClick={() => setAddEntryType('long_term')} className="ml-auto text-slate-400 hover:text-white">
                        <Plus className="w-3 h-3" />
                      </Button>
                    </div>
                    <div className="space-y-1">
                      {groupedEntries.longTerm.map((entry, i) => (
                        <button
                          key={`lt-${i}`}
                          onClick={() => handleEntryClick(entry)}
                          className={`w-full text-left px-3 py-2 rounded text-sm transition-colors ${
                            selectedLabel === 'Long-term Memory' ? 'bg-purple-500/20 text-purple-300' : 'text-slate-300 hover:bg-slate-700'
                          }`}
                        >
                          <div className="flex items-center gap-2">
                            <FileTypeIcon type="long_term" />
                            <span>Long-term ({entry.entry_count} entries)</span>
                            <ModeBadge identityId={entry.identity_id} />
                          </div>
                        </button>
                      ))}
                    </div>
                  </div>
                )}

                {/* Daily logs */}
                {groupedEntries.dailyLogs.length > 0 && (
                  <div>
                    <div className="flex items-center gap-2 mb-2">
                      <Calendar className="w-4 h-4 text-blue-400" />
                      <span className="text-sm font-medium text-blue-400">Daily Logs</span>
                      <span className="text-xs text-slate-500">({groupedEntries.dailyLogs.length})</span>
                      <Button variant="ghost" size="sm" onClick={() => setAddEntryType('daily')} className="ml-auto text-slate-400 hover:text-white">
                        <Plus className="w-3 h-3" />
                      </Button>
                    </div>
                    <div className="space-y-1 max-h-96 overflow-y-auto">
                      {groupedEntries.dailyLogs.map((entry) => (
                        <button
                          key={entry.date}
                          onClick={() => handleEntryClick(entry)}
                          className={`w-full text-left px-3 py-2 rounded text-sm transition-colors ${
                            selectedLabel === `Daily Log: ${entry.date}` ? 'bg-blue-500/20 text-blue-300' : 'text-slate-300 hover:bg-slate-700'
                          }`}
                        >
                          <div className="flex items-center gap-2">
                            <FileTypeIcon type="daily_log" />
                            <span>{entry.date}</span>
                            <ModeBadge identityId={entry.identity_id} />
                            <span className="text-xs text-slate-500 ml-auto">{entry.entry_count} entries</span>
                          </div>
                        </button>
                      ))}
                    </div>
                  </div>
                )}

                {filteredEntries.length === 0 && (
                  <Card>
                    <CardContent className="text-center py-8">
                      <Brain className="w-10 h-10 text-slate-600 mx-auto mb-3" />
                      <p className="text-slate-400">No memories yet</p>
                      <p className="text-slate-500 text-sm mt-1">Memories will appear here as they are created</p>
                    </CardContent>
                  </Card>
                )}
              </div>
            )}

            {viewMode === 'calendar' && (
              <div className="space-y-4">
                <CalendarView entries={filteredEntries} selectedDate={selectedDate} onSelectDate={loadDailyLogByDate} />
                <Button variant="secondary" size="sm" onClick={() => setAddEntryType('daily')} className="w-full flex items-center justify-center gap-2">
                  <Plus className="w-4 h-4" />
                  Add to Today's Log
                </Button>
              </div>
            )}

            {viewMode === 'search' && (
              <SearchView identityFilter={identityFilter} onSelectMemory={(id) => {
                // For now, just log. Could expand to show the specific memory.
                console.log('Selected memory:', id);
              }} />
            )}

            {/* Stats summary (below list/calendar) */}
            {viewMode !== 'search' && stats && (
              <Card className="mt-4">
                <CardContent className="py-3">
                  <div className="flex items-center gap-2 text-sm text-slate-400 mb-2">
                    <BarChart3 className="w-4 h-4" />
                    <span>Quick Stats</span>
                  </div>
                  <div className="grid grid-cols-2 gap-2 text-xs">
                    <div>
                      <span className="text-slate-500">Total:</span>{' '}
                      <span className="text-white">{stats.total_memories}</span>
                    </div>
                    <div>
                      <span className="text-slate-500">Daily:</span>{' '}
                      <span className="text-blue-400">{stats.daily_log_count}</span>
                    </div>
                    <div>
                      <span className="text-slate-500">Long-term:</span>{' '}
                      <span className="text-purple-400">{stats.long_term_count}</span>
                    </div>
                    <div>
                      <span className="text-slate-500">Identities:</span>{' '}
                      <span className="text-white">{stats.identity_count}</span>
                    </div>
                  </div>
                </CardContent>
              </Card>
            )}
          </div>

          {/* Right panel: Memory viewer */}
          <div className="lg:col-span-2">
            <Card className="h-full min-h-[400px]">
              <CardContent>
                {loadingContent ? (
                  <div className="flex items-center justify-center h-64">
                    <div className="w-6 h-6 border-2 border-stark-500 border-t-transparent rounded-full animate-spin" />
                  </div>
                ) : selectedMemories.length > 0 ? (
                  <div>
                    <div className="flex items-center gap-3 mb-4 pb-3 border-b border-slate-700">
                      <FileTypeIcon type={selectedLabel?.startsWith('Long') ? 'long_term' : 'daily_log'} />
                      <div>
                        <h2 className="text-lg font-medium text-white">{selectedLabel}</h2>
                        <span className="text-sm text-slate-400">{selectedMemories.length} entries</span>
                      </div>
                    </div>
                    <div className="max-h-[600px] overflow-y-auto space-y-3">
                      {selectedMemories.map((item) => (
                        <MemoryItemView key={item.id} item={item} />
                      ))}
                    </div>
                  </div>
                ) : (
                  <div className="flex flex-col items-center justify-center h-64 text-slate-500">
                    <Brain className="w-12 h-12 mb-3 opacity-50" />
                    <p>Select a memory group to view entries</p>
                    <p className="text-sm mt-1">Or use the calendar to browse daily logs</p>
                  </div>
                )}
              </CardContent>
            </Card>
          </div>
        </div>
      )}

      {/* Add entry modal */}
      {addEntryType && (
        <AddEntryModal
          type={addEntryType}
          identityId={modeFilter === 'safemode' ? 'safemode' : null}
          onClose={() => setAddEntryType(null)}
          onSuccess={() => {
            loadData();
            if (selectedLabel) {
              if (selectedLabel.startsWith('Long')) loadLongTerm();
              else if (selectedDate) loadDailyLogByDate(selectedDate);
            }
          }}
        />
      )}
    </div>
  );
}
