import { useState, useCallback, useEffect, useRef } from 'react';
import { Send, ChevronDown } from 'lucide-react';
import { apiFetch } from '@/lib/api';
import { useApi } from '@/hooks/useApi';
import { useGateway } from '@/hooks/useGateway';
import ChatMessageComponent from '@/components/chat/ChatMessage';
import TypingIndicator from '@/components/chat/TypingIndicator';
import type { ChatMessage } from '@/types';

interface CommandLog {
  id: number;
  capability: string;
  session_id?: string;
  message: string;
  status: string;
  result?: CommandOutput;
  created_at: string;
  updated_at: string;
}

interface CommandOutput {
  type: string;
  results?: unknown[];
  urls?: string[];
  media_type?: string;
  post_url?: string;
  confirmation?: string;
  text?: string;
  data?: unknown;
}

interface ChatAgent {
  capability: string;
  name: string;
  description: string;
  agent_id: string;
}

function formatCommandResult(result: CommandOutput): string {
  if (result.type === 'TextResponse' && result.text) {
    return result.text;
  }
  if (result.type === 'MediaGeneration' && result.urls) {
    const lines = result.urls.map((url) => `![generated](${url})`);
    return lines.join('\n');
  }
  return JSON.stringify(result, null, 2);
}

function commandsToMessages(commands: CommandLog[]): ChatMessage[] {
  const msgs: ChatMessage[] = [];
  const sorted = [...commands].reverse();
  for (const cmd of sorted) {
    msgs.push({
      id: `cmd-user-${cmd.id}`,
      role: 'user',
      content: cmd.message,
      timestamp: new Date(cmd.created_at),
    });
    if (cmd.status === 'failed') {
      msgs.push({
        id: `cmd-err-${cmd.id}`,
        role: 'error',
        content: cmd.result ? formatCommandResult(cmd.result) : 'Command failed',
        timestamp: new Date(cmd.updated_at),
      });
    } else if (cmd.result) {
      msgs.push({
        id: `cmd-resp-${cmd.id}`,
        role: 'assistant',
        content: formatCommandResult(cmd.result),
        timestamp: new Date(cmd.updated_at),
      });
    }
  }
  return msgs;
}

export default function CommandCenter() {
  const [messages, setMessages] = useState<ChatMessage[]>([]);
  const [input, setInput] = useState('');
  const [capability, setCapability] = useState('');
  const [sending, setSending] = useState(false);
  const [showAgentPicker, setShowAgentPicker] = useState(false);
  const scrollRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLTextAreaElement>(null);
  const pickerRef = useRef<HTMLDivElement>(null);

  const { data: commands } = useApi<CommandLog[]>('/starflask/commands?limit=50');
  const { data: chatAgents } = useApi<ChatAgent[]>('/starflask/chat_agents');
  const { on, off } = useGateway();

  // Auto-select first chat agent
  useEffect(() => {
    if (chatAgents?.length && !capability) {
      setCapability(chatAgents[0].capability);
    }
  }, [chatAgents, capability]);

  // Load history on mount
  useEffect(() => {
    if (commands) {
      setMessages(commandsToMessages(commands));
    }
  }, [commands]);

  // Auto-scroll to bottom
  useEffect(() => {
    const el = scrollRef.current;
    if (el) {
      el.scrollTop = el.scrollHeight;
    }
  }, [messages, sending]);

  // Close picker on outside click
  useEffect(() => {
    const handler = (e: MouseEvent) => {
      if (pickerRef.current && !pickerRef.current.contains(e.target as Node)) {
        setShowAgentPicker(false);
      }
    };
    document.addEventListener('mousedown', handler);
    return () => document.removeEventListener('mousedown', handler);
  }, []);

  // WebSocket events
  useEffect(() => {
    const onDelegation = (data: unknown) => {
      const d = data as { capability?: string };
      if (d.capability) {
        setMessages((prev) => [
          ...prev,
          {
            id: `delegation-${Date.now()}`,
            role: 'system',
            content: `Routing to ${d.capability}...`,
            timestamp: new Date(),
          },
        ]);
      }
    };

    const onStarted = () => {};

    const onCompleted = (data: unknown) => {
      const d = data as { result?: CommandOutput; error?: string };
      setSending(false);
      if (d.error) {
        setMessages((prev) => [
          ...prev,
          {
            id: `ws-err-${Date.now()}`,
            role: 'error',
            content: d.error!,
            timestamp: new Date(),
          },
        ]);
      } else if (d.result) {
        setMessages((prev) => [
          ...prev,
          {
            id: `ws-resp-${Date.now()}`,
            role: 'assistant',
            content: formatCommandResult(d.result!),
            timestamp: new Date(),
          },
        ]);
      }
    };

    on('starflask.delegation', onDelegation);
    on('starflask.command_started', onStarted);
    on('starflask.command_completed', onCompleted);

    return () => {
      off('starflask.delegation', onDelegation);
      off('starflask.command_started', onStarted);
      off('starflask.command_completed', onCompleted);
    };
  }, [on, off]);

  const handleSend = useCallback(async () => {
    const text = input.trim();
    if (!text) return;

    const userMsg: ChatMessage = {
      id: `user-${Date.now()}`,
      role: 'user',
      content: text,
      timestamp: new Date(),
    };
    setMessages((prev) => [...prev, userMsg]);
    setInput('');
    setSending(true);

    try {
      const body: Record<string, unknown> = { message: text };
      if (capability) body.capability = capability;

      const result = await apiFetch<CommandOutput>('/starflask/command', {
        method: 'POST',
        body: JSON.stringify(body),
      });

      setTimeout(() => {
        setSending((current) => {
          if (current) {
            setMessages((prev) => [
              ...prev,
              {
                id: `resp-${Date.now()}`,
                role: 'assistant',
                content: formatCommandResult(result),
                timestamp: new Date(),
              },
            ]);
            return false;
          }
          return current;
        });
      }, 300);
    } catch (e) {
      setSending(false);
      setMessages((prev) => [
        ...prev,
        {
          id: `err-${Date.now()}`,
          role: 'error',
          content: e instanceof Error ? e.message : 'Command failed',
          timestamp: new Date(),
        },
      ]);
    }
  }, [input, capability]);

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      handleSend();
    }
  };

  const selectedAgent = chatAgents?.find((a) => a.capability === capability);
  const showPicker = chatAgents && chatAgents.length > 1;

  return (
    <div className="flex flex-col h-[calc(100vh-4rem)]">
      {/* Header */}
      <div className="px-6 py-4 border-b border-slate-700/50">
        <h1 className="text-lg font-bold text-white">Chat</h1>
        <p className="text-slate-500 text-xs">
          {selectedAgent ? `Chatting with ${selectedAgent.name}` : 'Chat with your Starflask orchestrator'}
        </p>
      </div>

      {/* Message area */}
      <div ref={scrollRef} className="flex-1 overflow-y-auto px-6 py-4 space-y-1">
        {messages.length === 0 && !sending && (
          <div className="flex items-center justify-center h-full">
            <p className="text-slate-600 text-sm">Send a message to get started</p>
          </div>
        )}
        {messages.map((msg) => (
          <ChatMessageComponent
            key={msg.id}
            role={msg.role}
            content={msg.content}
            timestamp={msg.timestamp}
          />
        ))}
        {sending && <TypingIndicator />}
      </div>

      {/* Input bar */}
      <div className="border-t border-slate-700/50 px-6 py-3 bg-slate-900/80">
        <div className="flex items-end gap-2">
          {/* Agent picker — only shown when multiple chat agents exist */}
          {showPicker && (
            <div className="relative" ref={pickerRef}>
              <button
                onClick={() => setShowAgentPicker(!showAgentPicker)}
                className="flex items-center gap-1.5 px-3 py-2 rounded-full text-xs font-medium border transition-colors bg-stark-500/20 text-stark-400 border-stark-500/30"
              >
                {selectedAgent?.name || 'Agent'}
                <ChevronDown className="w-3 h-3" />
              </button>
              {showAgentPicker && (
                <div className="absolute bottom-full mb-2 left-0 bg-slate-800 border border-slate-700 rounded-lg shadow-xl py-1 min-w-[200px] z-50">
                  {chatAgents!.map((agent) => (
                    <button
                      key={agent.capability}
                      onClick={() => {
                        setCapability(agent.capability);
                        setShowAgentPicker(false);
                      }}
                      className={`w-full text-left px-3 py-2 text-sm transition-colors ${
                        capability === agent.capability
                          ? 'bg-slate-700 text-white'
                          : 'text-slate-300 hover:bg-slate-700/50 hover:text-white'
                      }`}
                    >
                      <span className="inline-block w-2 h-2 rounded-full mr-2 bg-stark-400" />
                      {agent.name}
                    </button>
                  ))}
                </div>
              )}
            </div>
          )}

          {/* Text input */}
          <textarea
            ref={inputRef}
            value={input}
            onChange={(e) => setInput(e.target.value)}
            onKeyDown={handleKeyDown}
            placeholder="Send a message..."
            rows={1}
            className="flex-1 px-4 py-2 rounded-xl bg-slate-800 border border-slate-700 text-white placeholder-slate-500 focus:outline-none focus:border-stark-500/50 resize-none text-sm leading-6 max-h-[4.5rem] overflow-y-auto"
          />

          {/* Send button */}
          <button
            onClick={handleSend}
            disabled={sending || !input.trim()}
            className="p-2.5 rounded-xl bg-stark-500 hover:bg-stark-600 text-white transition-colors disabled:opacity-40 disabled:cursor-not-allowed"
          >
            <Send className="w-4 h-4" />
          </button>
        </div>
      </div>
    </div>
  );
}
