import { apiFetch } from './core';

// Channels API
export interface ChannelInfo {
  id: number;
  channel_type: string;
  name: string;
  enabled: boolean;
  bot_token: string;
  app_token?: string;
  created_at: string;
  updated_at: string;
  running?: boolean;
}

// Channel Settings types
export interface ChannelSetting {
  channel_id: number;
  setting_key: string;
  setting_value: string;
}

export interface SelectOption {
  value: string;
  label: string;
}

export interface ChannelSettingDefinition {
  key: string;
  label: string;
  description: string;
  input_type: 'text' | 'text_area' | 'toggle' | 'number' | 'select';
  placeholder: string;
  options?: SelectOption[];
  default_value?: string;
}

export interface ChannelSettingsResponse {
  success: boolean;
  settings: ChannelSetting[];
}

export interface ChannelSettingsSchemaResponse {
  success: boolean;
  channel_type: string;
  settings: ChannelSettingDefinition[];
}

interface ChannelsListResponse {
  success: boolean;
  channels?: ChannelInfo[];
  error?: string;
}

interface ChannelOperationResponse {
  success: boolean;
  channel?: ChannelInfo;
  error?: string;
}

export async function getChannels(): Promise<ChannelInfo[]> {
  const response = await apiFetch<ChannelsListResponse>('/channels');
  return response.channels || [];
}

export async function getChannel(id: number): Promise<ChannelInfo | null> {
  const response = await apiFetch<ChannelOperationResponse>(`/channels/${id}`);
  return response.channel || null;
}

export async function createChannel(data: {
  channel_type: string;
  name: string;
  bot_token?: string;
  app_token?: string;
}): Promise<ChannelInfo> {
  const response = await apiFetch<ChannelOperationResponse>('/channels', {
    method: 'POST',
    body: JSON.stringify(data),
  });
  if (!response.success || !response.channel) {
    throw new Error(response.error || 'Failed to create channel');
  }
  return response.channel;
}

export async function updateChannel(id: number, config: {
  name?: string;
  enabled?: boolean;
  bot_token?: string;
  app_token?: string;
}): Promise<ChannelInfo> {
  const response = await apiFetch<ChannelOperationResponse>(`/channels/${id}`, {
    method: 'PUT',
    body: JSON.stringify(config),
  });
  if (!response.success || !response.channel) {
    throw new Error(response.error || 'Failed to update channel');
  }
  return response.channel;
}

export async function deleteChannel(id: number): Promise<void> {
  const response = await apiFetch<ChannelOperationResponse>(`/channels/${id}`, {
    method: 'DELETE',
  });
  if (!response.success) {
    throw new Error(response.error || 'Failed to delete channel');
  }
}

export async function startChannel(id: number): Promise<ChannelInfo> {
  const response = await apiFetch<ChannelOperationResponse>(`/channels/${id}/start`, {
    method: 'POST',
  });
  if (!response.success || !response.channel) {
    throw new Error(response.error || 'Failed to start channel');
  }
  return response.channel;
}

export async function stopChannel(id: number): Promise<ChannelInfo> {
  const response = await apiFetch<ChannelOperationResponse>(`/channels/${id}/stop`, {
    method: 'POST',
  });
  if (!response.success || !response.channel) {
    throw new Error(response.error || 'Failed to stop channel');
  }
  return response.channel;
}

// Channel Settings API
export async function getChannelSettingsSchema(channelType: string): Promise<ChannelSettingDefinition[]> {
  const response = await apiFetch<ChannelSettingsSchemaResponse>(`/channels/settings/schema/${channelType}`);
  return response.settings || [];
}

export async function getChannelSettings(channelId: number): Promise<ChannelSetting[]> {
  const response = await apiFetch<ChannelSettingsResponse>(`/channels/${channelId}/settings`);
  return response.settings || [];
}

export async function updateChannelSettings(
  channelId: number,
  settings: Array<{ key: string; value: string }>
): Promise<ChannelSetting[]> {
  const response = await apiFetch<ChannelSettingsResponse>(`/channels/${channelId}/settings`, {
    method: 'PUT',
    body: JSON.stringify({ settings }),
  });
  return response.settings || [];
}
