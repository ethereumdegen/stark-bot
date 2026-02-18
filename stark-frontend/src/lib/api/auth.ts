import { API_BASE, apiFetch } from './core';

// Auth API - SIWE (Sign In With Ethereum)
export async function generateChallenge(publicAddress: string): Promise<{ challenge: string }> {
  const response = await fetch(`${API_BASE}/auth/generate_challenge`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ public_address: publicAddress }),
  });

  if (!response.ok) {
    const data = await response.json();
    throw new Error(data.error || 'Failed to generate challenge');
  }

  const data = await response.json();
  if (!data.success || !data.challenge) {
    throw new Error(data.error || 'Failed to generate challenge');
  }

  return { challenge: data.challenge };
}

export async function validateAuth(
  publicAddress: string,
  challenge: string,
  signature: string
): Promise<{ token: string; expires_at: number }> {
  const response = await fetch(`${API_BASE}/auth/validate_auth`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({
      public_address: publicAddress,
      challenge,
      signature,
    }),
  });

  const data = await response.json();

  if (!response.ok || !data.success) {
    throw new Error(data.error || 'Authentication failed');
  }

  return { token: data.token, expires_at: data.expires_at };
}

export async function validateToken(): Promise<{ valid: boolean }> {
  return apiFetch('/auth/validate');
}

export async function logout(): Promise<void> {
  await apiFetch('/auth/logout', { method: 'POST' });
  localStorage.removeItem('stark_token');
}
