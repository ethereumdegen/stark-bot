import { useState, useCallback, useEffect, useRef } from 'react';
import { Send } from 'lucide-react';
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
  // Commands come newest-first; reverse so oldest is first
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
  const [sending, setSending] = useState(false);
  const scrollRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLTextAreaElement>(null);

  const { data: commands } = useApi<CommandLog[]>('/starflask/commands?limit=50');
  const { on, off } = useGateway();

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

    const onStarted = () => {
      // typing indicator is shown via `sending` state already
    };

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

    // Add user message immediately
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

      const result = await apiFetch<CommandOutput>('/starflask/command', {
        method: 'POST',
        body: JSON.stringify(body),
      });

      // Only add response if we didn't get it from WebSocket already
      // Use a small delay to let WS events arrive first
      setTimeout(() => {
        setSending((current) => {
          if (current) {
            // WS didn't deliver; use REST response
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
  }, [input]);

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      handleSend();
    }
  };

  return (
    <div className="flex flex-col h-[calc(100vh-4rem)]">
      {/* Header */}
      <div className="px-6 py-4 border-b border-slate-700/50">
        <h1 className="text-lg font-bold text-white">Chat</h1>
        <p className="text-slate-500 text-xs">Chat with your Starflask orchestrator</p>
      </div>

      {/* Message area */}
      <div ref={scrollRef} className="flex-1 overflow-y-auto px-6 py-4 space-y-1">
        {messages.length === 0 && !sending && (
          <div className="flex items-center justify-center h-full">
            <p className="text-slate-600 text-sm">Send a command to get started</p>
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
          {/* Text input */}
          <textarea
            ref={inputRef}
            value={input}
            onChange={(e) => setInput(e.target.value)}
            onKeyDown={handleKeyDown}
            placeholder="Send a command..."
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
