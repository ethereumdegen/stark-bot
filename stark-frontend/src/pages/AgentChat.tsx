import { useState, useEffect, useRef, useCallback, KeyboardEvent } from 'react';
import { Send, RotateCcw, Copy, Check, Wallet, Bug, Square } from 'lucide-react';
import Button from '@/components/ui/Button';
import ChatMessage from '@/components/chat/ChatMessage';
import TypingIndicator from '@/components/chat/TypingIndicator';
import ExecutionProgress from '@/components/chat/ExecutionProgress';
import DebugPanel from '@/components/chat/DebugPanel';
import CommandAutocomplete from '@/components/chat/CommandAutocomplete';
import CommandMenu from '@/components/chat/CommandMenu';
import TransactionTracker from '@/components/chat/TransactionTracker';
import { ConfirmationPrompt } from '@/components/chat/ConfirmationPrompt';
import SubagentBadge, { Subagent } from '@/components/chat/SubagentBadge';
import { useGateway } from '@/hooks/useGateway';
import { useWallet } from '@/hooks/useWallet';
import { sendChatMessage, getAgentSettings, getSkills, getTools, confirmTransaction, cancelTransaction, stopExecution, listSubagents } from '@/lib/api';
import { Command, COMMAND_DEFINITIONS, getAllCommands } from '@/lib/commands';
import type { ChatMessage as ChatMessageType, MessageRole, SlashCommand, TrackedTransaction, TxPendingEvent, TxConfirmedEvent, PendingConfirmation, ConfirmationRequiredEvent } from '@/types';

interface ConversationMessage {
  role: string;
  content: string;
}

// localStorage keys for persistence
const STORAGE_KEY_MESSAGES = 'agentChat_messages';
const STORAGE_KEY_HISTORY = 'agentChat_history';
const STORAGE_KEY_MODE = 'agentChat_mode';
const STORAGE_KEY_SUBTYPE = 'agentChat_subtype';

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
  const [showAutocomplete, setShowAutocomplete] = useState(false);
  const [selectedCommandIndex, setSelectedCommandIndex] = useState(0);
  const [debugMode, setDebugMode] = useState(false);
  const [sessionStartTime] = useState(new Date());
  const [copied, setCopied] = useState(false);
  const [trackedTxs, setTrackedTxs] = useState<TrackedTransaction[]>([]);
  const [pendingConfirmation, setPendingConfirmation] = useState<PendingConfirmation | null>(null);
  const [subagents, setSubagents] = useState<Subagent[]>([]);
  const [agentMode, setAgentMode] = useState<{ mode: string; label: string } | null>(() =>
    loadFromStorage<{ mode: string; label: string } | null>(STORAGE_KEY_MODE, null)
  );
  const [agentSubtype, setAgentSubtype] = useState<{ subtype: string; label: string } | null>(() =>
    loadFromStorage<{ subtype: string; label: string } | null>(STORAGE_KEY_SUBTYPE, null)
  );

  const messagesEndRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLTextAreaElement>(null);
  const { connected, on, off } = useGateway();
  const { address, usdcBalance, isConnected: walletConnected, connect: connectWallet, isCorrectNetwork } = useWallet();

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
      console.log('[AgentChat] Received agent.tool_call event:', data);
      const event = data as { tool_name: string; parameters: Record<string, unknown> };
      const paramsPretty = JSON.stringify(event.parameters, null, 2);
      const content = `🔧 **Tool Call:** \`${event.tool_name}\`\n\`\`\`json\n${paramsPretty}\n\`\`\``;

      const message: ChatMessageType = {
        id: crypto.randomUUID(),
        role: 'tool' as MessageRole,
        content,
        timestamp: new Date(),
      };
      setMessages((prev) => [...prev, message]);
    };

    on('agent.tool_call', handleToolCall);
    return () => {
      console.log('[AgentChat] Unregistering agent.tool_call listener');
      off('agent.tool_call', handleToolCall);
    };
  }, [on, off]);

  // Listen for tool result events to show success/failure in chat
  useEffect(() => {
    console.log('[AgentChat] Registering tool.result listener');
    const handleToolResult = (data: unknown) => {
      console.log('[AgentChat] Received tool.result event:', data);
      const event = data as { tool_name: string; success: boolean; duration_ms: number; content: string };
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
      };
      setMessages((prev) => [...prev, message]);
    };

    on('tool.result', handleToolResult);
    return () => {
      console.log('[AgentChat] Unregistering tool.result listener');
      off('tool.result', handleToolResult);
    };
  }, [on, off]);

  // Listen for transaction events
  useEffect(() => {
    const handleTxPending = (data: unknown) => {
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
      const event = data as { subtype: string; label: string };
      console.log('[Agent] Subtype changed:', event.subtype, event.label);
      setAgentSubtype({ subtype: event.subtype, label: event.label });
    };

    on('agent.subtype_change', handleSubtypeChange);
    return () => {
      off('agent.subtype_change', handleSubtypeChange);
    };
  }, [on, off]);

  // Listen for agent thinking/progress events (long AI calls)
  useEffect(() => {
    const handleThinking = (data: unknown) => {
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
          },
        ];
      });
    };

    const handleError = (data: unknown) => {
      const event = data as { error: string; timestamp: string };
      console.error('[Agent] Error:', event.error);
      setIsLoading(false);
      setMessages((prev) => [
        ...prev,
        {
          id: crypto.randomUUID(),
          role: 'system' as MessageRole,
          content: `⚠️ ${event.error}`,
          timestamp: new Date(event.timestamp),
        },
      ]);
    };

    const handleWarning = (data: unknown) => {
      const event = data as { warning_type: string; message: string; attempt: number; timestamp: string };
      console.warn('[Agent] Warning:', event.warning_type, event.message);
      setMessages((prev) => [
        ...prev,
        {
          id: crypto.randomUUID(),
          role: 'system' as MessageRole,
          content: `⚠️ [${event.warning_type}] ${event.message}`,
          timestamp: new Date(event.timestamp),
        },
      ]);
    };

    on('agent.thinking', handleThinking);
    on('agent.error', handleError);
    on('agent.warning', handleWarning);

    return () => {
      off('agent.thinking', handleThinking);
      off('agent.error', handleError);
      off('agent.warning', handleWarning);
    };
  }, [on, off]);

  // Listen for execution lifecycle events to track loading state
  useEffect(() => {
    const handleExecutionStarted = () => {
      console.log('[Execution] Started');
      setIsLoading(true);
    };

    const handleExecutionCompleted = () => {
      console.log('[Execution] Completed');
      setIsLoading(false);
    };

    on('execution.started', handleExecutionStarted);
    on('execution.completed', handleExecutionCompleted);

    return () => {
      off('execution.started', handleExecutionStarted);
      off('execution.completed', handleExecutionCompleted);
    };
  }, [on, off]);

  // Listen for confirmation events
  useEffect(() => {
    const handleConfirmationRequired = (data: unknown) => {
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

    const handleConfirmationApproved = () => {
      console.log('[Confirmation] Approved');
      setPendingConfirmation(null);
    };

    const handleConfirmationRejected = () => {
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
  }, [on, off]);

  // Listen for subagent events
  useEffect(() => {
    const handleSubagentSpawned = (data: unknown) => {
      const event = data as { subagent_id: string; label: string; task: string; timestamp: string };
      console.log('[Subagent] Spawned:', event.label);
      setSubagents((prev) => [
        ...prev.filter(s => s.id !== event.subagent_id),
        {
          id: event.subagent_id,
          label: event.label,
          task: event.task,
          status: 'Running',
          started_at: event.timestamp,
        },
      ]);
    };

    const handleSubagentCompleted = (data: unknown) => {
      const event = data as { subagent_id: string };
      console.log('[Subagent] Completed:', event.subagent_id);
      setSubagents((prev) => prev.map(s =>
        s.id === event.subagent_id ? { ...s, status: 'Completed' } : s
      ));
    };

    const handleSubagentFailed = (data: unknown) => {
      const event = data as { subagent_id: string };
      console.log('[Subagent] Failed:', event.subagent_id);
      setSubagents((prev) => prev.map(s =>
        s.id === event.subagent_id ? { ...s, status: 'Failed' } : s
      ));
    };

    on('subagent.spawned', handleSubagentSpawned);
    on('subagent.completed', handleSubagentCompleted);
    on('subagent.failed', handleSubagentFailed);

    return () => {
      off('subagent.spawned', handleSubagentSpawned);
      off('subagent.completed', handleSubagentCompleted);
      off('subagent.failed', handleSubagentFailed);
    };
  }, [on, off]);

  // Fetch initial subagent list when connected
  useEffect(() => {
    if (connected) {
      console.log('[Subagent] Fetching initial subagent list...');
      listSubagents().then((response) => {
        console.log('[Subagent] Initial fetch response:', response);
        if (response.success) {
          setSubagents(response.subagents);
        }
      }).catch((err) => {
        console.error('[Subagent] Failed to fetch subagents:', err);
      });
    }
  }, [connected]);

  // Debug: log subagents state changes
  useEffect(() => {
    console.log('[Subagent] State updated:', subagents);
  }, [subagents]);

  const addMessage = useCallback((role: MessageRole, content: string) => {
    const message: ChatMessageType = {
      id: crypto.randomUUID(),
      role,
      content,
      timestamp: new Date(),
    };
    setMessages((prev) => [...prev, message]);

    // Add to conversation history if user or assistant
    if (role === 'user' || role === 'assistant') {
      conversationHistory.current.push({ role, content });
      localStorage.setItem(STORAGE_KEY_HISTORY, JSON.stringify(conversationHistory.current));
    }
  }, []);

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
    [Command.New]: () => {
      setMessages([]);
      conversationHistory.current = [];
      localStorage.removeItem(STORAGE_KEY_HISTORY);
      localStorage.removeItem(STORAGE_KEY_MODE);
      localStorage.removeItem(STORAGE_KEY_SUBTYPE);
      setAgentMode(null);
      setAgentSubtype(null);
      addMessage('system', 'Conversation cleared. Starting fresh.');
    },
    [Command.Reset]: () => {
      setMessages([]);
      conversationHistory.current = [];
      localStorage.removeItem(STORAGE_KEY_HISTORY);
      localStorage.removeItem(STORAGE_KEY_MODE);
      localStorage.removeItem(STORAGE_KEY_SUBTYPE);
      setAgentMode(null);
      setAgentSubtype(null);
      addMessage('system', 'Conversation reset.');
    },
    [Command.Clear]: () => {
      setMessages([]);
      conversationHistory.current = [];
      localStorage.removeItem(STORAGE_KEY_HISTORY);
      localStorage.removeItem(STORAGE_KEY_MODE);
      localStorage.removeItem(STORAGE_KEY_SUBTYPE);
      setAgentMode(null);
      setAgentSubtype(null);
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
      if (!isLoading) {
        addMessage('system', 'No execution in progress.');
        return;
      }
      try {
        const result = await stopExecution();
        if (result.success) {
          setIsLoading(false);
          addMessage('system', result.message || 'Execution stopped.');
        } else {
          addMessage('error', result.error || 'Failed to stop execution.');
        }
      } catch (error) {
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
      const response = await sendChatMessage(trimmedInput, conversationHistory.current);
      // Remove "still thinking" progress messages before adding the response
      setMessages((prev) => prev.filter(
        (m) => !(m.role === 'system' && m.content.startsWith('Still thinking'))
      ));
      addMessage('assistant', response.response);
    } catch (error) {
      addMessage('error', error instanceof Error ? error.message : 'Failed to send message');
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
      <div className="flex items-center justify-between px-6 py-4 border-b border-slate-700 bg-slate-800/50">
        <div className="flex items-center gap-4">
          <h1 className="text-xl font-bold text-white">Agent Chat</h1>
          <div className="flex items-center gap-2">
            <span
              className={`w-2 h-2 rounded-full ${
                connected ? 'bg-green-400' : 'bg-red-400'
              }`}
            />
            <span className="text-sm text-slate-400">
              {connected ? 'Connected' : 'Disconnected'}
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
          {/* Agent Subtype Badge */}
          {agentSubtype && (
            <div className={`flex items-center gap-2 px-3 py-1 rounded-full text-sm font-medium ${
              agentSubtype.subtype === 'finance'
                ? 'bg-purple-500/20 text-purple-400 border border-purple-500/50'
                : agentSubtype.subtype === 'code_engineer'
                ? 'bg-cyan-500/20 text-cyan-400 border border-cyan-500/50'
                : 'bg-pink-500/20 text-pink-400 border border-pink-500/50'
            }`}>
              <span>{
                agentSubtype.subtype === 'finance' ? '💰' :
                agentSubtype.subtype === 'code_engineer' ? '🛠️' : '📱'
              }</span>
              <span>{agentSubtype.label}</span>
            </div>
          )}
        </div>

        {/* Debug Toggle + Wallet Info */}
        <div className="flex items-center gap-4">
          {/* Debug Toggle - to the left of wallet */}
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

          {walletConnected && address ? (
            <div className="flex items-center gap-3">
              {/* Wallet Address */}
              <div className="flex items-center gap-2 bg-slate-700/50 px-3 py-1.5 rounded-lg">
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
              </div>

              {/* USDC Balance */}
              <div className="flex items-center gap-2 bg-slate-700/50 px-3 py-1.5 rounded-lg">
                <span className="text-sm font-semibold text-white">
                  {isCorrectNetwork ? formatBalance(usdcBalance) : '--'}
                </span>
                <span className="text-xs px-2 py-0.5 bg-blue-500/20 text-blue-400 rounded-full font-medium">
                  USDC · Base
                </span>
              </div>
            </div>
          ) : (
            <button
              onClick={connectWallet}
              className="flex items-center gap-2 bg-stark-500/20 hover:bg-stark-500/30 text-stark-400 px-3 py-1.5 rounded-lg text-sm font-medium transition-colors"
            >
              <Wallet className="w-4 h-4" />
              Connect Wallet
            </button>
          )}

          <SubagentBadge
            subagents={subagents}
            onSubagentCancelled={(id) => {
              setSubagents((prev) => prev.filter(s => s.id !== id));
            }}
          />

          <Button
            variant="ghost"
            size="sm"
            onClick={async () => {
              if (isLoading) {
                // Stop the execution
                try {
                  const result = await stopExecution();
                  if (result.success) {
                    setIsLoading(false);
                    addMessage('system', result.message || 'Execution stopped.');
                  }
                } catch (error) {
                  console.error('Failed to stop execution:', error);
                }
              } else {
                // Clear the chat
                setMessages([]);
                conversationHistory.current = [];
                localStorage.removeItem(STORAGE_KEY_MESSAGES);
                localStorage.removeItem(STORAGE_KEY_HISTORY);
                localStorage.removeItem(STORAGE_KEY_MODE);
                localStorage.removeItem(STORAGE_KEY_SUBTYPE);
                setAgentMode(null);
                setAgentSubtype(null);
              }
            }}
          >
            {isLoading ? (
              <>
                <Square className="w-4 h-4 mr-2" />
                Stop
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
      <div className="flex-1 overflow-y-auto p-6">
        {messages.length === 0 ? (
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
            {messages.map((message) => (
              <ChatMessage
                key={message.id}
                role={message.role}
                content={message.content}
                timestamp={message.timestamp}
              />
            ))}
            {isLoading && <TypingIndicator />}
          </>
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

      {/* Input */}
      <div className="px-6 pb-6">
        <div className="relative">
          {showAutocomplete && (
            <CommandAutocomplete
              commands={slashCommands}
              filter={input}
              selectedIndex={selectedCommandIndex}
              onSelect={handleCommandSelect}
              onClose={() => setShowAutocomplete(false)}
            />
          )}
          <div className="flex gap-3">
            <div className="flex-1 relative">
              <textarea
                ref={inputRef}
                value={input}
                onChange={(e) => handleInputChange(e.target.value)}
                onKeyDown={handleKeyDown}
                placeholder="Type a message or /command..."
                rows={1}
                className="w-full px-4 py-3 bg-slate-800 border border-slate-700 rounded-lg text-white placeholder-slate-500 focus:outline-none focus:ring-2 focus:ring-stark-500 focus:border-transparent resize-none"
                style={{ minHeight: '48px', maxHeight: '200px' }}
              />
            </div>
            <CommandMenu onCommandSelect={handleMenuCommand} />
            <Button
              onClick={handleSend}
              disabled={!input.trim() || isLoading}
              className="shrink-0"
            >
              <Send className="w-5 h-5" />
            </Button>
          </div>
        </div>
      </div>
    </div>
  );
}
