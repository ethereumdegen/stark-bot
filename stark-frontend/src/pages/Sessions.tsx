import { useState, useEffect, useMemo } from 'react';
import { useParams, useNavigate } from 'react-router-dom';
import clsx from 'clsx';
import { Calendar, Trash2, MessageSquare, Download, ChevronLeft, User, Bot, Wrench, CheckCircle, XCircle, AlertCircle, Play, Pause, RefreshCw, Loader2, Terminal } from 'lucide-react';
import Card, { CardContent } from '@/components/ui/Card';
import Button from '@/components/ui/Button';
import { getSessions, getSession, deleteSession, deleteAllSessions, getSessionTranscript, SessionMessage, getCronJobs, CronJobInfo, stopSession, resumeSession } from '@/lib/api';

type CompletionStatus = 'active' | 'complete' | 'cancelled' | 'failed';

interface Session {
  id: number;
  channel_type: string;
  channel_id: number;
  platform_chat_id?: string;
  created_at: string;
  updated_at: string;
  message_count?: number;
  completion_status?: string;
  initial_query?: string;
  safe_mode?: boolean;
  special_role_name?: string;
}

function isValidStatus(status: string | undefined): status is CompletionStatus {
  return status !== undefined && ['active', 'complete', 'cancelled', 'failed'].includes(status);
}

// Extract cron job_id from platform_chat_id (format: "cron:job_id")
function getCronJobId(platformChatId?: string): string | null {
  if (!platformChatId || !platformChatId.startsWith('cron:')) return null;
  return platformChatId.slice(5); // Remove "cron:" prefix
}

const statusConfig: Record<CompletionStatus, { icon: typeof CheckCircle; bg: string; text: string; label: string }> = {
  active: { icon: Play, bg: 'bg-blue-500/20', text: 'text-blue-400', label: 'Active' },
  complete: { icon: CheckCircle, bg: 'bg-green-500/20', text: 'text-green-400', label: 'Complete' },
  cancelled: { icon: XCircle, bg: 'bg-yellow-500/20', text: 'text-yellow-400', label: 'Cancelled' },
  failed: { icon: AlertCircle, bg: 'bg-red-500/20', text: 'text-red-400', label: 'Failed' },
};

export default function Sessions() {
  const { sessionId } = useParams<{ sessionId?: string }>();
  const navigate = useNavigate();
  const [sessions, setSessions] = useState<Session[]>([]);
  const [cronJobs, setCronJobs] = useState<Map<string, CronJobInfo>>(new Map());
  const [selectedSession, setSelectedSession] = useState<Session | null>(null);
  const [messages, setMessages] = useState<SessionMessage[]>([]);
  const [isLoading, setIsLoading] = useState(true);
  const [isLoadingMessages, setIsLoadingMessages] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [successMessage, setSuccessMessage] = useState<string | null>(null);
  const [isRefreshing, setIsRefreshing] = useState(false);
  const [isDeletingAll, setIsDeletingAll] = useState(false);
  const [typeFilter, setTypeFilter] = useState<string>('all');
  const [modeFilter, setModeFilter] = useState<string>('all');

  useEffect(() => {
    loadSessions();
  }, []);

  // Clear selected session when navigating back to index (e.g. via sidebar nav)
  useEffect(() => {
    if (!sessionId) {
      setSelectedSession(null);
      setMessages([]);
    }
  }, [sessionId]);

  // Auto-load session from URL params
  useEffect(() => {
    if (sessionId && !isLoading && !selectedSession) {
      const id = parseInt(sessionId, 10);
      // First try to find in cached list
      const session = sessions.find(s => s.id === id);
      if (session) {
        loadTranscript(session);
      } else {
        // Session not in list (e.g., older than top 100), fetch directly
        loadSessionById(id);
      }
    }
  }, [sessionId, sessions, isLoading]);

  // Polling for new messages when viewing a session
  useEffect(() => {
    if (!selectedSession) return;

    const pollInterval = setInterval(async () => {
      try {
        const transcript = await getSessionTranscript(selectedSession.id);
        setMessages(transcript.messages);
      } catch (err) {
        // Silently fail on poll errors to avoid spamming the user
        console.error('Poll refresh failed:', err);
      }
    }, 5000);

    return () => clearInterval(pollInterval);
  }, [selectedSession]);

  // Polling for sessions list refresh (every 10s when on list view)
  useEffect(() => {
    if (selectedSession) return;

    const pollInterval = setInterval(async () => {
      try {
        const [data, jobs] = await Promise.all([
          getSessions(),
          getCronJobs().catch(() => []),
        ]);
        const sorted = data
          .sort((a, b) => new Date(b.updated_at).getTime() - new Date(a.updated_at).getTime())
          .slice(0, 100);
        setSessions(sorted);
        const jobMap = new Map<string, CronJobInfo>();
        jobs.forEach(job => jobMap.set(job.job_id, job));
        setCronJobs(jobMap);
      } catch (err) {
        console.error('Sessions list poll failed:', err);
      }
    }, 10000);

    return () => clearInterval(pollInterval);
  }, [selectedSession]);

  const loadSessions = async () => {
    try {
      const [data, jobs] = await Promise.all([
        getSessions(),
        getCronJobs().catch(() => []), // Don't fail if cron jobs can't be fetched
      ]);
      // Sort by updated_at desc and limit to 100
      const sorted = data
        .sort((a, b) => new Date(b.updated_at).getTime() - new Date(a.updated_at).getTime())
        .slice(0, 100);
      setSessions(sorted);
      // Build a map of cron job_id (UUID) -> info for quick lookup
      // This uses the job_id (UUID string), not the database id, because
      // sessions reference cron jobs via platform_chat_id like "cron:job_id"
      const jobMap = new Map<string, CronJobInfo>();
      jobs.forEach(job => jobMap.set(job.job_id, job));
      setCronJobs(jobMap);
    } catch (err) {
      setError('Failed to load sessions');
    } finally {
      setIsLoading(false);
    }
  };

  const loadTranscript = async (session: Session) => {
    setSelectedSession(session);
    setIsLoadingMessages(true);
    setError(null);
    try {
      const transcript = await getSessionTranscript(session.id);
      setMessages(transcript.messages);
    } catch (err) {
      setError('Failed to load transcript');
      setMessages([]);
    } finally {
      setIsLoadingMessages(false);
    }
  };

  const loadSessionById = async (id: number) => {
    setIsLoadingMessages(true);
    setError(null);
    try {
      const session = await getSession(id);
      setSelectedSession(session);
      const transcript = await getSessionTranscript(id);
      setMessages(transcript.messages);
    } catch (err) {
      setError('Session not found');
      setMessages([]);
    } finally {
      setIsLoadingMessages(false);
    }
  };

  const refreshTranscript = async () => {
    if (!selectedSession || isRefreshing) return;
    setIsRefreshing(true);
    try {
      const transcript = await getSessionTranscript(selectedSession.id);
      setMessages(transcript.messages);
    } catch (err) {
      setError('Failed to refresh messages');
    } finally {
      setIsRefreshing(false);
    }
  };

  const handleDelete = async (id: number, e: React.MouseEvent) => {
    e.stopPropagation(); // Prevent triggering the card click

    const confirmed = confirm(
      'Force Delete Session?\n\n' +
      'This will:\n' +
      '• Delete the session and all messages\n' +
      '• Cancel any running AI agents/tasks for this session\n' +
      '• Stop any cron jobs using this session\n\n' +
      'This action cannot be undone.'
    );
    if (!confirmed) return;

    setError(null);
    setSuccessMessage(null);

    try {
      const result = await deleteSession(String(id));
      setSessions((prev) => prev.filter((s) => s.id !== id));

      // Show success message with cancelled agents count
      if (result.cancelled_agents && result.cancelled_agents > 0) {
        setSuccessMessage(`Session deleted. Cancelled ${result.cancelled_agents} running agent(s).`);
      } else {
        setSuccessMessage('Session deleted successfully.');
      }

      // Auto-hide success message after 5 seconds
      setTimeout(() => setSuccessMessage(null), 5000);
    } catch (err) {
      setError('Failed to delete session');
    }
  };

  const handleDeleteAll = async () => {
    if (sessions.length === 0) return;

    const confirmed = confirm(
      `Delete ALL ${sessions.length} Sessions?\n\n` +
      'This will:\n' +
      '• Delete all sessions and their messages\n' +
      '• Cancel any running AI agents/tasks\n' +
      '• Stop any cron jobs using these sessions\n\n' +
      'This action cannot be undone.'
    );
    if (!confirmed) return;

    setIsDeletingAll(true);
    setError(null);
    setSuccessMessage(null);

    try {
      const result = await deleteAllSessions();
      setSessions([]);

      const agentMsg = result.cancelled_agents > 0 ? ` Cancelled ${result.cancelled_agents} running agent(s).` : '';
      setSuccessMessage(`Deleted ${result.deleted_count} session(s).${agentMsg}`);
      setTimeout(() => setSuccessMessage(null), 5000);
    } catch (err) {
      setError('Failed to delete all sessions');
    } finally {
      setIsDeletingAll(false);
    }
  };

  const handleToggleStatus = async (session: Session, e: React.MouseEvent) => {
    e.stopPropagation(); // Prevent triggering the card click

    setError(null);
    setSuccessMessage(null);

    try {
      const isActive = session.completion_status === 'active';

      if (isActive) {
        // Stop the session
        const result = await stopSession(session.id);
        if (result.success) {
          setSessions((prev) => prev.map((s) =>
            s.id === session.id
              ? { ...s, completion_status: 'cancelled' }
              : s
          ));
          const agentMsg = result.cancelled_agents && result.cancelled_agents > 0
            ? ` Cancelled ${result.cancelled_agents} running agent(s).`
            : '';
          setSuccessMessage(`Session stopped.${agentMsg}`);
        } else {
          setError(result.error || 'Failed to stop session');
        }
      } else {
        // Resume the session (for cancelled or failed)
        const result = await resumeSession(session.id);
        if (result.success) {
          setSessions((prev) => prev.map((s) =>
            s.id === session.id
              ? { ...s, completion_status: 'active' }
              : s
          ));
          setSuccessMessage('Session resumed.');
        } else {
          setError(result.error || 'Failed to resume session');
        }
      }

      // Auto-hide success message after 3 seconds
      setTimeout(() => setSuccessMessage(null), 3000);
    } catch (err) {
      setError('Failed to update session status');
    }
  };

  const formatDate = (dateStr: string) => {
    return new Date(dateStr).toLocaleString();
  };

  const formatShortDate = (dateStr: string) => {
    const date = new Date(dateStr);
    return date.toLocaleDateString() + ' ' + date.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });
  };

  const exportAsMarkdown = () => {
    if (!selectedSession || messages.length === 0) return;

    let md = `# Chat Session - ${selectedSession.channel_type} (Session ${selectedSession.id})\n\n`;
    md += `**Created:** ${formatDate(selectedSession.created_at)}\n`;
    md += `**Last Updated:** ${formatDate(selectedSession.updated_at)}\n\n`;
    md += `---\n\n`;

    messages.forEach((msg) => {
      const roleEmoji = msg.role === 'user' ? '**User**' : '**Assistant**';
      md += `### ${roleEmoji}\n`;
      md += `*${formatShortDate(msg.created_at)}*\n\n`;
      md += `${msg.content}\n\n`;
      md += `---\n\n`;
    });

    downloadFile(md, `chat-session-${selectedSession.id}.md`, 'text/markdown');
  };

  const exportAsText = () => {
    if (!selectedSession || messages.length === 0) return;

    let txt = `Chat Session - ${selectedSession.channel_type} (Session ${selectedSession.id})\n`;
    txt += `${'='.repeat(60)}\n\n`;
    txt += `Created: ${formatDate(selectedSession.created_at)}\n`;
    txt += `Last Updated: ${formatDate(selectedSession.updated_at)}\n\n`;
    txt += `${'-'.repeat(60)}\n\n`;

    messages.forEach((msg) => {
      const role = msg.role === 'user' ? 'USER' : 'ASSISTANT';
      txt += `[${role}] ${formatShortDate(msg.created_at)}\n`;
      txt += `${msg.content}\n\n`;
      txt += `${'-'.repeat(60)}\n\n`;
    });

    downloadFile(txt, `chat-session-${selectedSession.id}.txt`, 'text/plain');
  };

  const downloadFile = (content: string, filename: string, mimeType: string) => {
    const blob = new Blob([content], { type: mimeType });
    const url = URL.createObjectURL(blob);
    const a = document.createElement('a');
    a.href = url;
    a.download = filename;
    document.body.appendChild(a);
    a.click();
    document.body.removeChild(a);
    URL.revokeObjectURL(url);
  };

  const availableTypes = useMemo(() => {
    const types = new Set(sessions.map(s => s.channel_type.toLowerCase()));
    return Array.from(types).sort();
  }, [sessions]);

  const filteredSessions = useMemo(() => {
    return sessions.filter(session => {
      if (typeFilter !== 'all' && session.channel_type.toLowerCase() !== typeFilter) return false;
      if (modeFilter === 'safe' && session.safe_mode !== true) return false;
      if (modeFilter === 'standard' && session.safe_mode === true) return false;
      return true;
    });
  }, [sessions, typeFilter, modeFilter]);

  if (isLoading) {
    return (
      <div className="p-4 sm:p-8 flex items-center justify-center">
        <div className="flex items-center gap-3">
          <div className="w-6 h-6 border-2 border-stark-500 border-t-transparent rounded-full animate-spin" />
          <span className="text-slate-400">Loading sessions...</span>
        </div>
      </div>
    );
  }

  // Loading a session directly by URL (not in cached list)
  if (isLoadingMessages && !selectedSession && sessionId) {
    return (
      <div className="p-4 sm:p-8 flex items-center justify-center">
        <div className="flex items-center gap-3">
          <div className="w-6 h-6 border-2 border-stark-500 border-t-transparent rounded-full animate-spin" />
          <span className="text-slate-400">Loading session...</span>
        </div>
      </div>
    );
  }

  // Session not found (tried to load by URL but failed)
  if (error && !selectedSession && sessionId) {
    return (
      <div className="p-4 sm:p-8">
        <Button
          variant="ghost"
          size="sm"
          onClick={() => {
            setError(null);
            navigate('/sessions');
          }}
          className="mb-4"
        >
          <ChevronLeft className="w-4 h-4 mr-1" />
          Back to sessions
        </Button>
        <Card>
          <CardContent className="text-center py-12">
            <AlertCircle className="w-12 h-12 text-red-400 mx-auto mb-4" />
            <p className="text-slate-400">{error}</p>
          </CardContent>
        </Card>
      </div>
    );
  }

  // Session detail view
  if (selectedSession) {
    return (
      <div className="p-4 sm:p-8">
        <div className="mb-6">
          <Button
            variant="ghost"
            size="sm"
            onClick={() => {
              setSelectedSession(null);
              navigate('/sessions');
            }}
            className="mb-4"
          >
            <ChevronLeft className="w-4 h-4 mr-1" />
            Back to sessions
          </Button>
          <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-4">
            <div>
              <div className="flex items-center gap-2 mb-1">
                <h1 className="text-xl sm:text-2xl font-bold text-white">
                  {selectedSession.channel_type} - Session {selectedSession.id}
                </h1>
                {selectedSession.channel_type === 'dev_chat' && (
                  <span className="text-xs px-2 py-1 bg-purple-500/20 text-purple-400 rounded-full flex items-center gap-1">
                    <Terminal className="w-3 h-3" />
                    Dev Chat
                  </span>
                )}
                {selectedSession.safe_mode && (
                  <span className="text-xs px-2 py-1 bg-orange-500/20 text-orange-400 rounded-full flex items-center gap-1">
                    <AlertCircle className="w-3 h-3" />
                    Safe Mode
                  </span>
                )}
                {selectedSession.special_role_name && (
                  <span className="text-xs px-2 py-1 bg-cyan-500/20 text-cyan-400 rounded-full flex items-center gap-1">
                    {selectedSession.special_role_name}
                  </span>
                )}
              </div>
              {selectedSession.channel_type === 'cron' && (() => {
                const jobId = getCronJobId(selectedSession.platform_chat_id);
                const cronJob = jobId ? cronJobs.get(jobId) : null;
                return cronJob && (
                  <p className="text-slate-300 text-sm mb-1">
                    {cronJob.name}
                    {cronJob.description && (
                      <span className="text-slate-500"> - {cronJob.description}</span>
                    )}
                  </p>
                );
              })()}
              <p className="text-slate-400 text-sm">
                {formatDate(selectedSession.created_at)} - {messages.length} messages
              </p>
            </div>
            <div className="flex gap-2">
              <Button
                variant="secondary"
                size="sm"
                onClick={refreshTranscript}
                disabled={isRefreshing}
                title="Refresh messages (auto-refreshes every 5s)"
              >
                <RefreshCw className={`w-4 h-4 sm:mr-1 ${isRefreshing ? 'animate-spin' : ''}`} />
                <span className="hidden sm:inline">Refresh</span>
              </Button>
              <Button
                variant="secondary"
                size="sm"
                onClick={exportAsMarkdown}
                disabled={messages.length === 0}
              >
                <Download className="w-4 h-4 sm:mr-1" />
                <span className="hidden sm:inline">Export</span> MD
              </Button>
              <Button
                variant="secondary"
                size="sm"
                onClick={exportAsText}
                disabled={messages.length === 0}
              >
                <Download className="w-4 h-4 sm:mr-1" />
                <span className="hidden sm:inline">Export</span> TXT
              </Button>
            </div>
          </div>
        </div>

        {error && (
          <div className="mb-6 bg-red-500/20 border border-red-500/50 text-red-400 px-4 py-3 rounded-lg">
            {error}
          </div>
        )}

        {isLoadingMessages ? (
          <div className="flex items-center justify-center py-12">
            <div className="flex items-center gap-3">
              <div className="w-6 h-6 border-2 border-stark-500 border-t-transparent rounded-full animate-spin" />
              <span className="text-slate-400">Loading messages...</span>
            </div>
          </div>
        ) : messages.length > 0 ? (
          <div className="space-y-4">
            {messages.map((msg) => {
              const roleConfig = {
                user: { icon: User, bg: 'bg-blue-500/20', border: 'border-blue-500/30', iconColor: 'text-blue-400', label: 'User' },
                assistant: { icon: Bot, bg: 'bg-stark-500/20', border: 'border-stark-500/30', iconColor: 'text-stark-400', label: 'Assistant' },
                tool_call: { icon: Wrench, bg: 'bg-amber-500/20', border: 'border-amber-500/30', iconColor: 'text-amber-400', label: 'Tool Call' },
                tool_result: { icon: CheckCircle, bg: 'bg-green-500/20', border: 'border-green-500/30', iconColor: 'text-green-400', label: 'Tool Result' },
                system: { icon: Bot, bg: 'bg-slate-500/20', border: 'border-slate-500/30', iconColor: 'text-slate-400', label: 'System' },
              }[msg.role] || { icon: Bot, bg: 'bg-slate-500/20', border: 'border-slate-500/30', iconColor: 'text-slate-400', label: msg.role };

              const IconComponent = roleConfig.icon;

              return (
                <Card key={msg.id} className={roleConfig.border}>
                  <CardContent>
                    <div className="flex gap-3">
                      <div className={`p-2 rounded-lg ${roleConfig.bg}`}>
                        <IconComponent className={`w-5 h-5 ${roleConfig.iconColor}`} />
                      </div>
                      <div className="flex-1 min-w-0">
                        <div className="flex items-center gap-2 mb-1">
                          <span className="font-medium text-white">
                            {roleConfig.label}
                          </span>
                          <span className="text-xs text-slate-500">
                            {formatShortDate(msg.created_at)}
                          </span>
                        </div>
                        <div className="text-slate-300 whitespace-pre-wrap break-words">
                          {msg.content}
                        </div>
                      </div>
                    </div>
                  </CardContent>
                </Card>
              );
            })}
          </div>
        ) : (
          <Card>
            <CardContent className="text-center py-12">
              <MessageSquare className="w-12 h-12 text-slate-600 mx-auto mb-4" />
              <p className="text-slate-400">No messages in this session</p>
            </CardContent>
          </Card>
        )}
      </div>
    );
  }

  // Sessions list view
  return (
    <div className="p-4 sm:p-8">
      <div className="mb-6 sm:mb-8 flex flex-col sm:flex-row sm:items-start sm:justify-between gap-4">
        <div>
          <h1 className="text-xl sm:text-2xl font-bold text-white mb-1 sm:mb-2">Chat Sessions</h1>
          <p className="text-sm sm:text-base text-slate-400">View conversation history, export transcripts, or delete sessions</p>
        </div>
        {sessions.length > 0 && (
          <Button
            variant="secondary"
            size="sm"
            onClick={handleDeleteAll}
            disabled={isDeletingAll}
            className="text-red-400 hover:text-red-300 hover:bg-red-500/20 border-red-500/30 self-start"
          >
            {isDeletingAll ? (
              <>
                <div className="w-4 h-4 border-2 border-red-400 border-t-transparent rounded-full animate-spin mr-2" />
                Deleting...
              </>
            ) : (
              <>
                <Trash2 className="w-4 h-4 mr-2" />
                Delete All
              </>
            )}
          </Button>
        )}
      </div>

      {error && (
        <div className="mb-6 bg-red-500/20 border border-red-500/50 text-red-400 px-4 py-3 rounded-lg">
          {error}
        </div>
      )}

      {successMessage && (
        <div className="mb-6 bg-green-500/20 border border-green-500/50 text-green-400 px-4 py-3 rounded-lg flex items-center gap-2">
          <CheckCircle className="w-5 h-5" />
          {successMessage}
        </div>
      )}

      {sessions.length > 0 ? (
        <>
          {/* Filters */}
          <div className="mb-4 space-y-2">
            <div className="flex items-center gap-2 flex-wrap">
              <span className="text-sm text-slate-400">Type:</span>
              <button
                onClick={() => setTypeFilter('all')}
                className={clsx(
                  'px-3 py-1 rounded-full text-sm transition-colors',
                  typeFilter === 'all'
                    ? 'bg-stark-500 text-white'
                    : 'bg-slate-700 text-slate-300 hover:bg-slate-600'
                )}
              >
                All
              </button>
              {availableTypes.map((type) => (
                <button
                  key={type}
                  onClick={() => setTypeFilter(type)}
                  className={clsx(
                    'px-3 py-1 rounded-full text-sm transition-colors',
                    typeFilter === type
                      ? 'bg-stark-500 text-white'
                      : 'bg-slate-700 text-slate-300 hover:bg-slate-600'
                  )}
                >
                  {type.charAt(0).toUpperCase() + type.slice(1)}
                </button>
              ))}
            </div>
            <div className="flex items-center gap-2 flex-wrap">
              <span className="text-sm text-slate-400">Mode:</span>
              {(['all', 'standard', 'safe'] as const).map((mode) => (
                <button
                  key={mode}
                  onClick={() => setModeFilter(mode)}
                  className={clsx(
                    'px-3 py-1 rounded-full text-sm transition-colors',
                    modeFilter === mode
                      ? 'bg-stark-500 text-white'
                      : 'bg-slate-700 text-slate-300 hover:bg-slate-600'
                  )}
                >
                  {mode === 'all' ? 'All' : mode === 'safe' ? 'Safe Mode' : 'Standard'}
                </button>
              ))}
            </div>
            {(typeFilter !== 'all' || modeFilter !== 'all') && (
              <p className="text-xs text-slate-500">
                Showing {filteredSessions.length} of {sessions.length}
              </p>
            )}
          </div>

          {filteredSessions.length > 0 ? (
          <div className="space-y-3">
          {filteredSessions.map((session) => (
            <Card
              key={session.id}
              className="cursor-pointer hover:border-stark-500/50 transition-colors"
              onClick={() => navigate(`/sessions/${session.id}`)}
            >
              <CardContent>
                {/* Mobile: stacked layout, Desktop: side by side */}
                <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-3">
                  {/* Main content */}
                  <div className="flex items-start sm:items-center gap-2 sm:gap-4 min-w-0">
                    {/* Icon - smaller on mobile */}
                    <div className="p-1.5 sm:p-3 bg-blue-500/20 rounded-lg shrink-0">
                      <Calendar className="w-3.5 h-3.5 sm:w-6 sm:h-6 text-blue-400" />
                    </div>
                    <div className="min-w-0 flex-1">
                      {/* Title row with type and status */}
                      <div className="flex items-center gap-2 flex-wrap">
                        <h3 className="font-semibold text-white text-sm sm:text-base">
                          {session.channel_type}
                        </h3>
                        <span className="text-xs px-1.5 py-0.5 bg-slate-700 text-slate-400 rounded">
                          #{session.id}
                        </span>
                        <span className="hidden sm:inline text-xs font-mono px-2 py-0.5 bg-slate-700/50 text-slate-300 rounded">
                          {session.id.toString(16).padStart(8, '0')}
                        </span>
                        {isValidStatus(session.completion_status) && (() => {
                          const config = statusConfig[session.completion_status];
                          const StatusIcon = config.icon;
                          return (
                            <span className={`text-xs px-1.5 sm:px-2 py-0.5 ${config.bg} ${config.text} rounded-full flex items-center gap-1`}>
                              <StatusIcon className="w-3 h-3" />
                              <span className="hidden sm:inline">{config.label}</span>
                            </span>
                          );
                        })()}
                        {session.channel_type === 'dev_chat' && (
                          <span className="text-xs px-1.5 sm:px-2 py-0.5 bg-purple-500/20 text-purple-400 rounded-full flex items-center gap-1">
                            <Terminal className="w-3 h-3" />
                            <span className="hidden sm:inline">Dev Chat</span>
                          </span>
                        )}
                        {session.safe_mode && (
                          <span className="text-xs px-1.5 sm:px-2 py-0.5 bg-orange-500/20 text-orange-400 rounded-full flex items-center gap-1">
                            <AlertCircle className="w-3 h-3" />
                            <span className="hidden sm:inline">Safe Mode</span>
                          </span>
                        )}
                        {session.special_role_name && (
                          <span className="text-xs px-1.5 sm:px-2 py-0.5 bg-cyan-500/20 text-cyan-400 rounded-full flex items-center gap-1">
                            <span className="hidden sm:inline">{session.special_role_name}</span>
                            <span className="sm:hidden">SR</span>
                          </span>
                        )}
                      </div>
                      {/* Cron job info */}
                      {session.channel_type === 'cron' && (() => {
                        const jobId = getCronJobId(session.platform_chat_id);
                        const cronJob = jobId ? cronJobs.get(jobId) : null;
                        return cronJob && (
                          <p className="text-xs text-slate-500 mt-0.5 truncate">
                            {cronJob.name}
                            <span className="hidden sm:inline text-slate-600">
                              {cronJob.description && ` - ${cronJob.description}`}
                            </span>
                          </p>
                        );
                      })()}
                      {/* Web session initial query */}
                      {session.channel_type === 'web' && session.initial_query && (
                        <p className="text-xs text-slate-500 mt-0.5 truncate">
                          {session.initial_query}
                        </p>
                      )}
                      {/* Metadata row */}
                      <div className="flex items-center gap-2 sm:gap-4 mt-1 text-xs sm:text-sm text-slate-400">
                        <span className="truncate">{formatShortDate(session.updated_at)}</span>
                        {session.message_count !== undefined && (
                          <span className="flex items-center gap-1 shrink-0">
                            <MessageSquare className="w-3 h-3" />
                            {session.message_count}
                          </span>
                        )}
                      </div>
                    </div>
                  </div>
                  {/* Action buttons */}
                  <div className="flex items-center gap-1 sm:gap-2 self-end sm:self-center shrink-0">
                    {/* Spinning indicator for active sessions */}
                    {session.completion_status === 'active' && (
                      <Loader2 className="w-4 h-4 text-blue-400 animate-spin" />
                    )}
                    {/* Play/Pause button - don't show for completed sessions */}
                    {session.completion_status !== 'complete' && (
                      <Button
                        variant="ghost"
                        size="sm"
                        onClick={(e) => handleToggleStatus(session, e)}
                        className={session.completion_status === 'active'
                          ? "text-yellow-400 hover:text-yellow-300 hover:bg-yellow-500/20 p-1.5 sm:p-2"
                          : "text-green-400 hover:text-green-300 hover:bg-green-500/20 p-1.5 sm:p-2"
                        }
                        title={session.completion_status === 'active'
                          ? "Stop session and cancel running agents"
                          : "Resume session"
                        }
                      >
                        {session.completion_status === 'active' ? (
                          <Pause className="w-4 h-4" />
                        ) : (
                          <Play className="w-4 h-4" />
                        )}
                      </Button>
                    )}
                    <Button
                      variant="ghost"
                      size="sm"
                      onClick={(e) => handleDelete(session.id, e)}
                      className="text-red-400 hover:text-red-300 hover:bg-red-500/20 p-1.5 sm:p-2"
                      title="Force delete session and cancel running agents"
                    >
                      <Trash2 className="w-4 h-4" />
                    </Button>
                    <ChevronLeft className="w-4 h-4 sm:w-5 sm:h-5 text-slate-500 rotate-180" />
                  </div>
                </div>
              </CardContent>
            </Card>
          ))}
        </div>
          ) : (
            <Card>
              <CardContent className="text-center py-12">
                <Calendar className="w-12 h-12 text-slate-600 mx-auto mb-4" />
                <p className="text-slate-400">No sessions match the selected filters</p>
              </CardContent>
            </Card>
          )}
        </>
      ) : (
        <Card>
          <CardContent className="text-center py-12">
            <Calendar className="w-12 h-12 text-slate-600 mx-auto mb-4" />
            <p className="text-slate-400">No sessions found</p>
          </CardContent>
        </Card>
      )}
    </div>
  );
}
