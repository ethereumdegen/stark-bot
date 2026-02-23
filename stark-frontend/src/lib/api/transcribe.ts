import { API_BASE } from './core';

// Transcribe API (voice-to-text)
export async function transcribeAudio(blob: Blob): Promise<{ text: string }> {
  const token = localStorage.getItem('stark_token');
  const formData = new FormData();
  formData.append('audio', blob, 'recording.webm');

  const response = await fetch(`${API_BASE}/transcribe`, {
    method: 'POST',
    headers: token ? { Authorization: `Bearer ${token}` } : {},
    body: formData,
  });

  if (!response.ok) {
    const data = await response.json().catch(() => ({ error: `HTTP ${response.status}` }));
    throw new Error(data.error || 'Transcription failed');
  }

  const data = await response.json();
  if (!data.success) {
    throw new Error(data.error || 'Transcription failed');
  }
  return { text: data.text || '' };
}
