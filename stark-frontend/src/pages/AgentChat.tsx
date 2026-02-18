import { useState, useEffect, useRef, useCallback, KeyboardEvent } from 'react';
import { useNavigate, useSearchParams } from 'react-router-dom';
import { Send, RotateCcw, Copy, Check, Wallet, Bug, Square, Loader2, ChevronDown, CheckCircle, Circle, ExternalLink, Wrench } from 'lucide-react';
import Button from '@/components/ui/Button';
import ChatMessage from '@/components/chat/ChatMessage';
import TypingIndicator from '@/components/chat/TypingIndicator';
import ExecutionProgress from '@/components/chat/ExecutionProgress';
import DebugPanel from '@/components/chat/DebugPanel';
import CommandAutocomplete from '@/components/chat/CommandAutocomplete';
import CommandMenu from '@/components/chat/CommandMenu';
import TransactionTracker from '@/components/chat/TransactionTracker';
import { ConfirmationPrompt } from '@/components/chat/ConfirmationPrompt';
import TxQueueConfirmationModal, { TxQueueTransaction } from '@/components/chat/TxQueueConfirmationModal';
import SubagentBadge from '@/components/chat/SubagentBadge';
import { Subagent, SubagentStatus } from '@/lib/subagent-types';
import { useGateway } from '@/hooks/useGateway';
import { useWallet, SUPPORTED_NETWORKS, type SupportedNetwork } from '@/hooks/useWallet';
import { sendChatMessage, getAgentSettings, getSkills, getTools, confirmTransaction, cancelTransaction, stopExecution, listSubagents, getActiveWebSession, getSessionTranscript, getExecutionStatus, createNewWebSession, getPlannerTasks, getAgentSubtypes, AgentSubtypeInfo } from '@/lib/api';
import { Command, COMMAND_DEFINITIONS, getAllCommands } from '@/lib/commands';
import type { ChatMessage as ChatMessageType, MessageRole, SlashCommand, TrackedTransaction, TxPendingEvent, TxConfirmedEvent, PendingConfirmation, ConfirmationRequiredEvent, PlannerTask, TaskQueueUpdateEvent, TaskStatusChangeEvent } from '@/types';

interface ConversationMessage {
  role: string;
  content: string;
}

// localStorage keys for persistence
const STORAGE_KEY_MESSAGES = 'agentChat_messages';
const STORAGE_KEY_HISTORY = 'agentChat_history';
const STORAGE_KEY_MODE = 'agentChat_mode';
const STORAGE_KEY_SUBTYPE = 'agentChat_subtype';
const STORAGE_KEY_SESSION_ID = 'agentChat_sessionId';

// Web channel ID - must match backend WEB_CHANNEL_ID
const WEB_CHANNEL_ID = 0;

// Helper to check if an event is for the web channel
function isWebChannelEvent(data: unknown): boolean {
  if (typeof data !== 'object' || data === null) return true; // Allow events without channel_id
  const event = data as { channel_id?: number };
  // Accept events with no channel_id (legacy) or channel_id === 0 (web channel)
  return event.channel_id === undefined || event.channel_id === WEB_CHANNEL_ID;
}

// Helper to check if an event is for the current session
// This filters out events from other browser tabs/sessions
function isCurrentSessionEvent(data: unknown, currentDbSessionId: number | null): boolean {
  if (typeof data !== 'object' || data === null) return true; // Allow events without session_id
  const event = data as { channel_id?: number; session_id?: number };

  // First check channel_id (must be web channel or undefined)
  if (event.channel_id !== undefined && event.channel_id !== WEB_CHANNEL_ID) {
    return false;
  }

  // If no session_id in event (legacy) or no current session, allow the event
  if (event.session_id === undefined || currentDbSessionId === null) {
    return true;
  }

  // Check if session_id matches current session
  return event.session_id === currentDbSessionId;
}

// Color palette for agent subtypes (cycles through these for dynamic subtypes)
const SUBTYPE_COLORS = [
  { bgClass: 'bg-amber-500/20', textClass: 'text-amber-400', borderClass: 'border-amber-500/50', hoverClass: 'hover:bg-amber-500/30' },
  { bgClass: 'bg-purple-500/20', textClass: 'text-purple-400', borderClass: 'border-purple-500/50', hoverClass: 'hover:bg-purple-500/30' },
  { bgClass: 'bg-cyan-500/20', textClass: 'text-cyan-400', borderClass: 'border-cyan-500/50', hoverClass: 'hover:bg-cyan-500/30' },
  { bgClass: 'bg-pink-500/20', textClass: 'text-pink-400', borderClass: 'border-pink-500/50', hoverClass: 'hover:bg-pink-500/30' },
  { bgClass: 'bg-green-500/20', textClass: 'text-green-400', borderClass: 'border-green-500/50', hoverClass: 'hover:bg-green-500/30' },
  { bgClass: 'bg-orange-500/20', textClass: 'text-orange-400', borderClass: 'border-orange-500/50', hoverClass: 'hover:bg-orange-500/30' },
];

// Generate a new session ID
function generateSessionId(): string {
  return crypto.randomUUID();
}

// Helper to safely parse JSON from localStorage
function loadFromStorage<T>(key: string, fallback: T): T {
  try {
    const stored = localStorage.getItem(key);
    if (!stored) return fallback;
    const parsed = JSON.parse(stored);
    // Restore Date objects for messages
    if (key === STORAGE_KEY_MESSAGES && Array.isArray(parsed)) {
      return parsed.map((m: ChatMessageType) => ({
        ...m,
        timestamp: new Date(m.timestamp),
      })) as T;
    }
    return parsed;
  } catch {
    return fallback;
  }
}

export default function AgentChat() {
  // Load persisted state from localStorage
  const [messages, setMessages] = useState<ChatMessageType[]>(() =>
    loadFromStorage<ChatMessageType[]>(STORAGE_KEY_MESSAGES, [])
  );
  const [input, setInput] = useState('');
  const [isLoading, setIsLoading] = useState(false);
  const [activeExecutionId, setActiveExecutionId] = useState<string | null>(null);
  const [isStopping, setIsStopping] = useState(false);
  const [showAutocomplete, setShowAutocomplete] = useState(false);
  const [selectedCommandIndex, setSelectedCommandIndex] = useState(0);
  const [debugMode, setDebugMode] = useState(false);
  const [sessionStartTime] = useState(new Date());
  const [copied, setCopied] = useState(false);
  const [trackedTxs, setTrackedTxs] = useState<TrackedTransaction[]>([]);
  const [pendingConfirmation, setPendingConfirmation] = useState<PendingConfirmation | null>(null);
  const [txQueueConfirmation, setTxQueueConfirmation] = useState<TxQueueTransaction | null>(null);
  const [subagents, setSubagents] = useState<Subagent[]>([]);
  const [plannerTasks, setPlannerTasks] = useState<PlannerTask[]>([]);
  const [cronExecutionActive, setCronExecutionActive] = useState<{
    job_id: string;
    job_name: string;
  } | null>(null);
  const [agentMode, setAgentMode] = useState<{ mode: string; label: string } | null>(() =>
    loadFromStorage<{ mode: string; label: string } | null>(STORAGE_KEY_MODE, null)
  );
  const [agentSubtype, setAgentSubtype] = useState<{ subtype: string; label: string } | null>(() =>
    loadFromStorage<{ subtype: string; label: string } | null>(STORAGE_KEY_SUBTYPE, null)
  );
  const [availableSubtypes, setAvailableSubtypes] = useState<AgentSubtypeInfo[]>([]);
  const [subtypeDropdownOpen, setSubtypeDropdownOpen] = useState(false);
  const subtypeDropdownRef = useRef<HTMLDivElement>(null);
  const [networkDropdownOpen, setNetworkDropdownOpen] = useState(false);
  const networkDropdownRef = useRef<HTMLDivElement>(null);
  const [sessionId, setSessionId] = useState<string>(() => {
    const stored = localStorage.getItem(STORAGE_KEY_SESSION_ID);
    if (stored) return stored;
    const newId = generateSessionId();
    localStorage.setItem(STORAGE_KEY_SESSION_ID, newId);
    return newId;
  });
  const [dbSessionId, setDbSessionId] = useState<number | null>(null);
  const [historyLoaded, setHistoryLoaded] = useState(false);

  const messagesEndRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLTextAreaElement>(null);
  // Track spawn message IDs so we can update them when session_ready fires
  const subagentSpawnMsgIds = useRef<Map<string, string>>(new Map());
  const navigate = useNavigate();
  const [searchParams, setSearchParams] = useSearchParams();
  const { connected, on, off } = useGateway();
  const { address, usdcBalance, isConnected: walletConnected, isLoading: walletLoading, error: walletError, isCorrectNetwork, currentNetwork, switchNetwork, walletMode } = useWallet();

  // Persist messages to localStorage
  useEffect(() => {
    localStorage.setItem(STORAGE_KEY_MESSAGES, JSON.stringify(messages));
  }, [messages]);

  // Persist agent mode to localStorage
  useEffect(() => {
    if (agentMode) {
      localStorage.setItem(STORAGE_KEY_MODE, JSON.stringify(agentMode));
    }
  }, [agentMode]);

  // Persist agent subtype to localStorage
  useEffect(() => {
    if (agentSubtype) {
      localStorage.setItem(STORAGE_KEY_SUBTYPE, JSON.stringify(agentSubtype));
    }
  }, [agentSubtype]);

  // Load available subtypes from API
  useEffect(() => {
    getAgentSubtypes()
      .then(subtypes => setAvailableSubtypes(subtypes.filter(s => s.enabled).sort((a, b) => a.sort_order - b.sort_order)))
      .catch(err => console.error('[AgentChat] Failed to load subtypes:', err));
  }, []);

  // Pre-fill input from ?message= query param
  useEffect(() => {
    const message = searchParams.get('message');
    if (message) {
      setInput(message);
      searchParams.delete('message');
      setSearchParams(searchParams, { replace: true });
      setTimeout(() => inputRef.current?.focus(), 100);
    }
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  // Close dropdowns when clicking outside
  useEffect(() => {
    const handleClickOutside = (event: MouseEvent) => {
      if (subtypeDropdownRef.current && !subtypeDropdownRef.current.contains(event.target as Node)) {
        setSubtypeDropdownOpen(false);
      }
      if (networkDropdownRef.current && !networkDropdownRef.current.contains(event.target as Node)) {
        setNetworkDropdownOpen(false);
      }
    };
    document.addEventListener('mousedown', handleClickOutside);
    return () => document.removeEventListener('mousedown', handleClickOutside);
  }, []);

  // Helper to truncate address
  const truncateAddress = (addr: string) => `${addr.slice(0, 6)}...${addr.slice(-4)}`;

  // Copy address to clipboard
  const copyAddress = useCallback(() => {
    if (address) {
      navigator.clipboard.writeText(address);
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    }
  }, [address]);

  // Format USDC balance
  const formatBalance = (balance: string | null) => {
    if (!balance) return '0.00';
    const num = parseFloat(balance);
    if (num >= 1000000) return `${(num / 1000000).toFixed(2)}M`;
    if (num >= 1000) return `${(num / 1000).toFixed(2)}K`;
    return num.toFixed(2);
  };

  // Conversation history for API
  const conversationHistory = useRef<ConversationMessage[]>(
    loadFromStorage<ConversationMessage[]>(STORAGE_KEY_HISTORY, [])
  );

  // Load chat history from database on mount
  // The backend is the source of truth for the active session
  useEffect(() => {
    const loadHistory = async () => {
      try {
        // Get the active session from the backend (creates one if needed)
        const webSession = await getActiveWebSession();
        if (webSession) {
          setDbSessionId(webSession.session_id);
          // Update local sessionId to match backend
          const backendSessionId = `session-${webSession.session_id}`;
          setSessionId(backendSessionId);
          localStorage.setItem(STORAGE_KEY_SESSION_ID, backendSessionId);

          // Only load if we have messages and haven't loaded yet
          if (webSession.message_count && webSession.message_count > 0 && !historyLoaded) {
            const transcript = await getSessionTranscript(webSession.session_id);
            if (transcript.messages.length > 0) {
              // Convert DB messages to frontend format
              // Map tool_call and tool_result to 'tool' role for consistent styling
              // Special case: say_to_user tool results render as assistant bubbles
              const dbMessages: ChatMessageType[] = transcript.messages.map((msg, index) => {
                let role: MessageRole = msg.role as MessageRole;
                let content = msg.content;

                // Check if this is a say_to_user tool result - render as assistant bubble
                if (msg.role === 'tool_result' && msg.content.startsWith('**Result:** say_to_user\n')) {
                  role = 'assistant';
                  // Extract the actual message content (after the header line)
                  content = msg.content.replace('**Result:** say_to_user\n', '');
                } else if (msg.role === 'tool_call' || msg.role === 'tool_result') {
                  // Map other tool messages to 'tool' role
                  role = 'tool';
                }
                return {
                  id: `db-${msg.id || index}`,
                  role,
                  content,
                  timestamp: new Date(msg.created_at),
                  sessionId: backendSessionId,
                };
              });

              // Replace localStorage messages with DB messages
              setMessages(dbMessages);

              // Also update conversation history for API (filter out tool_call and tool_result)
              conversationHistory.current = transcript.messages
                .filter(msg => msg.role === 'user' || msg.role === 'assistant')
                .map(msg => ({
                  role: msg.role,
                  content: msg.content,
                }));
              localStorage.setItem(STORAGE_KEY_HISTORY, JSON.stringify(conversationHistory.current));
            }
          }
        }
      } catch (err) {
        console.error('Failed to load chat history from database:', err);
      } finally {
        setHistoryLoaded(true);
      }
    };

    loadHistory();
  }, []); // Only run on mount

  const scrollToBottom = useCallback(() => {
    messagesEndRef.current?.scrollIntoView({ behavior: 'smooth' });
  }, []);

  useEffect(() => {
    scrollToBottom();
  }, [messages, scrollToBottom]);

  // Listen for real-time tool call events from the agent
  useEffect(() => {
    console.log('[AgentChat] Registering agent.tool_call listener');
    const handleToolCall = (data: unknown) => {
      // Filter out events from other channels (e.g., cron jobs)
      if (!isWebChannelEvent(data)) return;

      console.log('[AgentChat] Received agent.tool_call event:', data);
      const event = data as { tool_name: string; parameters: Record<string, unknown> };
      const paramsPretty = JSON.stringify(event.parameters, null, 2);
      const content = `🔧 **Tool Call:** \`${event.tool_name}\`\n\`\`\`json\n${paramsPretty}\n\`\`\``;

      const message: ChatMessageType = {
        id: crypto.randomUUID(),
        role: 'tool' as MessageRole,
        content,
        timestamp: new Date(),
        sessionId,
      };
      setMessages((prev) => [...prev, message]);
    };

    on('agent.tool_call', handleToolCall);
    return () => {
      console.log('[AgentChat] Unregistering agent.tool_call listener');
      off('agent.tool_call', handleToolCall);
    };
  }, [on, off, sessionId]);

  // Listen for tool result events to show success/failure in chat
  useEffect(() => {
    console.log('[AgentChat] Registering tool.result listener');
    const handleToolResult = (data: unknown) => {
      // Filter out events from other channels (e.g., cron jobs)
      if (!isWebChannelEvent(data)) return;

      console.log('[AgentChat] Received tool.result event:', data);
      const event = data as { tool_name: string; success: boolean; duration_ms: number; content: string };

      // Show say_to_user messages immediately as assistant bubbles
      if (event.tool_name === 'say_to_user') {
        if (event.success && event.content.trim()) {
          const message: ChatMessageType = {
            id: crypto.randomUUID(),
            role: 'assistant' as MessageRole,
            content: event.content,
            timestamp: new Date(),
            sessionId,
          };
          setMessages((prev) => [...prev, message]);
        }
        return;
      }

      const statusEmoji = event.success ? '✅' : '❌';
      const statusText = event.success ? 'Success' : 'Failed';

      // Show full content - no truncation for visibility
      let displayContent = event.content;

      const content = `${statusEmoji} **Tool Result:** \`${event.tool_name}\` - ${statusText} (${event.duration_ms}ms)\n\`\`\`\n${displayContent}\n\`\`\``;

      const message: ChatMessageType = {
        id: crypto.randomUUID(),
        role: event.success ? 'tool' as MessageRole : 'error' as MessageRole,
        content,
        timestamp: new Date(),
        sessionId,
      };
      setMessages((prev) => [...prev, message]);
    };

    on('tool.result', handleToolResult);
    return () => {
      console.log('[AgentChat] Unregistering tool.result listener');
      off('tool.result', handleToolResult);
    };
  }, [on, off, sessionId]);

  // Listen for transaction events
  useEffect(() => {
    const handleTxPending = (data: unknown) => {
      // Filter out events from other channels (e.g., cron jobs)
      if (!isWebChannelEvent(data)) return;

      const event = data as TxPendingEvent;
      console.log('[TX] Pending transaction:', event.tx_hash);

      setTrackedTxs((prev) => {
        // Avoid duplicates
        if (prev.some((tx) => tx.tx_hash === event.tx_hash)) {
          return prev;
        }
        return [
          ...prev,
          {
            tx_hash: event.tx_hash,
            network: event.network,
            explorer_url: event.explorer_url,
            status: 'pending',
            timestamp: new Date(),
          },
        ];
      });
    };

    const handleTxConfirmed = (data: unknown) => {
      // Filter out events from other channels (e.g., cron jobs)
      if (!isWebChannelEvent(data)) return;

      const event = data as TxConfirmedEvent;
      console.log('[TX] Transaction confirmed:', event.tx_hash, event.status);

      setTrackedTxs((prev) =>
        prev.map((tx) =>
          tx.tx_hash === event.tx_hash
            ? { ...tx, status: event.status as 'confirmed' | 'reverted' | 'pending' }
            : tx
        )
      );

      // Auto-remove confirmed transactions after 30 seconds
      if (event.status === 'confirmed') {
        setTimeout(() => {
          setTrackedTxs((prev) => prev.filter((tx) => tx.tx_hash !== event.tx_hash));
        }, 30000);
      }
    };

    on('tx.pending', handleTxPending);
    on('tx.confirmed', handleTxConfirmed);

    return () => {
      off('tx.pending', handleTxPending);
      off('tx.confirmed', handleTxConfirmed);
    };
  }, [on, off]);

  // Listen for agent mode changes
  useEffect(() => {
    const handleModeChange = (data: unknown) => {
      // Filter out events from other channels (e.g., cron jobs)
      if (!isWebChannelEvent(data)) return;

      const event = data as { mode: string; label: string; reason?: string };
      console.log('[Agent] Mode changed:', event.mode, event.label, event.reason);
      setAgentMode({ mode: event.mode, label: event.label });
    };

    on('agent.mode_change', handleModeChange);
    return () => {
      off('agent.mode_change', handleModeChange);
    };
  }, [on, off]);

  // Listen for agent subtype changes (Finance/CodeEngineer)
  useEffect(() => {
    const handleSubtypeChange = (data: unknown) => {
      // Filter out events from other channels (e.g., cron jobs)
      if (!isWebChannelEvent(data)) return;

      const event = data as { subtype: string; label: string };
      console.log('[Agent] Subtype changed:', event.subtype, event.label);
      setAgentSubtype({ subtype: event.subtype, label: event.label });
    };

    on('agent.subtype_change', handleSubtypeChange);
    return () => {
      off('agent.subtype_change', handleSubtypeChange);
    };
  }, [on, off]);

  // Listen for cron execution events (for stop button visibility when cron runs in main mode)
  useEffect(() => {
    const handleCronStarted = (data: unknown) => {
      if (!isWebChannelEvent(data)) return;
      const event = data as { job_id: string; job_name: string; session_mode: string };
      console.log('[Cron] Execution started on web channel:', event.job_id, event.job_name);
      setCronExecutionActive({ job_id: event.job_id, job_name: event.job_name });
      setIsLoading(true);
    };

    const handleCronStopped = (data: unknown) => {
      if (!isWebChannelEvent(data)) return;
      const event = data as { job_id: string; reason: string };
      console.log('[Cron] Execution stopped on web channel:', event.job_id, event.reason);
      setCronExecutionActive(null);
      setIsLoading(false);
    };

    on('cron.execution_started_on_channel', handleCronStarted);
    on('cron.execution_stopped_on_channel', handleCronStopped);

    return () => {
      off('cron.execution_started_on_channel', handleCronStarted);
      off('cron.execution_stopped_on_channel', handleCronStopped);
    };
  }, [on, off]);

  // Listen for agent thinking/progress events (long AI calls)
  useEffect(() => {
    const handleThinking = (data: unknown) => {
      // Filter out events from other channels/sessions
      if (!isCurrentSessionEvent(data, dbSessionId)) return;

      const event = data as { message: string; timestamp: string };
      console.log('[Agent] Thinking:', event.message);
      // Add thinking message - only filter duplicate "Still thinking" progress messages
      setMessages((prev) => {
        // Only filter out repeated "Still thinking..." messages (same content pattern)
        const isStillThinking = event.message.startsWith('Still thinking');
        const filtered = isStillThinking
          ? prev.filter((m) => !(m.role === 'system' && m.content.startsWith('Still thinking')))
          : prev;
        return [
          ...filtered,
          {
            id: crypto.randomUUID(),
            role: 'system' as MessageRole,
            content: `💭 ${event.message}`,
            timestamp: new Date(event.timestamp),
            sessionId,
          },
        ];
      });
    };

    const handleError = (data: unknown) => {
      // Filter out events from other channels/sessions
      if (!isCurrentSessionEvent(data, dbSessionId)) return;

      const event = data as { error: string; timestamp: string };
      console.error('[Agent] Error:', event.error);
      setIsLoading(false);

      const errorLower = event.error.toLowerCase();
      const ts = new Date(event.timestamp);

      setMessages((prev) => {
        const newMessages: ChatMessageType[] = [
          ...prev,
          {
            id: crypto.randomUUID(),
            role: 'system' as MessageRole,
            content: `⚠️ ${event.error}`,
            timestamp: ts,
            sessionId,
          },
        ];

        // Detect AI endpoint errors that warrant a hint
        const isServerError = /\b(50[0-9]|502|503|504)\b/.test(event.error)
          || errorLower.includes('bad gateway')
          || errorLower.includes('service unavailable')
          || errorLower.includes('gateway timeout')
          || errorLower.includes('internal server error');
        const isConnectionError = errorLower.includes('connection refused')
          || errorLower.includes('connection reset')
          || errorLower.includes('econnrefused')
          || errorLower.includes('timed out');
        const isX402Failure = errorLower.includes('x402')
          || errorLower.includes('402')
          || errorLower.includes('payment');
        const isAuthError = errorLower.includes('401')
          || errorLower.includes('403')
          || errorLower.includes('unauthorized')
          || errorLower.includes('forbidden');

        if (isServerError || isConnectionError || isX402Failure || isAuthError) {
          const balanceNum = usdcBalance ? parseFloat(usdcBalance) : null;
          const isLowBalance = balanceNum !== null && balanceNum < 0.1;

          let hint = '**Tip:** Go to **Agent Settings** and try switching to a different AI model or endpoint.';
          if (isX402Failure || isLowBalance) {
            const balanceStr = balanceNum !== null ? ` (current balance: ${balanceNum.toFixed(4)} USDC)` : '';
            hint += `\n\nAlso make sure you have enough **USDC on Base** to cover x402 endpoint micropayments${balanceStr}.`;
          } else if (isServerError || isConnectionError) {
            hint += '\n\nIf you\'re using an x402 pay-per-call endpoint, also check your **USDC balance on Base** — insufficient funds can cause failures.';
          }

          newMessages.push({
            id: crypto.randomUUID(),
            role: 'hint' as MessageRole,
            content: hint,
            timestamp: ts,
            sessionId,
          });
        }

        return newMessages;
      });
    };

    const handleWarning = (data: unknown) => {
      // Filter out events from other channels/sessions
      if (!isCurrentSessionEvent(data, dbSessionId)) return;

      const event = data as { warning_type: string; message: string; attempt: number; timestamp: string };
      console.warn('[Agent] Warning:', event.warning_type, event.message);
      setMessages((prev) => [
        ...prev,
        {
          id: crypto.randomUUID(),
          role: 'system' as MessageRole,
          content: `⚠️ [${event.warning_type}] ${event.message}`,
          timestamp: new Date(event.timestamp),
          sessionId,
        },
      ]);
    };

    const handleAiRetrying = (data: unknown) => {
      // Filter out events from other channels/sessions
      if (!isCurrentSessionEvent(data, dbSessionId)) return;

      const event = data as {
        attempt: number;
        max_attempts: number;
        wait_seconds: number;
        error: string;
        provider: string;
        timestamp: string;
      };
      console.warn('[AI] Retrying:', event.provider, `attempt ${event.attempt}/${event.max_attempts}`);
      // Replace any previous retry message with the new one
      setMessages((prev) => {
        const filtered = prev.filter((m) => !(m.role === 'system' && m.content.startsWith('🔄 API retry')));
        return [
          ...filtered,
          {
            id: crypto.randomUUID(),
            role: 'system' as MessageRole,
            content: `🔄 API retry ${event.attempt}/${event.max_attempts} (${event.provider}) - waiting ${event.wait_seconds}s... ${event.error}`,
            timestamp: new Date(event.timestamp),
            sessionId,
          },
        ];
      });
    };

    const handleContextCompacting = (data: unknown) => {
      // Filter out events from other sessions
      const event = data as {
        channel_id: number;
        session_id: number;
        compaction_type: string;
        reason: string;
        timestamp: string;
      };
      // Only show for current session
      if (event.session_id !== dbSessionId) return;

      console.log('[Context] Compacting:', event.compaction_type, event.reason);
      // Replace any previous compaction message with the new one
      setMessages((prev) => {
        const filtered = prev.filter((m) => !(m.role === 'system' && m.content.startsWith('📦 Compacting')));
        return [
          ...filtered,
          {
            id: crypto.randomUUID(),
            role: 'system' as MessageRole,
            content: `📦 Compacting conversation history (${event.compaction_type})...`,
            timestamp: new Date(event.timestamp),
            sessionId,
          },
        ];
      });
    };

    on('agent.thinking', handleThinking);
    on('agent.error', handleError);
    on('agent.warning', handleWarning);
    on('ai.retrying', handleAiRetrying);
    on('context.compacting', handleContextCompacting);

    return () => {
      off('agent.thinking', handleThinking);
      off('agent.error', handleError);
      off('agent.warning', handleWarning);
      off('ai.retrying', handleAiRetrying);
      off('context.compacting', handleContextCompacting);
    };
  }, [on, off, sessionId, dbSessionId]);

  // Listen for execution lifecycle events to track loading state
  useEffect(() => {
    const handleExecutionStarted = (data: unknown) => {
      // Filter out events from other channels/sessions
      if (!isCurrentSessionEvent(data, dbSessionId)) return;

      const event = data as { execution_id: string; channel_id: number; mode: string };
      console.log('[Execution] Started:', event.execution_id);
      setActiveExecutionId(event.execution_id);
      setIsLoading(true);
      setIsStopping(false); // Reset stopping state on new execution
    };

    const handleExecutionCompleted = (data: unknown) => {
      // Filter out events from other channels/sessions
      if (!isCurrentSessionEvent(data, dbSessionId)) return;

      const event = data as { execution_id: string; channel_id: number };
      console.log('[Execution] Completed:', event.execution_id);

      // Only clear loading if this matches our tracked execution
      setActiveExecutionId(prev => {
        if (prev === event.execution_id || prev === null) {
          setIsLoading(false);
          setIsStopping(false);
          return null;
        }
        return prev;
      });
    };

    const handleExecutionStopped = (data: unknown) => {
      // Filter out events from other channels/sessions
      if (!isCurrentSessionEvent(data, dbSessionId)) return;

      const event = data as { channel_id: number; execution_id: string; reason: string };
      console.log('[Execution] Stopped:', event.execution_id, event.reason);

      // Only clear loading if this matches our tracked execution
      setActiveExecutionId(prev => {
        if (prev === event.execution_id || prev === null) {
          setIsLoading(false);
          setIsStopping(false);
          setCronExecutionActive(null);
          // Mark all running subagents as cancelled
          setSubagents(s => s.map(sub =>
            sub.status === SubagentStatus.Running ? { ...sub, status: SubagentStatus.Cancelled } : sub
          ));
          return null;
        }
        return prev;
      });
    };

    on('execution.started', handleExecutionStarted);
    on('execution.completed', handleExecutionCompleted);
    on('execution.stopped', handleExecutionStopped);

    return () => {
      off('execution.started', handleExecutionStarted);
      off('execution.completed', handleExecutionCompleted);
      off('execution.stopped', handleExecutionStopped);
    };
  }, [on, off, dbSessionId]);

  // Listen for confirmation events
  useEffect(() => {
    const handleConfirmationRequired = (data: unknown) => {
      // Filter out events from other channels/sessions
      if (!isCurrentSessionEvent(data, dbSessionId)) return;

      const event = data as ConfirmationRequiredEvent;
      console.log('[Confirmation] Required:', event.tool_name, event.description);

      setPendingConfirmation({
        confirmation_id: event.confirmation_id,
        channel_id: event.channel_id,
        tool_name: event.tool_name,
        description: event.description,
        parameters: event.parameters,
        timestamp: event.timestamp,
      });
    };

    const handleConfirmationApproved = (data: unknown) => {
      // Filter out events from other channels/sessions
      if (!isCurrentSessionEvent(data, dbSessionId)) return;

      console.log('[Confirmation] Approved');
      setPendingConfirmation(null);
    };

    const handleConfirmationRejected = (data: unknown) => {
      // Filter out events from other channels/sessions
      if (!isCurrentSessionEvent(data, dbSessionId)) return;

      console.log('[Confirmation] Rejected');
      setPendingConfirmation(null);
    };

    on('confirmation.required', handleConfirmationRequired);
    on('confirmation.approved', handleConfirmationApproved);
    on('confirmation.rejected', handleConfirmationRejected);

    return () => {
      off('confirmation.required', handleConfirmationRequired);
      off('confirmation.approved', handleConfirmationApproved);
      off('confirmation.rejected', handleConfirmationRejected);
    };
  }, [on, off, dbSessionId]);

  // Listen for tx_queue confirmation events (partner mode)
  useEffect(() => {
    const handleTxQueueConfirmationRequired = (data: unknown) => {
      console.log('[TxQueue] RAW event received:', data);

      if (!isCurrentSessionEvent(data, dbSessionId)) {
        console.log('[TxQueue] Event filtered out by isCurrentSessionEvent');
        return;
      }

      const event = data as {
        channel_id: number;
        uuid: string;
        network: string;
        from?: string;
        to: string;
        value: string;
        value_formatted: string;
        data?: string;
      };
      console.log('[TxQueue] Confirmation required:', event.uuid, 'channel_id:', event.channel_id);

      // Only handle if it's for the web channel
      if (event.channel_id === WEB_CHANNEL_ID) {
        console.log('[TxQueue] Setting txQueueConfirmation state');
        setTxQueueConfirmation({
          uuid: event.uuid,
          network: event.network,
          from: event.from,
          to: event.to,
          value: event.value,
          value_formatted: event.value_formatted,
          data: event.data,
        });
      } else {
        console.log('[TxQueue] Wrong channel_id, expected', WEB_CHANNEL_ID, 'got', event.channel_id);
      }
    };

    const handleTxQueueResolved = (data: unknown) => {
      if (!isCurrentSessionEvent(data, dbSessionId)) return;
      console.log('[TxQueue] Transaction resolved');
      setTxQueueConfirmation(null);
    };

    on('tx_queue.confirmation_required', handleTxQueueConfirmationRequired);
    on('tx_queue.confirmed', handleTxQueueResolved);
    on('tx_queue.denied', handleTxQueueResolved);

    return () => {
      off('tx_queue.confirmation_required', handleTxQueueConfirmationRequired);
      off('tx_queue.confirmed', handleTxQueueResolved);
      off('tx_queue.denied', handleTxQueueResolved);
    };
  }, [on, off, dbSessionId]);

  // Listen for subagent events
  useEffect(() => {
    const handleSubagentSpawned = (data: unknown) => {
      // Filter out events from other channels/sessions
      if (!isCurrentSessionEvent(data, dbSessionId)) return;

      const event = data as { subagent_id: string; label: string; task: string; timestamp: string; parent_subagent_id?: string; depth?: number; agent_subtype?: string };
      console.log('[Subagent] Spawned:', event.label, 'subtype:', event.agent_subtype ?? 'none', 'depth:', event.depth ?? 0);
      setSubagents((prev) => [
        ...prev.filter(s => s.id !== event.subagent_id),
        {
          id: event.subagent_id,
          label: event.label,
          task: event.task,
          status: SubagentStatus.Running,
          started_at: event.timestamp,
          parent_subagent_id: event.parent_subagent_id,
          depth: event.depth ?? 0,
        },
      ]);

      // Add a visible chat message showing the subagent spawn
      const taskPreview = event.task.length > 120 ? `${event.task.slice(0, 120)}...` : event.task;
      const msgId = crypto.randomUUID();
      subagentSpawnMsgIds.current.set(event.subagent_id, msgId);
      const subtypeTag = event.agent_subtype ? ` \`${event.agent_subtype}\`` : '';
      const message: ChatMessageType = {
        id: msgId,
        role: 'system' as MessageRole,
        content: `🚀 **Subagent spawned:** ${event.label}${subtypeTag}\n${taskPreview}`,
        timestamp: new Date(),
        sessionId,
        subagentLabel: event.label,
      };
      setMessages((prev) => [...prev, message]);
    };

    const handleSubagentCompleted = (data: unknown) => {
      // Filter out events from other channels/sessions
      if (!isCurrentSessionEvent(data, dbSessionId)) return;

      const event = data as { subagent_id: string; label?: string };
      console.log('[Subagent] Completed:', event.subagent_id);
      const label = event.label || subagents.find(s => s.id === event.subagent_id)?.label || 'subagent';
      setSubagents((prev) => prev.map(s =>
        s.id === event.subagent_id ? { ...s, status: SubagentStatus.Completed } : s
      ));

      const message: ChatMessageType = {
        id: crypto.randomUUID(),
        role: 'system' as MessageRole,
        content: `✅ **Subagent completed:** ${label}`,
        timestamp: new Date(),
        sessionId,
        subagentLabel: label,
      };
      setMessages((prev) => [...prev, message]);
    };

    const handleSubagentFailed = (data: unknown) => {
      // Filter out events from other channels/sessions
      if (!isCurrentSessionEvent(data, dbSessionId)) return;

      const event = data as { subagent_id: string; label?: string; error?: string };
      console.log('[Subagent] Failed:', event.subagent_id);
      const label = event.label || subagents.find(s => s.id === event.subagent_id)?.label || 'subagent';
      setSubagents((prev) => prev.map(s =>
        s.id === event.subagent_id ? { ...s, status: SubagentStatus.Failed } : s
      ));

      const errorPreview = event.error ? `: ${event.error.slice(0, 100)}` : '';
      const message: ChatMessageType = {
        id: crypto.randomUUID(),
        role: 'error' as MessageRole,
        content: `❌ **Subagent failed:** ${label}${errorPreview}`,
        timestamp: new Date(),
        sessionId,
        subagentLabel: label,
      };
      setMessages((prev) => [...prev, message]);
    };

    const handleSubagentSessionReady = (data: unknown) => {
      if (!isWebChannelEvent(data)) return;
      const event = data as { subagent_id: string; session_id: number };
      console.log('[Subagent] Session ready:', event.subagent_id, 'session:', event.session_id);
      setSubagents((prev) => prev.map(s =>
        s.id === event.subagent_id ? { ...s, session_id: event.session_id } : s
      ));

      // Update the spawn message to include the session link
      const spawnMsgId = subagentSpawnMsgIds.current.get(event.subagent_id);
      if (spawnMsgId) {
        setMessages((prev) => prev.map(m =>
          m.id === spawnMsgId
            ? { ...m, content: `${m.content}\n📋 Session #${event.session_id} — /sessions/${event.session_id}` }
            : m
        ));
      }
    };

    const handleSubagentToolCall = (data: unknown) => {
      if (!isWebChannelEvent(data)) return;
      const event = data as { subagent_id: string; label: string; tool_name: string; params_preview?: string };
      console.log('[Subagent] Tool call:', event.subagent_id, event.tool_name);
      setSubagents((prev) => prev.map(s =>
        s.id === event.subagent_id ? { ...s, current_tool: event.tool_name } : s
      ));

      // Also show as a chat message with subagent label
      const paramsPretty = event.params_preview || '';
      const content = `🔧 **Tool Call:** \`${event.tool_name}\`\n\`\`\`json\n${paramsPretty}\n\`\`\``;
      const message: ChatMessageType = {
        id: crypto.randomUUID(),
        role: 'tool' as MessageRole,
        content,
        timestamp: new Date(),
        sessionId,
        subagentLabel: event.label,
      };
      setMessages((prev) => [...prev, message]);
    };

    const handleSubagentToolResult = (data: unknown) => {
      if (!isWebChannelEvent(data)) return;
      const event = data as { subagent_id: string; label: string; tool_name: string; success: boolean; content_preview?: string };
      console.log('[Subagent] Tool result:', event.subagent_id, event.tool_name, event.success);
      setSubagents((prev) => prev.map(s =>
        s.id === event.subagent_id ? { ...s, current_tool: undefined } : s
      ));

      // Also show as a chat message with subagent label
      const statusEmoji = event.success ? '✅' : '❌';
      const statusText = event.success ? 'Success' : 'Failed';
      const displayContent = event.content_preview || '';
      const content = `${statusEmoji} **Tool Result:** \`${event.tool_name}\` - ${statusText}\n\`\`\`\n${displayContent}\n\`\`\``;
      const message: ChatMessageType = {
        id: crypto.randomUUID(),
        role: event.success ? 'tool' as MessageRole : 'error' as MessageRole,
        content,
        timestamp: new Date(),
        sessionId,
        subagentLabel: event.label,
      };
      setMessages((prev) => [...prev, message]);
    };

    on('subagent.spawned', handleSubagentSpawned);
    on('subagent.completed', handleSubagentCompleted);
    on('subagent.failed', handleSubagentFailed);
    on('subagent.session_ready', handleSubagentSessionReady);
    on('subagent.tool_call', handleSubagentToolCall);
    on('subagent.tool_result', handleSubagentToolResult);

    return () => {
      off('subagent.spawned', handleSubagentSpawned);
      off('subagent.completed', handleSubagentCompleted);
      off('subagent.failed', handleSubagentFailed);
      off('subagent.session_ready', handleSubagentSessionReady);
      off('subagent.tool_call', handleSubagentToolCall);
      off('subagent.tool_result', handleSubagentToolResult);
    };
  }, [on, off, dbSessionId]);

  // Listen for planner task events (for inline task display)
  useEffect(() => {
    const handleTaskQueueUpdate = (data: unknown) => {
      if (!isCurrentSessionEvent(data, dbSessionId)) return;
      const event = data as TaskQueueUpdateEvent;
      console.log('[PlannerTasks] Queue update:', event);
      setPlannerTasks(event.tasks || []);
    };

    const handleTaskStatusChange = (data: unknown) => {
      if (!isCurrentSessionEvent(data, dbSessionId)) return;
      const event = data as TaskStatusChangeEvent;
      console.log('[PlannerTasks] Status change:', event);
      setPlannerTasks((prev) =>
        prev.map((task) =>
          task.id === event.task_id
            ? { ...task, status: event.status, description: event.description }
            : task
        )
      );
    };

    const handleSessionComplete = (data: unknown) => {
      if (!isCurrentSessionEvent(data, dbSessionId)) return;
      console.log('[PlannerTasks] Session complete, clearing tasks');
      setTimeout(() => setPlannerTasks([]), 3000);
    };

    const handleExecutionStopped = (data: unknown) => {
      if (!isCurrentSessionEvent(data, dbSessionId)) return;
      console.log('[PlannerTasks] Execution stopped, clearing tasks');
      setPlannerTasks([]);
    };

    on('task.queue_update', handleTaskQueueUpdate);
    on('task.status_change', handleTaskStatusChange);
    on('session.complete', handleSessionComplete);
    on('execution.stopped', handleExecutionStopped);

    return () => {
      off('task.queue_update', handleTaskQueueUpdate);
      off('task.status_change', handleTaskStatusChange);
      off('session.complete', handleSessionComplete);
      off('execution.stopped', handleExecutionStopped);
    };
  }, [on, off, dbSessionId]);

  // Poll for planner tasks while loading (fallback for missed WS events)
  useEffect(() => {
    if (!isLoading) return;

    const poll = async () => {
      try {
        const response = await getPlannerTasks();
        if (response.success && response.tasks.length > 0) {
          setPlannerTasks(response.tasks.map((t) => ({
            id: t.id,
            description: t.description,
            status: t.status as 'pending' | 'in_progress' | 'completed',
          })));
        }
      } catch {
        // Ignore polling errors
      }
    };

    // Initial fetch after a short delay (give WS a chance first)
    const initialTimeout = setTimeout(poll, 1500);
    // Then poll every 2 seconds
    const interval = setInterval(poll, 2000);

    return () => {
      clearTimeout(initialTimeout);
      clearInterval(interval);
    };
  }, [isLoading]);

  // Fetch initial subagent list when connected or session changes
  useEffect(() => {
    if (connected) {
      console.log('[Subagent] Fetching subagent list for session:', dbSessionId);
      listSubagents(dbSessionId ?? undefined).then((response) => {
        console.log('[Subagent] Initial fetch response:', response);
        if (response.success) {
          setSubagents(response.subagents);
        }
      }).catch((err) => {
        console.error('[Subagent] Failed to fetch subagents:', err);
      });
    }
  }, [connected, dbSessionId]);

  // Check execution status on mount/reconnect for page refresh resilience
  useEffect(() => {
    const checkExecutionStatus = async () => {
      try {
        const status = await getExecutionStatus();
        if (status.running && status.execution_id) {
          console.log('[Execution] Restoring active execution:', status.execution_id);
          setIsLoading(true);
          setActiveExecutionId(status.execution_id);
        }
      } catch (e) {
        console.error('Failed to check execution status:', e);
      }
    };

    if (connected) {
      checkExecutionStatus();
    }
  }, [connected]);

  // Debug: log subagents state changes
  useEffect(() => {
    console.log('[Subagent] State updated:', subagents);
  }, [subagents]);

  // Debug: log execution state changes
  useEffect(() => {
    console.log('[Execution] State - loading:', isLoading, 'activeId:', activeExecutionId);
  }, [isLoading, activeExecutionId]);

  const addMessage = useCallback((role: MessageRole, content: string) => {
    const message: ChatMessageType = {
      id: crypto.randomUUID(),
      role,
      content,
      timestamp: new Date(),
      sessionId,
    };
    setMessages((prev) => [...prev, message]);

    // Add to conversation history if user or assistant
    if (role === 'user' || role === 'assistant') {
      conversationHistory.current.push({ role, content });
      localStorage.setItem(STORAGE_KEY_HISTORY, JSON.stringify(conversationHistory.current));
    }
  }, [sessionId]);

  // Command handlers map - uses Command enum for type safety
  const commandHandlers: Record<Command, () => void | Promise<void>> = {
    [Command.Help]: () => {
      const helpText = getAllCommands()
        .map((cmd) => `• **/${cmd.name}** - ${cmd.description}`)
        .join('\n');
      addMessage('system', `**Available Commands:**\n\n${helpText}`);
    },
    [Command.Status]: async () => {
      const settings = await getAgentSettings();
      const duration = Math.floor((Date.now() - sessionStartTime.getTime()) / 1000);
      const mins = Math.floor(duration / 60);
      const secs = duration % 60;
      const messageCount = conversationHistory.current.length;
      const tokenEstimate = conversationHistory.current
        .reduce((acc, m) => acc + Math.ceil(m.content.length / 4), 0);

      addMessage('system', `**Session Status:**\n\n• Messages: ${messageCount}\n• Duration: ${mins}m ${secs}s\n• Provider: ${(settings as Record<string, unknown>).provider || 'anthropic'}\n• Est. tokens: ~${tokenEstimate}`);
    },
    [Command.New]: async () => {
      try {
        // Create a new session on the backend
        const newSession = await createNewWebSession();
        if (newSession) {
          const newSessionId = `session-${newSession.session_id}`;
          setDbSessionId(newSession.session_id);
          setSessionId(newSessionId);
          localStorage.setItem(STORAGE_KEY_SESSION_ID, newSessionId);
          console.log('[Session] Created new session:', newSession.session_id);

          // Clear local state
          conversationHistory.current = [];
          localStorage.removeItem(STORAGE_KEY_HISTORY);
          localStorage.removeItem(STORAGE_KEY_MODE);
          localStorage.removeItem(STORAGE_KEY_SUBTYPE);
          setAgentMode(null);
          setAgentSubtype(null);
          setSubagents([]);

          // Clear all messages and show welcome
          setMessages([{
            id: crypto.randomUUID(),
            role: 'system' as MessageRole,
            content: 'Conversation cleared. Starting fresh.',
            timestamp: new Date(),
            sessionId: newSessionId,
          }]);
        }
      } catch (err) {
        console.error('[Session] Failed to create new session:', err);
        addMessage('error', 'Failed to create new session');
      }
    },
    [Command.Reset]: async () => {
      try {
        const newSession = await createNewWebSession();
        if (newSession) {
          const newSessionId = `session-${newSession.session_id}`;
          setDbSessionId(newSession.session_id);
          setSessionId(newSessionId);
          localStorage.setItem(STORAGE_KEY_SESSION_ID, newSessionId);

          conversationHistory.current = [];
          localStorage.removeItem(STORAGE_KEY_HISTORY);
          localStorage.removeItem(STORAGE_KEY_MODE);
          localStorage.removeItem(STORAGE_KEY_SUBTYPE);
          setAgentMode(null);
          setAgentSubtype(null);
          setSubagents([]);

          setMessages([{
            id: crypto.randomUUID(),
            role: 'system' as MessageRole,
            content: 'Conversation reset.',
            timestamp: new Date(),
            sessionId: newSessionId,
          }]);
        }
      } catch (err) {
        console.error('[Session] Failed to reset session:', err);
        addMessage('error', 'Failed to reset session');
      }
    },
    [Command.Clear]: async () => {
      try {
        const newSession = await createNewWebSession();
        if (newSession) {
          const newSessionId = `session-${newSession.session_id}`;
          setDbSessionId(newSession.session_id);
          setSessionId(newSessionId);
          localStorage.setItem(STORAGE_KEY_SESSION_ID, newSessionId);

          conversationHistory.current = [];
          localStorage.removeItem(STORAGE_KEY_HISTORY);
          localStorage.removeItem(STORAGE_KEY_MODE);
          localStorage.removeItem(STORAGE_KEY_SUBTYPE);
          setAgentMode(null);
          setAgentSubtype(null);
          setSubagents([]);
          setMessages([]);
        }
      } catch (err) {
        console.error('[Session] Failed to clear session:', err);
      }
    },
    [Command.Skills]: async () => {
      try {
        const skills = await getSkills();
        if (skills.length === 0) {
          addMessage('system', 'No skills installed.');
          return;
        }
        const skillList = skills
          .map((s) => `• **${s.name}** - ${s.description || 'No description'}`)
          .join('\n');
        addMessage('system', `**Available Skills:**\n\n${skillList}`);
      } catch {
        addMessage('error', 'Failed to load skills');
      }
    },
    [Command.Tools]: async () => {
      try {
        const tools = await getTools();
        if (tools.length === 0) {
          addMessage('system', 'No tools available.');
          return;
        }
        const toolList = tools
          .map((t) => `• **${t.name}** ${t.enabled ? '✓' : '✗'} - ${t.description || 'No description'}`)
          .join('\n');
        addMessage('system', `**Available Tools:**\n\n${toolList}`);
      } catch {
        addMessage('error', 'Failed to load tools');
      }
    },
    [Command.Model]: async () => {
      try {
        const settings = await getAgentSettings() as Record<string, unknown>;
        addMessage('system', `**Model Configuration:**\n\n• Provider: ${settings.provider || 'anthropic'}\n• Model: ${settings.model || 'claude-3-opus'}\n• Temperature: ${settings.temperature ?? 0.7}`);
      } catch {
        addMessage('error', 'Failed to load model configuration');
      }
    },
    [Command.Export]: () => {
      const data = {
        messages: conversationHistory.current,
        exportedAt: new Date().toISOString(),
        sessionStart: sessionStartTime.toISOString(),
      };
      const blob = new Blob([JSON.stringify(data, null, 2)], { type: 'application/json' });
      const url = URL.createObjectURL(blob);
      const a = document.createElement('a');
      a.href = url;
      a.download = `chat-export-${Date.now()}.json`;
      a.click();
      URL.revokeObjectURL(url);
      addMessage('system', 'Conversation exported.');
    },
    [Command.Debug]: () => {
      setDebugMode((prev) => !prev);
      addMessage('system', `Debug mode ${!debugMode ? 'enabled' : 'disabled'}.`);
    },
    [Command.Confirm]: async () => {
      if (!pendingConfirmation) {
        addMessage('system', 'No pending transaction to confirm.');
        return;
      }
      try {
        addMessage('system', 'Confirming transaction...');
        const result = await confirmTransaction(pendingConfirmation.channel_id);
        if (result.success) {
          addMessage('system', result.message || 'Transaction confirmed and executing.');
          setPendingConfirmation(null);
        } else {
          addMessage('error', result.error || 'Failed to confirm transaction.');
        }
      } catch (error) {
        addMessage('error', error instanceof Error ? error.message : 'Failed to confirm transaction');
      }
    },
    [Command.Cancel]: async () => {
      if (!pendingConfirmation) {
        addMessage('system', 'No pending transaction to cancel.');
        return;
      }
      try {
        const result = await cancelTransaction(pendingConfirmation.channel_id);
        if (result.success) {
          addMessage('system', result.message || 'Transaction cancelled.');
          setPendingConfirmation(null);
        } else {
          addMessage('error', result.error || 'Failed to cancel transaction.');
        }
      } catch (error) {
        addMessage('error', error instanceof Error ? error.message : 'Failed to cancel transaction');
      }
    },
    [Command.Stop]: async () => {
      const hasRunningSubagents = subagents.some(s => s.status === SubagentStatus.Running);
      if (!isLoading && !hasRunningSubagents) {
        addMessage('system', 'No execution in progress.');
        return;
      }
      setIsStopping(true);
      try {
        const result = await stopExecution();
        if (result.success) {
          // Don't set isLoading=false here - wait for execution.stopped event
          addMessage('system', result.message || 'Stopping executions...');
        } else {
          setIsStopping(false);
          addMessage('error', result.error || 'Failed to stop execution.');
        }
      } catch (error) {
        setIsStopping(false);
        addMessage('error', error instanceof Error ? error.message : 'Failed to stop execution');
      }
    },
  };

  // Build slashCommands array from enum definitions (for autocomplete compatibility)
  const slashCommands: SlashCommand[] = getAllCommands().map((def) => ({
    name: def.name,
    description: def.description,
    handler: commandHandlers[def.command],
  }));

  const handleCommand = useCallback(async (commandName: string) => {
    const command = slashCommands.find((c) => c.name === commandName);
    if (command) {
      addMessage('command', `/${commandName}`);
      await command.handler();
    } else {
      addMessage('error', `Unknown command: /${commandName}`);
    }
  }, [addMessage, slashCommands]);

  // Handler for CommandMenu selections
  const handleMenuCommand = useCallback((command: Command) => {
    const def = COMMAND_DEFINITIONS[command];
    addMessage('command', `/${def.name}`);
    commandHandlers[command]();
  }, [addMessage, commandHandlers]);

  const handleSend = useCallback(async () => {
    const trimmedInput = input.trim();
    if (!trimmedInput || isLoading) return;

    setInput('');
    setShowAutocomplete(false);

    // Handle slash commands
    if (trimmedInput.startsWith('/')) {
      const commandName = trimmedInput.slice(1).split(' ')[0];
      await handleCommand(commandName);
      return;
    }

    // Regular message
    addMessage('user', trimmedInput);
    setIsLoading(true);

    try {
      const response = await sendChatMessage(trimmedInput, conversationHistory.current, currentNetwork?.name);
      // Remove "still thinking" progress messages before adding the response
      setMessages((prev) => prev.filter(
        (m) => !(m.role === 'system' && m.content.startsWith('Still thinking'))
      ));
      // Skip empty responses and responses already delivered via say_to_user WebSocket event
      if (response.response.trim()) {
        // Check if say_to_user already delivered this content via real-time event
        setMessages((prev) => {
          const trimmedResponse = response.response.trim();
          const alreadyDelivered = prev.some(
            (m) => m.role === 'assistant' && m.content.trim() === trimmedResponse
          );
          if (alreadyDelivered) return prev;
          return [...prev, {
            id: crypto.randomUUID(),
            role: 'assistant' as MessageRole,
            content: response.response,
            timestamp: new Date(),
            sessionId,
          }];
        });
      }
    } catch (error) {
      const errorMsg = error instanceof Error ? error.message : 'Failed to send message';
      addMessage('error', errorMsg);

      // Detect AI endpoint errors that warrant a hint
      const errorLower = errorMsg.toLowerCase();
      const isServerError = /\b(50[0-9]|502|503|504)\b/.test(errorMsg)
        || errorLower.includes('bad gateway')
        || errorLower.includes('service unavailable')
        || errorLower.includes('gateway timeout')
        || errorLower.includes('internal server error');
      const isConnectionError = errorLower.includes('connection refused')
        || errorLower.includes('connection reset')
        || errorLower.includes('econnrefused')
        || errorLower.includes('timed out');
      const isX402Failure = errorLower.includes('x402')
        || errorLower.includes('402')
        || errorLower.includes('payment');
      const isAuthError = errorLower.includes('401')
        || errorLower.includes('403')
        || errorLower.includes('unauthorized')
        || errorLower.includes('forbidden');

      if (isServerError || isConnectionError || isX402Failure || isAuthError) {
        const balanceNum = usdcBalance ? parseFloat(usdcBalance) : null;
        const isLowBalance = balanceNum !== null && balanceNum < 0.1;

        let hint = '**Tip:** Go to **Agent Settings** and try switching to a different AI model or endpoint.';
        if (isX402Failure || isLowBalance) {
          const balanceStr = balanceNum !== null ? ` (current balance: ${balanceNum.toFixed(4)} USDC)` : '';
          hint += `\n\nAlso make sure you have enough **USDC on Base** to cover x402 endpoint micropayments${balanceStr}.`;
        } else if (isServerError || isConnectionError) {
          hint += '\n\nIf you\'re using an x402 pay-per-call endpoint, also check your **USDC balance on Base** — insufficient funds can cause failures.';
        }

        addMessage('hint' as MessageRole, hint);
      }
    } finally {
      setIsLoading(false);
    }
  }, [input, isLoading, addMessage, handleCommand]);

  const handleKeyDown = useCallback((e: KeyboardEvent<HTMLTextAreaElement>) => {
    // Handle autocomplete navigation
    if (showAutocomplete) {
      const filteredCommands = slashCommands.filter((cmd) =>
        cmd.name.toLowerCase().startsWith(input.slice(1).toLowerCase())
      );

      if (e.key === 'ArrowDown') {
        e.preventDefault();
        setSelectedCommandIndex((prev) =>
          prev < filteredCommands.length - 1 ? prev + 1 : prev
        );
        return;
      }

      if (e.key === 'ArrowUp') {
        e.preventDefault();
        setSelectedCommandIndex((prev) => (prev > 0 ? prev - 1 : 0));
        return;
      }

      if (e.key === 'Tab' || e.key === 'Enter') {
        if (filteredCommands.length > 0) {
          e.preventDefault();
          const selectedCommand = filteredCommands[selectedCommandIndex];
          setInput(`/${selectedCommand.name}`);
          setShowAutocomplete(false);
          if (e.key === 'Enter') {
            handleCommand(selectedCommand.name);
            setInput('');
          }
          return;
        }
      }

      if (e.key === 'Escape') {
        setShowAutocomplete(false);
        return;
      }
    }

    // Send on Enter (without shift)
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      handleSend();
    }
  }, [showAutocomplete, input, selectedCommandIndex, slashCommands, handleSend, handleCommand]);

  const handleInputChange = useCallback((value: string) => {
    setInput(value);

    // Show autocomplete for slash commands
    if (value.startsWith('/') && !value.includes(' ')) {
      setShowAutocomplete(true);
      setSelectedCommandIndex(0);
    } else {
      setShowAutocomplete(false);
    }
  }, []);

  const handleCommandSelect = useCallback((command: SlashCommand) => {
    setInput('');
    setShowAutocomplete(false);
    handleCommand(command.name);
  }, [handleCommand]);

  return (
    <div className="flex flex-col h-screen">
      {/* Header */}
      <div className="flex flex-wrap items-center px-3 sm:px-6 py-2 sm:py-4 gap-x-4 gap-y-2 border-b border-slate-700 bg-slate-800/50">
        <div className="flex items-center gap-2 sm:gap-4 w-full sm:w-auto">
          <h1 className="text-lg sm:text-xl font-bold text-white">Agent Chat</h1>
          <div
            className="flex items-center gap-2 bg-slate-700/50 px-2 py-1 rounded cursor-pointer hover:bg-slate-600/50 transition-colors"
            onClick={() => navigate('/sessions')}
            title={connected ? 'Connected - View all chat sessions' : 'Disconnected - View all chat sessions'}
          >
            <span
              className={`w-2 h-2 rounded-full ${
                connected ? 'bg-green-400' : 'bg-red-400'
              }`}
            />
            <span className="text-xs text-slate-500 hidden sm:inline">Session:</span>
            <span className="text-xs font-mono text-slate-300">
              {dbSessionId ? dbSessionId.toString(16).padStart(8, '0') : sessionId.slice(0, 8)}
            </span>
          </div>
          {/* Agent Mode Badge */}
          {agentMode && (
            <div className={`flex items-center gap-2 px-3 py-1 rounded-full text-sm font-medium ${
              agentMode.mode === 'explore' ? 'bg-blue-500/20 text-blue-400 border border-blue-500/50' :
              agentMode.mode === 'plan' ? 'bg-orange-500/20 text-orange-400 border border-orange-500/50' :
              agentMode.mode === 'perform' ? 'bg-green-500/20 text-green-400 border border-green-500/50' :
              'bg-slate-500/20 text-slate-400 border border-slate-500/50'
            }`}>
              <span className={`w-2 h-2 rounded-full ${
                agentMode.mode === 'explore' ? 'bg-blue-400' :
                agentMode.mode === 'plan' ? 'bg-orange-400' :
                agentMode.mode === 'perform' ? 'bg-green-400' :
                'bg-slate-400'
              } ${isLoading ? 'animate-pulse' : ''}`} />
              <span>{agentMode.label}</span>
            </div>
          )}
        </div>

        {/* Row 2: Wallet */}
        <div className="flex items-center gap-2 sm:gap-3 w-full sm:w-auto">
          {/* Wallet Info - always shown (no browser connection needed) */}
          {walletLoading ? (
            <div className="flex items-center gap-2 bg-slate-700/50 px-3 py-1.5 rounded-lg">
              <Loader2 className="w-4 h-4 animate-spin text-slate-400" />
              <span className="text-sm text-slate-400">Loading wallet...</span>
            </div>
          ) : walletConnected && address ? (
            <div className="flex items-center gap-3">
              {/* Wallet Address Badge */}
              <div className="flex items-center gap-2 bg-slate-700/50 px-3 py-1.5 rounded-lg">
                <Wallet className="w-4 h-4 text-slate-400" />
                <span className="text-sm font-mono text-slate-300">
                  {truncateAddress(address)}
                </span>
                <button
                  onClick={copyAddress}
                  className="text-slate-400 hover:text-slate-200 transition-colors"
                  title="Copy address"
                >
                  {copied ? (
                    <Check className="w-4 h-4 text-green-400" />
                  ) : (
                    <Copy className="w-4 h-4" />
                  )}
                </button>
                {walletMode === 'flash' && (
                  <span className="text-xs px-1.5 py-0.5 bg-purple-500/20 text-purple-400 rounded font-medium ml-1">
                    Flash
                  </span>
                )}
              </div>

              {/* USDC Balance with Network Selector */}
              <div className="relative" ref={networkDropdownRef}>
                <button
                  onClick={() => setNetworkDropdownOpen(!networkDropdownOpen)}
                  className="flex items-center gap-2 bg-slate-700/50 px-3 py-1.5 rounded-lg hover:bg-slate-600/50 transition-colors cursor-pointer"
                >
                  <span className="text-sm font-semibold text-white">
                    {isCorrectNetwork ? formatBalance(usdcBalance) : '--'}
                  </span>
                  {isCorrectNetwork && currentNetwork ? (
                    <span className="text-xs px-2 py-0.5 bg-blue-500/20 text-blue-400 rounded-full font-medium">
                      USDC · {currentNetwork.displayName}
                    </span>
                  ) : (
                    <span className="text-xs px-2 py-0.5 bg-amber-500/20 text-amber-400 rounded-full font-medium">
                      Switch Network
                    </span>
                  )}
                  <ChevronDown className={`w-3 h-3 text-slate-400 transition-transform ${networkDropdownOpen ? 'rotate-180' : ''}`} />
                </button>
                {networkDropdownOpen && (
                  <div className="absolute top-full right-0 mt-1 bg-slate-800 border border-slate-600 rounded-lg shadow-xl z-50 min-w-[160px] max-w-[calc(100vw-1.5rem)] py-1">
                    {(Object.keys(SUPPORTED_NETWORKS) as SupportedNetwork[]).map((networkKey) => {
                      const network = SUPPORTED_NETWORKS[networkKey];
                      const isActive = currentNetwork?.name === network.name;
                      return (
                        <button
                          key={networkKey}
                          onClick={async () => {
                            setNetworkDropdownOpen(false);
                            if (!isActive) {
                              await switchNetwork(networkKey);
                            }
                          }}
                          className={`w-full flex items-center justify-between gap-2 px-3 py-2 text-sm text-left transition-colors ${
                            isActive
                              ? 'bg-blue-500/20 text-blue-400'
                              : 'text-slate-300 hover:bg-slate-700'
                          }`}
                        >
                          <span>{network.displayName}</span>
                          {isActive && <Check className="w-4 h-4" />}
                        </button>
                      );
                    })}
                  </div>
                )}
              </div>
            </div>
          ) : (
            <div className="flex items-center gap-2 bg-amber-500/10 border border-amber-500/30 px-3 py-1.5 rounded-lg">
              <Wallet className="w-4 h-4 text-amber-400" />
              <span className="text-sm text-amber-400">
                {walletError || 'No Wallet Configured'}
              </span>
            </div>
          )}
        </div>

        {/* Row 3: Controls */}
        <div className="flex items-center gap-2 sm:gap-4 w-full sm:w-auto sm:ml-auto flex-wrap">
          {/* Agent Subtype Dropdown */}
          <div className="relative" ref={subtypeDropdownRef}>
            {(() => {
              const currentIdx = availableSubtypes.findIndex(s => s.key === agentSubtype?.subtype);
              const colors = currentIdx >= 0 ? SUBTYPE_COLORS[currentIdx % SUBTYPE_COLORS.length] : null;
              const currentEmoji = currentIdx >= 0 ? availableSubtypes[currentIdx].emoji : null;
              return (
                <>
                  <button
                    onClick={() => setSubtypeDropdownOpen(!subtypeDropdownOpen)}
                    className={`flex items-center gap-2 px-3 py-1 rounded-full text-sm font-medium cursor-pointer transition-colors ${
                      colors
                        ? `${colors.bgClass} ${colors.textClass} border ${colors.borderClass} ${colors.hoverClass}`
                        : 'bg-slate-500/20 text-slate-400 border border-slate-500/50 hover:bg-slate-500/30'
                    }`}
                  >
                    <span>{currentEmoji || '🔧'}</span>
                    <span>{agentSubtype?.label || 'Select Toolbox'}</span>
                    <ChevronDown className={`w-3 h-3 transition-transform ${subtypeDropdownOpen ? 'rotate-180' : ''}`} />
                  </button>
                  {subtypeDropdownOpen && (
                    <div className="absolute top-full left-0 mt-1 bg-slate-800 border border-slate-600 rounded-lg shadow-xl z-50 min-w-[160px] max-w-[calc(100vw-1.5rem)] py-1">
                      {availableSubtypes.map((st, idx) => {
                        const stColors = SUBTYPE_COLORS[idx % SUBTYPE_COLORS.length];
                        return (
                          <button
                            key={st.key}
                            onClick={() => {
                              setAgentSubtype({ subtype: st.key, label: st.label });
                              setSubtypeDropdownOpen(false);
                            }}
                            className={`w-full flex items-center gap-2 px-3 py-2 text-sm text-left transition-colors ${
                              agentSubtype?.subtype === st.key
                                ? `${stColors.bgClass} ${stColors.textClass}`
                                : 'text-slate-300 hover:bg-slate-700'
                            }`}
                          >
                            <span>{st.emoji}</span>
                            <span>{st.label}</span>
                          </button>
                        );
                      })}
                    </div>
                  )}
                </>
              );
            })()}
          </div>

          {/* Debug Toggle */}
          <button
            onClick={() => setDebugMode(!debugMode)}
            className={`flex items-center gap-2 px-3 py-1.5 rounded-lg text-sm font-medium transition-colors ${
              debugMode
                ? 'bg-cyan-500/20 text-cyan-400 border border-cyan-500/50'
                : 'bg-slate-700/50 text-slate-400 hover:text-slate-200 hover:bg-slate-700'
            }`}
            title="Toggle debug mode"
          >
            <Bug className="w-4 h-4" />
            <span className="hidden sm:inline">Debug</span>
            {/* Toggle switch */}
            <div className={`w-8 h-4 rounded-full transition-colors ${debugMode ? 'bg-cyan-500' : 'bg-slate-600'}`}>
              <div
                className={`w-3 h-3 rounded-full bg-white transition-transform transform mt-0.5 ${
                  debugMode ? 'translate-x-4 ml-0.5' : 'translate-x-0.5'
                }`}
              />
            </div>
          </button>

          <SubagentBadge
            subagents={subagents}
            onSubagentCancelled={(id) => {
              setSubagents((prev) => prev.filter(s => s.id !== id));
            }}
          />

          <Button
            variant="ghost"
            size="sm"
            disabled={isStopping}
            onClick={async () => {
              const hasRunningSubagents = subagents.some(s => s.status === SubagentStatus.Running);
              if (isLoading || hasRunningSubagents || cronExecutionActive) {
                // Stop ALL executions including subagents and cron jobs
                setIsStopping(true);
                try {
                  const result = await stopExecution();
                  if (result.success) {
                    // Don't set isLoading=false here - wait for execution.stopped event
                    addMessage('system', result.message || 'Stopping executions...');
                  } else {
                    // Reset stopping state on failure
                    setIsStopping(false);
                  }
                } catch (error) {
                  console.error('Failed to stop execution:', error);
                  setIsStopping(false);
                }
              } else {
                // Clear the chat and start new session on the backend
                try {
                  const newSession = await createNewWebSession();
                  if (newSession) {
                    const newSessionId = `session-${newSession.session_id}`;
                    setDbSessionId(newSession.session_id);
                    setSessionId(newSessionId);
                    localStorage.setItem(STORAGE_KEY_SESSION_ID, newSessionId);

                    conversationHistory.current = [];
                    localStorage.removeItem(STORAGE_KEY_HISTORY);
                    localStorage.removeItem(STORAGE_KEY_MODE);
                    localStorage.removeItem(STORAGE_KEY_SUBTYPE);
                    setAgentMode(null);
                    setAgentSubtype(null);
                    setSubagents([]);
                    setMessages([]);
                  }
                } catch (err) {
                  console.error('[Session] Failed to create new session:', err);
                }
              }
            }}
          >
            {isStopping ? (
              <>
                <Loader2 className="w-4 h-4 mr-2 animate-spin" />
                Stopping...
              </>
            ) : (isLoading || cronExecutionActive || subagents.some(s => s.status === SubagentStatus.Running)) ? (
              <>
                <Square className="w-4 h-4 mr-2" />
                {cronExecutionActive ? `Stop: ${cronExecutionActive.job_name}` : 'Stop'}
              </>
            ) : (
              <>
                <RotateCcw className="w-4 h-4 mr-2" />
                Clear
              </>
            )}
          </Button>
        </div>
      </div>

      {/* Messages */}
      <div className="flex-1 overflow-y-auto p-3 sm:p-6">
        {messages.filter((m) => m.sessionId === sessionId).length === 0 ? (
          <div className="h-full flex items-center justify-center">
            <div className="text-center">
              <h2 className="text-xl font-semibold text-white mb-2">
                Welcome to Agent Chat
              </h2>
              <p className="text-slate-400 mb-4">
                Start a conversation or type <code className="bg-slate-700 px-1 rounded">/help</code> for commands
              </p>
            </div>
          </div>
        ) : (
          <>
            {messages
              .filter((message) => message.sessionId === sessionId)
              .map((message) => (
                <ChatMessage
                  key={message.id}
                  role={message.role}
                  content={message.content}
                  timestamp={message.timestamp}
                  subagentLabel={message.subagentLabel}
                />
              ))}
            {isLoading && <TypingIndicator />}
          </>
        )}
        {/* Live Subagent Activity Panel */}
        {subagents.filter(s => s.status === SubagentStatus.Running || s.status === SubagentStatus.Pending).length > 0 && (
          <div className="mx-2 my-3 space-y-2">
            {subagents
              .filter(s => s.status === SubagentStatus.Running || s.status === SubagentStatus.Pending)
              .map((sub) => (
                <div
                  key={sub.id}
                  className="flex items-center gap-3 px-4 py-2.5 bg-purple-500/10 border border-purple-500/30 rounded-lg"
                >
                  <div className="w-2 h-2 bg-purple-400 rounded-full animate-pulse flex-shrink-0" />
                  <div className="flex-1 min-w-0">
                    <div className="flex items-center gap-2">
                      <span className="text-sm font-medium text-purple-300">{sub.label}</span>
                      {sub.current_tool ? (
                        <span className="flex items-center gap-1 text-xs text-cyan-400">
                          <Wrench className="w-3 h-3" />
                          <span className="truncate">{sub.current_tool}</span>
                          <Loader2 className="w-3 h-3 animate-spin" />
                        </span>
                      ) : (
                        <span className="text-xs text-slate-500">waiting...</span>
                      )}
                    </div>
                  </div>
                  {sub.session_id && (
                    <a
                      href={`/sessions/${sub.session_id}`}
                      target="_blank"
                      rel="noopener noreferrer"
                      className="flex-shrink-0 p-1 text-slate-500 hover:text-purple-400 transition-colors"
                      title="View session transcript"
                    >
                      <ExternalLink className="w-3.5 h-3.5" />
                    </a>
                  )}
                </div>
              ))}
          </div>
        )}
        <div ref={messagesEndRef} />
      </div>

      {/* Debug Panel - always mounted to capture events, hidden when not in debug mode */}
      <DebugPanel className={`mx-6 mb-4 ${debugMode ? '' : 'hidden'}`} />

      {/* Execution Progress */}
      <ExecutionProgress className="mx-6 mb-4" />

      {/* Transaction Tracker */}
      <TransactionTracker transactions={trackedTxs} className="mx-6 mb-4" />

      {/* Confirmation Prompt */}
      {pendingConfirmation && (
        <div className="mx-6 mb-4">
          <ConfirmationPrompt
            confirmation={pendingConfirmation}
            onConfirm={async (confirmationId) => {
              console.log('[Confirmation] Confirming:', confirmationId);
              const result = await confirmTransaction(pendingConfirmation.channel_id);
              if (result.success) {
                addMessage('system', result.message || 'Transaction confirmed and executing.');
                setPendingConfirmation(null);
              } else {
                throw new Error(result.error || 'Failed to confirm');
              }
            }}
            onCancel={async (confirmationId) => {
              console.log('[Confirmation] Cancelling:', confirmationId);
              const result = await cancelTransaction(pendingConfirmation.channel_id);
              if (result.success) {
                addMessage('system', result.message || 'Transaction cancelled.');
                setPendingConfirmation(null);
              } else {
                throw new Error(result.error || 'Failed to cancel');
              }
            }}
          />
        </div>
      )}

      {/* Transaction Queue Confirmation Modal (Partner Mode) */}
      <TxQueueConfirmationModal
        isOpen={txQueueConfirmation !== null}
        onClose={() => setTxQueueConfirmation(null)}
        channelId={WEB_CHANNEL_ID}
        transaction={txQueueConfirmation}
      />

      {/* Pending Transaction Indicator Bar (Partner Mode) */}
      {txQueueConfirmation && (
        <div className="mx-6 mb-2 p-3 bg-amber-500/10 border border-amber-500/50 rounded-lg">
          <div className="flex items-center justify-between flex-wrap gap-2">
            <div className="flex items-center gap-2 flex-wrap">
              <div className="w-2 h-2 bg-amber-400 rounded-full animate-pulse flex-shrink-0" />
              <span className="text-amber-400 font-medium">1 pending transaction</span>
              <span className="text-slate-400 text-sm">- {txQueueConfirmation.value_formatted} to</span>
              <a
                href={txQueueConfirmation.network === 'mainnet'
                  ? `https://etherscan.io/address/${txQueueConfirmation.to}`
                  : `https://basescan.org/address/${txQueueConfirmation.to}`}
                target="_blank"
                rel="noopener noreferrer"
                className="text-cyan-400 hover:text-cyan-300 font-mono text-xs"
                onClick={(e) => e.stopPropagation()}
              >
                {txQueueConfirmation.to}
              </a>
            </div>
            <span className="text-amber-400 text-sm">Confirm or deny in modal above</span>
          </div>
        </div>
      )}

      {/* Input */}
      <div className="px-3 sm:px-6 pb-3 sm:pb-6">
        <div className="relative">
          {showAutocomplete && !isLoading && (
            <CommandAutocomplete
              commands={slashCommands}
              filter={input}
              selectedIndex={selectedCommandIndex}
              onSelect={handleCommandSelect}
              onClose={() => setShowAutocomplete(false)}
            />
          )}
          <div className="flex gap-2 sm:gap-3">
            <div className="flex-1 relative">
              {isLoading ? (
                /* Inline Task List when running */
                <div
                  className="w-full h-full px-3 sm:px-4 py-3 bg-slate-800 border border-slate-700 rounded-lg overflow-y-auto"
                  style={{ minHeight: '104px', maxHeight: '200px' }}
                >
                  {plannerTasks.length > 0 ? (
                    <div className="space-y-1.5">
                      {plannerTasks.map((task) => (
                        <div
                          key={task.id}
                          className={`flex items-start gap-2 text-sm py-1 px-2 rounded ${
                            task.status === 'in_progress' ? 'bg-cyan-500/10 border border-cyan-500/30' :
                            task.status === 'completed' ? 'opacity-60' : ''
                          }`}
                        >
                          <div className="shrink-0 mt-0.5">
                            {task.status === 'completed' ? (
                              <CheckCircle className="w-4 h-4 text-green-400" />
                            ) : task.status === 'in_progress' ? (
                              <Loader2 className="w-4 h-4 text-cyan-400 animate-spin" />
                            ) : (
                              <Circle className="w-4 h-4 text-slate-500" />
                            )}
                          </div>
                          <span
                            className={`flex-1 ${
                              task.status === 'in_progress' ? 'text-cyan-300 font-medium' :
                              task.status === 'completed' ? 'text-slate-400 line-through' :
                              'text-slate-300'
                            }`}
                          >
                            {task.description}
                          </span>
                        </div>
                      ))}
                    </div>
                  ) : (
                    <div className="flex items-center justify-center h-full text-slate-500 text-sm">
                      <Loader2 className="w-4 h-4 mr-2 animate-spin" />
                      Processing...
                    </div>
                  )}
                </div>
              ) : (
                /* Normal textarea when idle */
                <textarea
                  ref={inputRef}
                  value={input}
                  onChange={(e) => handleInputChange(e.target.value)}
                  onKeyDown={handleKeyDown}
                  placeholder="Type a message or /command..."
                  rows={1}
                  className="w-full h-full px-3 sm:px-4 py-3 bg-slate-800 border border-slate-700 rounded-lg text-sm sm:text-base text-white placeholder-slate-500 focus:outline-none focus:ring-2 focus:ring-stark-500 focus:border-transparent resize-none"
                  style={{ minHeight: '104px', maxHeight: '200px' }}
                />
              )}
            </div>
            <div className="flex flex-col sm:flex-row gap-2 sm:gap-3">
              <CommandMenu onCommandSelect={handleMenuCommand} />
              {isLoading ? (
                /* Stop button when running */
                <button
                  onClick={async () => {
                    setIsStopping(true);
                    try {
                      const result = await stopExecution();
                      if (result.success) {
                        addMessage('system', result.message || 'Stopping execution...');
                      } else {
                        setIsStopping(false);
                      }
                    } catch (error) {
                      console.error('Failed to stop execution:', error);
                      setIsStopping(false);
                    }
                  }}
                  disabled={isStopping}
                  className="w-12 h-12 flex items-center justify-center rounded-lg bg-red-600 hover:bg-red-500 text-white transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
                >
                  {isStopping ? (
                    <Loader2 className="w-5 h-5 animate-spin" />
                  ) : (
                    <Square className="w-5 h-5" />
                  )}
                </button>
              ) : (
                /* Send button when idle */
                <button
                  onClick={handleSend}
                  disabled={!input.trim()}
                  className="w-12 h-12 flex items-center justify-center rounded-lg bg-gradient-to-r from-stark-500 to-stark-600 hover:from-stark-400 hover:to-stark-500 text-white transition-all disabled:opacity-50 disabled:cursor-not-allowed"
                >
                  <Send className="w-5 h-5" />
                </button>
              )}
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}
