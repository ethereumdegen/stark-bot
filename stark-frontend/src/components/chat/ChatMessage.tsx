import { useState } from 'react';
import clsx from 'clsx';
import { Wrench, CheckCircle, XCircle, ChevronDown, ChevronUp, Lightbulb } from 'lucide-react';
import type { MessageRole } from '@/types';

// Threshold for collapsing large content
const COLLAPSE_CHAR_THRESHOLD = 400;
const COLLAPSE_LINE_THRESHOLD = 8;

interface ChatMessageProps {
  role: MessageRole;
  content: string;
  timestamp?: Date;
  subagentLabel?: string;
}

function parseMarkdown(text: string): string {
  // Escape HTML first to prevent XSS
  let parsed = text
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;');

  // Bold: **text**
  parsed = parsed.replace(/\*\*(.*?)\*\*/g, '<strong class="font-semibold text-slate-100">$1</strong>');

  // Italic: *text* (but not inside **)
  parsed = parsed.replace(/(?<!\*)\*([^*]+)\*(?!\*)/g, '<em>$1</em>');

  // Code blocks: ```code```
  parsed = parsed.replace(/```([\s\S]*?)```/g, '<pre class="bg-slate-900/80 p-3 rounded-lg my-2 overflow-x-auto text-sm font-mono text-slate-200">$1</pre>');

  // Inline code: `text`
  parsed = parsed.replace(/`([^`]+)`/g, '<code class="bg-slate-700/80 px-1.5 py-0.5 rounded text-cyan-300 text-sm font-mono">$1</code>');

  // Headers: ## text
  parsed = parsed.replace(/^### (.+)$/gm, '<h3 class="text-lg font-semibold text-slate-100 mt-4 mb-2">$1</h3>');
  parsed = parsed.replace(/^## (.+)$/gm, '<h2 class="text-xl font-bold text-slate-100 mt-4 mb-2">$1</h2>');

  // Bullet points: - text or • text
  parsed = parsed.replace(/^[-•] (.+)$/gm, '<li class="ml-4 list-disc text-slate-200">$1</li>');

  // Wrap consecutive <li> in <ul>
  parsed = parsed.replace(/(<li[^>]*>.*?<\/li>\n?)+/g, '<ul class="space-y-1 my-2">$&</ul>');

  // Markdown image syntax: ![alt](url)
  parsed = parsed.replace(
    /!\[([^\]]*)\]\(([^)]+)\)/g,
    '<a href="$2" target="_blank" rel="noopener noreferrer"><img src="$2" alt="$1" loading="lazy" class="max-w-full rounded-lg my-2 cursor-pointer hover:opacity-90 transition-opacity" /></a>'
  );

  // Auto-link URLs (after HTML escaping, before line breaks)
  parsed = parsed.replace(
    /(https?:\/\/[^\s<>"'`)\]]+)/g,
    '<a href="$1" target="_blank" rel="noopener noreferrer" class="text-cyan-400 hover:text-cyan-300 underline break-all">$1</a>'
  );

  // Auto-detect linked image URLs and render inline
  parsed = parsed.replace(
    /<a href="(https?:\/\/[^\s"]+\.(?:png|svg|jpe?g|gif|webp))"[^>]*>[^<]*<\/a>/gi,
    '<a href="$1" target="_blank" rel="noopener noreferrer"><img src="$1" alt="image" loading="lazy" class="max-w-full rounded-lg my-2 cursor-pointer hover:opacity-90 transition-opacity" /></a>'
  );

  // Auto-detect bare /public/ image paths and render inline
  parsed = parsed.replace(
    /(?<!["=\/])\/?public\/([\w\-.]+\.(?:png|svg|jpe?g|gif|webp))/gi,
    '<a href="/public/$1" target="_blank" rel="noopener noreferrer"><img src="/public/$1" alt="image" loading="lazy" class="max-w-full rounded-lg my-2 cursor-pointer hover:opacity-90 transition-opacity" /></a>'
  );

  // Line breaks
  parsed = parsed.replace(/\n/g, '<br/>');

  // Clean up excessive <br/> after block elements
  parsed = parsed.replace(/(<\/h[23]>)<br\/>/g, '$1');
  parsed = parsed.replace(/(<\/ul>)<br\/>/g, '$1');
  parsed = parsed.replace(/(<\/pre>)<br\/>/g, '$1');

  return parsed;
}

// Separate component for tool messages with accordion state
function ToolMessage({
  content,
  timestamp,
  isToolCall,
  isToolSuccess,
  isToolError,
  getToolBorderColor,
  subagentLabel,
}: {
  content: string;
  timestamp?: Date;
  isToolCall: boolean;
  isToolSuccess: boolean;
  isToolError: boolean;
  getToolBorderColor: () => string;
  subagentLabel?: string;
}) {
  // Remove emoji from content for cleaner display since we're using icons
  const cleanContent = content.replace(/^[✅❌🔧]\s*/, '');

  // Check if content is large enough to collapse
  const lineCount = cleanContent.split('\n').length;
  const isLargeContent = cleanContent.length > COLLAPSE_CHAR_THRESHOLD || lineCount > COLLAPSE_LINE_THRESHOLD;

  const [isExpanded, setIsExpanded] = useState(!isLargeContent);

  // Determine icon and colors based on status
  const getIcon = () => {
    if (isToolError) return <XCircle className="w-4 h-4 text-red-400" />;
    if (isToolSuccess) return <CheckCircle className="w-4 h-4 text-green-400" />;
    if (isToolCall) return <Wrench className="w-4 h-4 text-amber-400" />;
    return <Wrench className="w-4 h-4 text-slate-400" />;
  };

  const getTitle = () => {
    if (isToolCall) return 'Tool';
    return 'Result';
  };

  const getTitleColor = () => {
    if (isToolError) return 'text-red-300';
    if (isToolSuccess) return 'text-green-300';
    if (isToolCall) return 'text-amber-300';
    return 'text-slate-300';
  };

  // Get preview content (first few lines)
  const getPreviewContent = () => {
    const lines = cleanContent.split('\n');
    const previewLines = lines.slice(0, 3);
    return previewLines.join('\n') + (lines.length > 3 ? '...' : '');
  };

  return (
    <div className={clsx('flex mb-4 justify-start', subagentLabel && 'ml-6')}>
      <div
        className={clsx(
          'w-full rounded-r-xl rounded-l-sm border-l-4 bg-slate-800/95 text-slate-100 border border-slate-700/60',
          subagentLabel ? 'px-3 py-2' : 'px-4 py-3',
          getToolBorderColor()
        )}
      >
        {/* Icon header with title and expand/collapse button */}
        <div className="flex items-center justify-between mb-2">
          <div className="flex items-center gap-2">
            {getIcon()}
            <span className={clsx(subagentLabel ? 'text-xs font-semibold' : 'text-sm font-semibold', getTitleColor())}>
              {getTitle()}
            </span>
            {subagentLabel && (
              <span className="text-xs px-1.5 py-0.5 rounded bg-violet-900/50 text-violet-300 font-medium">
                {subagentLabel}
              </span>
            )}
            {/* Show status badge for results */}
            {!isToolCall && (
              <span className={clsx(
                'text-xs px-1.5 py-0.5 rounded',
                isToolSuccess && 'bg-green-900/50 text-green-300',
                isToolError && 'bg-red-900/50 text-red-300',
                !isToolSuccess && !isToolError && 'bg-green-900/50 text-green-300'
              )}>
                {isToolSuccess ? 'success' : isToolError ? 'error' : 'complete'}
              </span>
            )}
          </div>

          {/* Expand/Collapse button for large content */}
          {isLargeContent && (
            <button
              onClick={() => setIsExpanded(!isExpanded)}
              className="flex items-center gap-1 text-xs text-slate-400 hover:text-slate-200 transition-colors px-2 py-1 rounded hover:bg-slate-700/50"
            >
              {isExpanded ? (
                <>
                  <ChevronUp className="w-3 h-3" />
                  <span>Collapse</span>
                </>
              ) : (
                <>
                  <ChevronDown className="w-3 h-3" />
                  <span>Expand</span>
                </>
              )}
            </button>
          )}
        </div>

        {/* Content in dark box */}
        <div className={clsx(
          'bg-slate-900/80 rounded-lg p-3 mt-2 overflow-hidden transition-all duration-200',
          !isExpanded && 'max-h-24'
        )}>
          <div
            className="prose prose-sm prose-invert max-w-none leading-relaxed text-slate-200"
            dangerouslySetInnerHTML={{
              __html: parseMarkdown(isExpanded ? cleanContent : getPreviewContent())
            }}
          />
        </div>

        {/* Show "click to expand" hint when collapsed */}
        {isLargeContent && !isExpanded && (
          <button
            onClick={() => setIsExpanded(true)}
            className="w-full mt-1 text-xs text-slate-500 hover:text-slate-300 text-center py-1 transition-colors"
          >
            Click to show full content ({lineCount} lines)
          </button>
        )}

        {timestamp && (
          <p className="text-xs mt-2 text-slate-500">
            {timestamp.toLocaleTimeString()}
          </p>
        )}
      </div>
    </div>
  );
}

export default function ChatMessage({ role, content, timestamp, subagentLabel }: ChatMessageProps) {
  const isUser = role === 'user' || role === 'command';
  const isToolIndicator = role === 'tool-indicator';
  const isToolMessage = role === 'tool' || role === 'tool_call' || role === 'tool_result';

  // Detect success/failure for tool results
  const isToolSuccess = isToolMessage && (content.includes('✅') || content.includes('Success'));
  const isToolError = isToolMessage && (content.includes('❌') || content.includes('Failed') || content.includes('Error'));
  const isToolCall = role === 'tool_call' || (isToolMessage && content.includes('Tool Call'));

  const roleStyles: Record<MessageRole, string> = {
    user: 'bg-orange-500 text-black',
    assistant: 'bg-slate-800 text-slate-100',
    system: 'bg-slate-800/50 text-slate-300 border border-slate-700',
    error: 'bg-red-950/60 text-red-100 border border-red-900/50',
    hint: 'bg-amber-950/40 text-amber-100 border border-amber-700/50',
    command: 'bg-slate-700 text-slate-200',
    'tool-indicator': 'bg-slate-700/80 text-amber-300 border border-slate-600',
    tool: 'bg-slate-850 text-slate-100 border border-slate-700/60',
    tool_call: 'bg-slate-850 text-slate-100 border border-slate-700/60',
    tool_result: 'bg-slate-850 text-slate-100 border border-slate-700/60',
  };

  // Determine border color for tool messages
  const getToolBorderColor = () => {
    if (isToolError) return 'border-l-red-500';
    if (isToolSuccess) return 'border-l-green-500';
    if (isToolCall) return 'border-l-amber-500';
    return 'border-l-slate-500';
  };

  if (isToolIndicator) {
    return (
      <div className="flex justify-start mb-2">
        <div
          className={clsx(
            'inline-flex items-center gap-2 px-3 py-1.5 rounded-full text-sm',
            roleStyles[role]
          )}
        >
          <span className="w-2 h-2 bg-amber-400 rounded-full animate-pulse" />
          <span>{content}</span>
        </div>
      </div>
    );
  }

  // Render hint messages with lightbulb icon
  if (role === 'hint') {
    return (
      <div className="flex mb-4 justify-start">
        <div
          className={clsx(
            'max-w-[80%] px-4 py-3 rounded-2xl rounded-bl-md flex items-start gap-3',
            roleStyles[role]
          )}
        >
          <Lightbulb className="w-5 h-5 text-amber-400 shrink-0 mt-0.5" />
          <div>
            <div
              className="prose prose-sm prose-invert max-w-none leading-relaxed"
              dangerouslySetInnerHTML={{ __html: parseMarkdown(content) }}
            />
            {timestamp && (
              <p className="text-xs mt-2 text-amber-600/60">
                {timestamp.toLocaleTimeString()}
              </p>
            )}
          </div>
        </div>
      </div>
    );
  }

  // Render tool messages with icon header
  if (isToolMessage) {
    return (
      <ToolMessage
        content={content}
        timestamp={timestamp}
        isToolCall={isToolCall}
        isToolSuccess={isToolSuccess}
        isToolError={isToolError}
        getToolBorderColor={getToolBorderColor}
        subagentLabel={subagentLabel}
      />
    );
  }

  return (
    <div
      className={clsx(
        'flex mb-4',
        isUser ? 'justify-end' : 'justify-start'
      )}
    >
      <div
        className={clsx(
          'max-w-[80%] px-4 py-3 rounded-2xl',
          roleStyles[role],
          isUser ? 'rounded-br-md' : 'rounded-bl-md'
        )}
      >
        {role === 'assistant' || role === 'system' ? (
          <div
            className="prose prose-sm prose-invert max-w-none leading-relaxed"
            dangerouslySetInnerHTML={{ __html: parseMarkdown(content) }}
          />
        ) : (
          <p className="whitespace-pre-wrap break-words">{content}</p>
        )}
        {timestamp && (
          <p
            className={clsx(
              'text-xs mt-2',
              isUser ? 'text-white/60' : 'text-slate-500'
            )}
          >
            {timestamp.toLocaleTimeString()}
          </p>
        )}
      </div>
    </div>
  );
}
