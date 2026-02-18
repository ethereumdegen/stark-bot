import { apiFetch } from './core';

// Chat API
export async function sendChatMessage(
  content: string,
  conversationHistory: Array<{ role: string; content: string }>,
  network?: string  // The currently selected network from the UI
): Promise<{ response: string }> {
  // Backend expects { messages: [...] } with the full conversation including the new message
  const messages = [
    ...conversationHistory,
    { role: 'user', content }
  ];

  const response = await apiFetch<{ success: boolean; message?: { content: string }; error?: string }>('/chat', {
    method: 'POST',
    body: JSON.stringify({ messages, network }),
  });

  if (!response.success || !response.message) {
    throw new Error(response.error || 'Failed to get response');
  }

  return { response: response.message.content };
}

// Confirmation API
export interface ConfirmationResponse {
  success: boolean;
  message?: string;
  error?: string;
  result?: string;
}

export interface PendingConfirmationResponse {
  has_pending: boolean;
  confirmation?: {
    id: string;
    channel_id: number;
    tool_name: string;
    description: string;
    parameters: Record<string, unknown>;
  };
}

export async function getPendingConfirmation(channelId: number): Promise<PendingConfirmationResponse> {
  return apiFetch(`/confirmation/pending/${channelId}`);
}

export async function confirmTransaction(channelId: number): Promise<ConfirmationResponse> {
  return apiFetch('/confirmation/confirm', {
    method: 'POST',
    body: JSON.stringify({ channel_id: channelId }),
  });
}

export async function cancelTransaction(channelId: number): Promise<ConfirmationResponse> {
  return apiFetch('/confirmation/cancel', {
    method: 'POST',
    body: JSON.stringify({ channel_id: channelId }),
  });
}

// Execution Control API
export interface StopExecutionResponse {
  success: boolean;
  message?: string;
  error?: string;
}

export async function stopExecution(): Promise<StopExecutionResponse> {
  return apiFetch('/chat/stop', {
    method: 'POST',
  });
}

// Execution Status API
export interface ExecutionStatusResponse {
  running: boolean;
  execution_id: string | null;
}

export async function getExecutionStatus(): Promise<ExecutionStatusResponse> {
  return apiFetch('/chat/execution-status');
}
