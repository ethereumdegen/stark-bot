import { apiFetch } from './core';

// Task Queue API
export interface PlannerTaskInfo {
  id: number;
  description: string;
  status: string;
}

export interface GetPlannerTasksResponse {
  success: boolean;
  tasks: PlannerTaskInfo[];
}

export interface DeleteTaskResponse {
  success: boolean;
  message?: string;
  error?: string;
  was_current_task?: boolean;
}

export async function getPlannerTasks(): Promise<GetPlannerTasksResponse> {
  return apiFetch('/chat/tasks');
}

export async function deletePlannerTask(taskId: number): Promise<DeleteTaskResponse> {
  return apiFetch(`/chat/tasks/${taskId}`, { method: 'DELETE' });
}

// Subagent API
// Types imported from shared subagent-types.ts which matches Rust SubAgentStatus enum
import { Subagent, SubagentStatus } from '@/lib/subagent-types';
export { SubagentStatus };
export type SubagentInfo = Subagent;

export interface SubagentListResponse {
  success: boolean;
  subagents: SubagentInfo[];
}

export interface SubagentResponse {
  success: boolean;
  message?: string;
  error?: string;
}

export async function listSubagents(sessionId?: number): Promise<SubagentListResponse> {
  const params = sessionId != null ? `?session_id=${sessionId}` : '';
  return apiFetch(`/chat/subagents${params}`);
}

export async function cancelSubagent(subagentId: string): Promise<SubagentResponse> {
  return apiFetch('/chat/subagents/cancel', {
    method: 'POST',
    body: JSON.stringify({ subagent_id: subagentId }),
  });
}
