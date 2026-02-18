import { apiFetch } from './core';

// Sessions API
export async function getSessions(): Promise<Array<{
  id: number;
  channel_type: string;
  channel_id: number;
  platform_chat_id?: string;
  is_active?: boolean;
  completion_status?: string;
  created_at: string;
  updated_at: string;
  message_count?: number;
  initial_query?: string;
  safe_mode?: boolean;
}>> {
  return apiFetch('/sessions');
}

export async function getSession(id: number): Promise<{
  id: number;
  channel_type: string;
  channel_id: number;
  platform_chat_id?: string;
  is_active?: boolean;
  completion_status?: string;
  created_at: string;
  updated_at: string;
  message_count?: number;
  initial_query?: string;
  safe_mode?: boolean;
}> {
  return apiFetch(`/sessions/${id}`);
}

export async function deleteSession(id: string): Promise<{
  success: boolean;
  message: string;
  cancelled_agents?: number;
}> {
  return apiFetch(`/sessions/${id}`, { method: 'DELETE' });
}

export async function deleteAllSessions(): Promise<{
  success: boolean;
  message: string;
  deleted_count: number;
  cancelled_agents: number;
}> {
  return apiFetch('/sessions', { method: 'DELETE' });
}

// Get or create a session by channel type and ID
export async function getOrCreateSession(
  channelType: string,
  channelId: number,
  platformChatId: string
): Promise<{
  id: number;
  channel_type: string;
  channel_id: number;
  created_at: string;
  updated_at: string;
  message_count?: number;
}> {
  return apiFetch('/sessions', {
    method: 'POST',
    body: JSON.stringify({
      channel_type: channelType,
      channel_id: channelId,
      platform_chat_id: platformChatId,
    }),
  });
}

// Reset a session (marks old as inactive, creates new one with same settings)
export async function resetSession(id: number): Promise<{
  id: number;
  channel_type: string;
  channel_id: number;
  is_active: boolean;
  completion_status: string;
  created_at: string;
  updated_at: string;
}> {
  return apiFetch(`/sessions/${id}/reset`, { method: 'POST' });
}

// Stop a session (cancels execution and marks as cancelled)
export async function stopSession(id: number): Promise<{
  success: boolean;
  session?: {
    id: number;
    completion_status: string;
  };
  cancelled_agents?: number;
  error?: string;
}> {
  return apiFetch(`/sessions/${id}/stop`, { method: 'POST' });
}

// Resume a session (marks as active so it can continue processing)
export async function resumeSession(id: number): Promise<{
  success: boolean;
  session?: {
    id: number;
    completion_status: string;
  };
  error?: string;
}> {
  return apiFetch(`/sessions/${id}/resume`, { method: 'POST' });
}

// Web session response type
export interface WebSessionInfo {
  session_id: number;
  completion_status: string;
  message_count: number | null;
  created_at: string;
}

// Get the current active web chat session from the backend
// The backend tracks which session is active for the current user
export async function getActiveWebSession(): Promise<WebSessionInfo | null> {
  const response = await apiFetch<{
    success: boolean;
    session_id?: number;
    completion_status?: string;
    message_count?: number;
    created_at?: string;
    error?: string;
  }>('/chat/session');

  if (response.success && response.session_id) {
    return {
      session_id: response.session_id,
      completion_status: response.completion_status || 'active',
      message_count: response.message_count ?? null,
      created_at: response.created_at || new Date().toISOString(),
    };
  }
  return null;
}

// Create a new web session (resets the current one)
export async function createNewWebSession(): Promise<WebSessionInfo | null> {
  const response = await apiFetch<{
    success: boolean;
    session_id?: number;
    completion_status?: string;
    message_count?: number;
    created_at?: string;
    error?: string;
  }>('/chat/session/new', { method: 'POST' });

  if (response.success && response.session_id) {
    return {
      session_id: response.session_id,
      completion_status: response.completion_status || 'active',
      message_count: response.message_count ?? 0,
      created_at: response.created_at || new Date().toISOString(),
    };
  }
  return null;
}

// Legacy: Get the web chat session from sessions list (fallback)
export async function getWebSession(): Promise<{
  id: number;
  channel_type: string;
  channel_id: number;
  is_active?: boolean;
  completion_status?: string;
  created_at: string;
  updated_at: string;
  message_count?: number;
} | null> {
  // Find the active web session
  const sessions = await getSessions();
  // Prefer active session, fall back to any web session
  const activeWebSession = sessions.find(s => s.channel_type === 'web' && s.channel_id === 0 && s.is_active !== false);
  const webSession = activeWebSession || sessions.find(s => s.channel_type === 'web' && s.channel_id === 0);
  return webSession || null;
}

// Session Transcript API
export interface SessionMessage {
  id: number;
  session_id: number;
  role: string;
  content: string;
  created_at: string;
}

export interface SessionTranscriptResponse {
  session_id: number;
  messages: SessionMessage[];
  total_count: number;
}

export async function getSessionTranscript(sessionId: number, limit?: number): Promise<SessionTranscriptResponse> {
  const query = limit ? `?limit=${limit}` : '';
  return apiFetch(`/sessions/${sessionId}/transcript${query}`);
}
