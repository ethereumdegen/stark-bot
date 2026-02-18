import { apiFetch } from './core';

// Transaction Queue API
export interface QueuedTransactionInfo {
  uuid: string;
  network: string;
  from: string;
  to: string;
  value: string;
  value_formatted: string;
  /** Hex-encoded calldata for function selector lookup */
  data: string;
  status: 'pending' | 'broadcasting' | 'broadcast' | 'confirmed' | 'failed' | 'expired';
  tx_hash?: string;
  explorer_url?: string;
  error?: string;
  created_at: string;
  broadcast_at?: string;
}

export interface QueuedTransactionsResponse {
  success: boolean;
  transactions: QueuedTransactionInfo[];
  total: number;
  pending_count: number;
  confirmed_count: number;
  failed_count: number;
}

export interface QueuedTransactionResponse {
  success: boolean;
  transaction?: QueuedTransactionInfo;
  error?: string;
}

export async function getQueuedTransactions(status?: string, limit?: number): Promise<QueuedTransactionsResponse> {
  const params = new URLSearchParams();
  if (status) params.set('status', status);
  if (limit) params.set('limit', String(limit));
  const query = params.toString();
  return apiFetch(`/tx-queue${query ? `?${query}` : ''}`);
}

export async function getPendingTransactions(): Promise<QueuedTransactionsResponse> {
  return apiFetch('/tx-queue/pending');
}

export async function getQueuedTransaction(uuid: string): Promise<QueuedTransactionResponse> {
  return apiFetch(`/tx-queue/${encodeURIComponent(uuid)}`);
}

// Broadcasted Transactions API (persistent history)
export interface BroadcastedTransactionInfo {
  id: number;
  uuid: string;
  network: string;
  from_address: string;
  to_address: string;
  value: string;
  value_formatted: string;
  tx_hash?: string;
  explorer_url?: string;
  status: 'broadcast' | 'confirmed' | 'failed';
  broadcast_mode: 'rogue' | 'partner';
  error?: string;
  broadcast_at: string;
  confirmed_at?: string;
  created_at: string;
}

export interface BroadcastedTransactionsResponse {
  success: boolean;
  transactions: BroadcastedTransactionInfo[];
  total: number;
}

export async function getBroadcastedTransactions(params?: {
  status?: string;
  network?: string;
  broadcast_mode?: string;
  limit?: number;
}): Promise<BroadcastedTransactionsResponse> {
  const queryParams = new URLSearchParams();
  if (params?.status) queryParams.set('status', params.status);
  if (params?.network) queryParams.set('network', params.network);
  if (params?.broadcast_mode) queryParams.set('broadcast_mode', params.broadcast_mode);
  if (params?.limit) queryParams.set('limit', String(params.limit));
  const query = queryParams.toString();
  return apiFetch(`/broadcasted-transactions${query ? `?${query}` : ''}`);
}

// x402 Payment Limits API
export interface X402PaymentLimit {
  asset: string;
  max_amount: string;
  decimals: number;
  display_name: string;
}

export interface X402PaymentLimitsResponse {
  limits: X402PaymentLimit[];
}

export async function getX402PaymentLimits(): Promise<X402PaymentLimitsResponse> {
  return apiFetch('/x402-limits');
}

export async function updateX402PaymentLimit(data: {
  asset: string;
  max_amount: string;
  decimals?: number;
  display_name?: string;
}): Promise<X402PaymentLimit> {
  return apiFetch('/x402-limits', {
    method: 'PUT',
    body: JSON.stringify(data),
  });
}
