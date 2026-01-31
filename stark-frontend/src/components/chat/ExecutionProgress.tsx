import { useState, useEffect, useCallback, useRef } from 'react';
import { ChevronDown, ChevronRight, Loader2 } from 'lucide-react';
import clsx from 'clsx';
import { useGateway } from '@/hooks/useGateway';
import type { ExecutionTask, ExecutionEvent } from '@/types';

interface RunningTool {
  name: string;
  startTime: number;
}

interface ExecutionProgressProps {
  className?: string;
}

export default function ExecutionProgress({ className }: ExecutionProgressProps) {
  const [executions, setExecutions] = useState<Map<string, ExecutionTask>>(new Map());
  const [collapsed, setCollapsed] = useState<Set<string>>(new Set());
  const [visible, setVisible] = useState(false);
  const [runningTool, setRunningTool] = useState<RunningTool | null>(null);
  const [elapsedTime, setElapsedTime] = useState(0);
  const { on, off } = useGateway();
  const hideTimeoutRef = useRef<ReturnType<typeof setTimeout>>();
  const timerRef = useRef<ReturnType<typeof setInterval>>();

  const updateExecution = useCallback((executionId: string, updater: (task: ExecutionTask) => ExecutionTask) => {
    setExecutions((prev) => {
      const newMap = new Map(prev);
      const execution = newMap.get(executionId);
      if (execution) {
        newMap.set(executionId, updater(execution));
      }
      return newMap;
    });
  }, []);

  const handleExecutionStarted = useCallback((data: unknown) => {
    const event = data as ExecutionEvent;
    if (hideTimeoutRef.current) {
      clearTimeout(hideTimeoutRef.current);
    }
    setVisible(true);

    const newExecution: ExecutionTask = {
      id: event.execution_id,
      name: 'Processing',
      status: 'in_progress',
      startTime: Date.now(),
      children: [],
    };

    setExecutions((prev) => {
      const newMap = new Map(prev);
      newMap.set(event.execution_id, newExecution);
      return newMap;
    });
  }, []);

  const handleExecutionThinking = useCallback((data: unknown) => {
    const event = data as ExecutionEvent;
    updateExecution(event.execution_id, (execution) => ({
      ...execution,
      activeForm: event.active_form || 'Thinking...',
    }));
  }, [updateExecution]);

  const handleTaskStarted = useCallback((data: unknown) => {
    const event = data as ExecutionEvent;
    const newTask: ExecutionTask = {
      id: event.task_id || crypto.randomUUID(),
      parentId: event.parent_task_id,
      name: event.name || 'Task',
      activeForm: event.active_form,
      status: 'in_progress',
      startTime: Date.now(),
      children: [],
    };

    updateExecution(event.execution_id, (execution) => {
      const addToParent = (tasks: ExecutionTask[]): ExecutionTask[] => {
        return tasks.map((task) => {
          if (task.id === event.parent_task_id) {
            return { ...task, children: [...task.children, newTask] };
          }
          return { ...task, children: addToParent(task.children) };
        });
      };

      if (!event.parent_task_id || event.parent_task_id === execution.id) {
        return { ...execution, children: [...execution.children, newTask] };
      }
      return { ...execution, children: addToParent(execution.children) };
    });
  }, [updateExecution]);

  const handleTaskUpdated = useCallback((data: unknown) => {
    const event = data as ExecutionEvent;
    if (!event.task_id) return;

    updateExecution(event.execution_id, (execution) => {
      const updateTask = (tasks: ExecutionTask[]): ExecutionTask[] => {
        return tasks.map((task) => {
          if (task.id === event.task_id) {
            return {
              ...task,
              toolsCount: event.tools_count ?? task.toolsCount,
              tokensUsed: event.tokens_used ?? task.tokensUsed,
              activeForm: event.active_form ?? task.activeForm,
            };
          }
          return { ...task, children: updateTask(task.children) };
        });
      };

      if (execution.id === event.task_id) {
        return {
          ...execution,
          toolsCount: event.tools_count ?? execution.toolsCount,
          tokensUsed: event.tokens_used ?? execution.tokensUsed,
          activeForm: event.active_form ?? execution.activeForm,
        };
      }
      return { ...execution, children: updateTask(execution.children) };
    });
  }, [updateExecution]);

  const handleTaskCompleted = useCallback((data: unknown) => {
    const event = data as ExecutionEvent;
    if (!event.task_id) return;

    updateExecution(event.execution_id, (execution) => {
      const completeTask = (tasks: ExecutionTask[]): ExecutionTask[] => {
        return tasks.map((task) => {
          if (task.id === event.task_id) {
            return {
              ...task,
              status: 'completed',
              endTime: Date.now(),
              duration: event.duration_ms ?? (Date.now() - (task.startTime || Date.now())),
            };
          }
          return { ...task, children: completeTask(task.children) };
        });
      };

      return { ...execution, children: completeTask(execution.children) };
    });
  }, [updateExecution]);

  const handleExecutionCompleted = useCallback((data: unknown) => {
    const event = data as ExecutionEvent;

    updateExecution(event.execution_id, (execution) => ({
      ...execution,
      status: 'completed',
      endTime: Date.now(),
      duration: event.duration_ms ?? (Date.now() - (execution.startTime || Date.now())),
    }));

    // Hide after delay - keep visible longer to show task history
    hideTimeoutRef.current = setTimeout(() => {
      setVisible(false);
      // Clean up completed executions after hiding
      setTimeout(() => {
        setExecutions((prev) => {
          const newMap = new Map(prev);
          newMap.delete(event.execution_id);
          return newMap;
        });
      }, 500);
    }, 15000); // Keep visible for 15 seconds to show more task history
  }, [updateExecution]);

  // Handle tool execution start
  const handleToolExecution = useCallback((data: unknown) => {
    const event = data as { tool_name: string };
    console.log('[ExecutionProgress] Tool execution started:', event.tool_name);
    setRunningTool({
      name: event.tool_name,
      startTime: Date.now(),
    });
    setElapsedTime(0);
    setVisible(true);
    if (hideTimeoutRef.current) {
      clearTimeout(hideTimeoutRef.current);
    }
  }, []);

  // Handle tool result (clear running tool)
  const handleToolResult = useCallback((data: unknown) => {
    const event = data as { tool_name: string };
    console.log('[ExecutionProgress] Tool result received:', event.tool_name);
    setRunningTool(null);
    setElapsedTime(0);
    // Don't hide immediately - let the execution progress handle visibility
    // or hide after a short delay if no other executions
    if (executions.size === 0) {
      hideTimeoutRef.current = setTimeout(() => {
        setVisible(false);
      }, 500);
    }
  }, [executions.size]);

  // Timer effect for elapsed time
  useEffect(() => {
    if (runningTool) {
      timerRef.current = setInterval(() => {
        setElapsedTime(Date.now() - runningTool.startTime);
      }, 100); // Update every 100ms for smooth display
    } else {
      if (timerRef.current) {
        clearInterval(timerRef.current);
      }
    }
    return () => {
      if (timerRef.current) {
        clearInterval(timerRef.current);
      }
    };
  }, [runningTool]);

  useEffect(() => {
    on('execution.started', handleExecutionStarted);
    on('execution.thinking', handleExecutionThinking);
    on('execution.task_started', handleTaskStarted);
    on('execution.task_updated', handleTaskUpdated);
    on('execution.task_completed', handleTaskCompleted);
    on('execution.completed', handleExecutionCompleted);
    on('tool.execution', handleToolExecution);
    on('tool.result', handleToolResult);

    return () => {
      off('execution.started', handleExecutionStarted);
      off('execution.thinking', handleExecutionThinking);
      off('execution.task_started', handleTaskStarted);
      off('execution.task_updated', handleTaskUpdated);
      off('execution.task_completed', handleTaskCompleted);
      off('execution.completed', handleExecutionCompleted);
      off('tool.execution', handleToolExecution);
      off('tool.result', handleToolResult);

      if (hideTimeoutRef.current) {
        clearTimeout(hideTimeoutRef.current);
      }
      if (timerRef.current) {
        clearInterval(timerRef.current);
      }
    };
  }, [on, off, handleExecutionStarted, handleExecutionThinking, handleTaskStarted, handleTaskUpdated, handleTaskCompleted, handleExecutionCompleted, handleToolExecution, handleToolResult]);

  const toggleCollapse = (taskId: string) => {
    setCollapsed((prev) => {
      const newSet = new Set(prev);
      if (newSet.has(taskId)) {
        newSet.delete(taskId);
      } else {
        newSet.add(taskId);
      }
      return newSet;
    });
  };

  const formatDuration = (ms?: number): string => {
    if (!ms) return '';
    if (ms < 1000) return `${ms}ms`;
    return `${(ms / 1000).toFixed(1)}s`;
  };

  const renderTask = (task: ExecutionTask, depth: number = 0, isLast: boolean = true): JSX.Element => {
    const hasChildren = task.children.length > 0;
    const isCollapsed = collapsed.has(task.id);

    const statusIcon = {
      pending: <span className="text-slate-500">○</span>,
      in_progress: <span className="text-cyan-400 animate-pulse-subtle">●</span>,
      completed: <span className="text-green-400">✓</span>,
      error: <span className="text-red-400">✗</span>,
    };

    const prefix = depth > 0 ? (isLast ? '└─' : '├─') : '';
    const taskText = task.status === 'in_progress' && task.activeForm
      ? task.activeForm
      : task.name;

    return (
      <div key={task.id}>
        <div
          className={clsx(
            'py-1 text-sm font-mono',
            task.status === 'in_progress' && 'text-cyan-400'
          )}
          style={{ paddingLeft: `${depth * 16}px` }}
        >
          {/* Header row */}
          <div className="flex items-center gap-2">
            <span className="text-slate-600 shrink-0">{prefix}</span>
            {hasChildren && (
              <button
                onClick={() => toggleCollapse(task.id)}
                className="p-0.5 hover:bg-slate-700 rounded shrink-0"
              >
                {isCollapsed ? (
                  <ChevronRight className="w-3 h-3 text-slate-500" />
                ) : (
                  <ChevronDown className="w-3 h-3 text-slate-500" />
                )}
              </button>
            )}
            <span className="shrink-0">{statusIcon[task.status]}</span>
            {task.duration && (
              <span className="text-slate-600 text-xs shrink-0">
                {formatDuration(task.duration)}
              </span>
            )}
          </div>
          {/* Full task text - NO TRUNCATION */}
          <div
            className={clsx(
              'ml-6 whitespace-pre-wrap text-xs',
              task.status === 'completed' && 'text-slate-400',
              task.status === 'error' && 'text-red-400',
              task.status === 'in_progress' && 'text-cyan-300'
            )}
            style={{ wordBreak: 'break-word', overflowWrap: 'anywhere' }}
          >
            {taskText}
          </div>
        </div>
        {hasChildren && !isCollapsed && (
          <div>
            {task.children.map((child, idx) =>
              renderTask(child, depth + 1, idx === task.children.length - 1)
            )}
          </div>
        )}
      </div>
    );
  };

  // Format elapsed time for display
  const formatElapsedTime = (ms: number): string => {
    const seconds = Math.floor(ms / 1000);
    const tenths = Math.floor((ms % 1000) / 100);
    if (seconds < 60) {
      return `${seconds}.${tenths}s`;
    }
    const minutes = Math.floor(seconds / 60);
    const remainingSeconds = seconds % 60;
    return `${minutes}m ${remainingSeconds}s`;
  };

  // Show if we have executions or a running tool
  if (!visible && !runningTool) {
    return null;
  }

  if (executions.size === 0 && !runningTool) {
    return null;
  }

  return (
    <div
      className={clsx(
        'bg-slate-800/80 backdrop-blur border border-slate-700 rounded-lg p-4 transition-opacity duration-300 max-h-[120px] overflow-y-auto',
        (visible || runningTool) ? 'opacity-100' : 'opacity-0',
        className
      )}
    >
      {/* Running Tool Indicator */}
      {runningTool && (
        <div className="flex items-center gap-3 text-sm">
          <Loader2 className="w-4 h-4 text-cyan-400 animate-spin" />
          <span className="text-cyan-400 font-medium">Running:</span>
          <code className="text-cyan-300 bg-cyan-500/10 px-2 py-0.5 rounded">
            {runningTool.name}
          </code>
          <span className="text-slate-400 font-mono tabular-nums">
            {formatElapsedTime(elapsedTime)}
          </span>
        </div>
      )}

      {/* Existing execution tasks */}
      {executions.size > 0 && (
        <div className={runningTool ? 'mt-2 pt-2 border-t border-slate-700' : ''}>
          {Array.from(executions.values()).map((execution) => (
            <div key={execution.id}>
              {renderTask(execution)}
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
