export const API_BASE = '/api';

// Config Status API (unauthenticated)
export interface ConfigStatus {
  login_configured: boolean;
  burner_wallet_configured: boolean;
  guest_dashboard_enabled: boolean;
  wallet_address: string;
  wallet_mode: string;
}

export async function getConfigStatus(): Promise<ConfigStatus> {
  const response = await fetch(`${API_BASE}/health/config`);
  if (!response.ok) throw new Error('Failed to fetch config status');
  return response.json();
}

export async function apiFetch<T>(
  endpoint: string,
  options: RequestInit = {}
): Promise<T> {
  const token = localStorage.getItem('stark_token');

  const headers: HeadersInit = {
    'Content-Type': 'application/json',
    ...options.headers,
  };

  if (token) {
    (headers as Record<string, string>)['Authorization'] = `Bearer ${token}`;
  }

  const response = await fetch(`${API_BASE}${endpoint}`, {
    ...options,
    headers,
  });

  if (!response.ok) {
    if (response.status === 401) {
      localStorage.removeItem('stark_token');
      window.location.href = '/';
      throw new Error('Unauthorized');
    }
    const errorText = await response.text();
    throw new Error(errorText || `HTTP ${response.status}`);
  }

  // Handle empty responses
  const text = await response.text();
  if (!text) {
    return {} as T;
  }

  return JSON.parse(text);
}
