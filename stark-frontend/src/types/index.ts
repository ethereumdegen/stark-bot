// Message types
export type MessageRole = 'user' | 'assistant' | 'system' | 'error' | 'hint' | 'command' | 'tool-indicator' | 'tool' | 'tool_call' | 'tool_result';

export interface ChatMessage {
  id: string;
  role: MessageRole;
  content: string;
  timestamp: Date;
  sessionId?: string;
  subagentLabel?: string;
}

// Gateway types
export interface GatewayMessage {
  id?: string;
  type?: 'event';
  event?: string;
  data?: unknown;
  result?: unknown;
  error?: {
    code: number;
    message: string;
  };
}

export interface RpcRequest {
  id: string;
  method: string;
  params?: Record<string, unknown>;
}

// Execution progress types
export interface ExecutionTask {
  id: string;
  parentId?: string;
  name: string;
  activeForm?: string;
  status: 'pending' | 'in_progress' | 'completed' | 'error';
  startTime?: number;
  endTime?: number;
  duration?: number;
  toolsCount?: number;
  tokensUsed?: number;
  children: ExecutionTask[];
}

export interface ExecutionEvent {
  execution_id: string;
  task_id?: string;
  parent_task_id?: string;
  name?: string;
  active_form?: string;
  status?: string;
  tools_count?: number;
  tokens_used?: number;
  duration_ms?: number;
  message?: string;
}

// x402 payment event
export interface X402PaymentEvent {
  channel_id: number;
  amount: string;
  amount_formatted: string;
  asset: string;
  pay_to: string;
  resource?: string;
  timestamp: string;
}

// Transaction events
export interface TxPendingEvent {
  channel_id: number;
  tx_hash: string;
  network: string;
  explorer_url: string;
  timestamp: string;
}

export interface TxConfirmedEvent {
  channel_id: number;
  tx_hash: string;
  network: string;
  status: 'confirmed' | 'reverted' | 'pending';
  timestamp: string;
}

// Transaction tracker state
export interface TrackedTransaction {
  tx_hash: string;
  network: string;
  explorer_url: string;
  status: 'pending' | 'confirmed' | 'reverted';
  timestamp: Date;
}

// Confirmation events
export interface ConfirmationRequiredEvent {
  channel_id: number;
  confirmation_id: string;
  tool_name: string;
  description: string;
  parameters: Record<string, unknown>;
  instructions: string;
  timestamp: string;
}

export interface ConfirmationApprovedEvent {
  channel_id: number;
  confirmation_id: string;
  tool_name: string;
  timestamp: string;
}

export interface ConfirmationRejectedEvent {
  channel_id: number;
  confirmation_id: string;
  tool_name: string;
  timestamp: string;
}

// Pending confirmation state
export interface PendingConfirmation {
  confirmation_id: string;
  channel_id: number;
  tool_name: string;
  description: string;
  parameters: Record<string, unknown>;
  timestamp: string;
}

// API types
export interface ApiResponse<T> {
  data?: T;
  error?: string;
}

export interface AuthValidateResponse {
  valid: boolean;
  user?: {
    id: string;
    role: string;
  };
}

export interface AgentSettings {
  provider?: string;
  model?: string;
  context_size?: number;
  temperature?: number;
}

export interface Tool {
  name: string;
  description?: string;
  enabled: boolean;
  builtin: boolean;
}

export interface Skill {
  id: string;
  name: string;
  description?: string;
  version?: string;
  enabled: boolean;
}

export interface Session {
  id: string;
  channel_type: string;
  channel_id: string;
  created_at: string;
  updated_at: string;
  message_count?: number;
  // Context management
  context_tokens?: number;
  max_context_tokens?: number;
  compaction_id?: number;
  // Completion status
  completion_status?: 'active' | 'complete';
}

// Task Planner types
export type TaskStatus = 'pending' | 'in_progress' | 'completed';

export interface PlannerTask {
  id: number;
  description: string;
  status: TaskStatus;
}

export interface TaskQueueUpdateEvent {
  channel_id: number;
  tasks: PlannerTask[];
  current_task_id?: number;
  timestamp: string;
}

export interface TaskStatusChangeEvent {
  channel_id: number;
  task_id: number;
  status: TaskStatus;
  description: string;
  timestamp: string;
}

export interface SessionCompleteEvent {
  channel_id: number;
  session_id: number;
  timestamp: string;
}

// Memory Graph types (Phase 3)
export interface GraphNode {
  id: number;
  content: string;
  memory_type: string;
  importance: number;
}

export interface GraphEdge {
  source: number;
  target: number;
  association_type: string;
  strength: number;
}

export interface MemoryGraphResponse {
  success: boolean;
  nodes: GraphNode[];
  edges: GraphEdge[];
  error?: string;
}

export interface HybridSearchItem {
  memory_id: number;
  content: string;
  memory_type: string;
  importance: number;
  rrf_score: number;
  fts_rank?: number;
  vector_similarity?: number;
  association_count?: number;
}

export interface HybridSearchResponse {
  success: boolean;
  query: string;
  mode: string;
  results: HybridSearchItem[];
  error?: string;
}

export interface EmbeddingStatsResponse {
  success: boolean;
  total_memories: number;
  with_embeddings: number;
  without_embeddings: number;
  coverage_pct: number;
}

export interface MemoryAssociation {
  id: number;
  source_memory_id: number;
  target_memory_id: number;
  association_type: string;
  strength: number;
  created_at: string;
}

// Branch/Worker events (Phase 2 gateway)
export interface BranchStartedEvent {
  channel_id: number;
  branch_id: string;
  label: string;
  task: string;
}

export interface BranchCompletedEvent {
  channel_id: number;
  branch_id: string;
  result_summary: string;
}

export interface BranchFailedEvent {
  channel_id: number;
  branch_id: string;
  error: string;
}

export interface CoalesceBufferingEvent {
  channel_id: number;
  pending_count: number;
}

export interface CoalesceFlushedEvent {
  channel_id: number;
  message_count: number;
}

export interface WorkerCheckpointEvent {
  channel_id: number;
  subagent_id: string;
  iteration: number;
}

// Cortex Bulletin
export interface CortexBulletin {
  content: string;
  generated_at: string;
  topics: string[];
}

export type MemoryType = 'daily_log' | 'long_term' | 'session_summary' | 'compaction';

export interface Memory {
  id: string;
  memory_type?: MemoryType;
  content: string;
  importance?: number;
  category?: string;
  tags?: string;
  created_at: string;
}

export interface Identity {
  id: string;
  name: string;
  description?: string;
  created_at: string;
}

export interface LogEntry {
  id: string;
  level: 'debug' | 'info' | 'warn' | 'error';
  message: string;
  timestamp: string;
  metadata?: Record<string, unknown>;
}

export interface Channel {
  id: string;
  type: 'telegram' | 'slack' | 'discord';
  name: string;
  enabled: boolean;
  config?: Record<string, unknown>;
}

// Slash command types
export interface SlashCommand {
  name: string;
  description: string;
  handler: () => void | Promise<void>;
}

// Cron Job types
export interface CronJob {
  id: number;
  job_id: string;
  name: string;
  description?: string;
  schedule_type: 'at' | 'every' | 'cron';
  schedule_value: string;
  timezone?: string;
  session_mode: 'main' | 'isolated';
  message?: string;
  system_event?: string;
  channel_id?: number;
  deliver_to?: string;
  deliver: boolean;
  model_override?: string;
  thinking_level?: string;
  timeout_seconds?: number;
  delete_after_run: boolean;
  status: 'active' | 'paused' | 'completed' | 'failed';
  last_run_at?: string;
  next_run_at?: string;
  created_at: string;
  updated_at: string;
}

export interface CronJobRun {
  id: number;
  cron_job_id: number;
  started_at: string;
  completed_at?: string;
  success: boolean;
  response?: string;
  error?: string;
  duration_ms?: number;
}

// Heartbeat Config types
export interface HeartbeatConfig {
  id: number;
  channel_id?: number;
  interval_minutes: number;
  target?: string;
  active_hours_start?: string;
  active_hours_end?: string;
  active_days?: string;
  enabled: boolean;
  last_beat_at?: string;
  next_beat_at?: string;
  created_at: string;
  updated_at: string;
}
