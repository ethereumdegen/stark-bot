import { useState, useEffect } from 'react';
import { MessageSquare, Hash, Plus, Play, Square, Trash2, Save, Settings } from 'lucide-react';
import Card, { CardContent, CardHeader, CardTitle } from '@/components/ui/Card';
import Button from '@/components/ui/Button';
import Input from '@/components/ui/Input';
import {
  getChannels,
  createChannel,
  updateChannel,
  deleteChannel,
  startChannel,
  stopChannel,
  getChannelSettings,
  getChannelSettingsSchema,
  updateChannelSettings,
  ChannelInfo,
  ChannelSetting,
  ChannelSettingDefinition,
} from '@/lib/api';

const CHANNEL_TYPES = [
  { value: 'telegram', label: 'Telegram', icon: MessageSquare, color: 'blue' },
  { value: 'slack', label: 'Slack', icon: Hash, color: 'purple' },
  { value: 'discord', label: 'Discord', icon: MessageSquare, color: 'indigo' },
];

function getChannelHints(channelType: string): string[] {
  switch (channelType) {
    case 'discord':
      return [
        'In the Discord Developer Portal, enable Presence Intent, Server Members Intent, and Message Content Intent under Bot settings.',
        'Warning: Only install Starkbot in your own Discord server. The admin will have full control over the Agentic Loop and Tools.',
      ];
    default:
      return [];
  }
}

interface ChannelFormData {
  channel_type: string;
  name: string;
  bot_token: string;
  app_token: string;
}

const emptyForm: ChannelFormData = {
  channel_type: 'telegram',
  name: '',
  bot_token: '',
  app_token: '',
};

// Settings toggle with gear icon
function SettingsToggle({
  enabled,
  onToggle,
}: {
  enabled: boolean;
  onToggle: () => void;
}) {
  return (
    <button
      onClick={onToggle}
      className={`relative inline-flex h-6 w-11 items-center rounded-full transition-colors ${
        enabled ? 'bg-stark-500' : 'bg-slate-600'
      }`}
      title={enabled ? 'Hide settings' : 'Show settings'}
    >
      <span
        className={`inline-flex h-5 w-5 items-center justify-center rounded-full bg-white transition-transform ${
          enabled ? 'translate-x-5' : 'translate-x-0.5'
        }`}
      >
        <Settings className="h-3 w-3 text-slate-600" />
      </span>
    </button>
  );
}

export default function Channels() {
  const [channels, setChannels] = useState<ChannelInfo[]>([]);
  const [isLoading, setIsLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [showAddForm, setShowAddForm] = useState(false);
  const [newChannel, setNewChannel] = useState<ChannelFormData>(emptyForm);
  const [editingId, setEditingId] = useState<number | null>(null);
  const [editForm, setEditForm] = useState<ChannelFormData>(emptyForm);
  const [actionLoading, setActionLoading] = useState<number | null>(null);

  // Settings mode state
  const [settingsMode, setSettingsMode] = useState<number | null>(null);
  const [settingsSchema, setSettingsSchema] = useState<ChannelSettingDefinition[]>([]);
  const [settingsValues, setSettingsValues] = useState<Record<string, string>>({});
  const [settingsLoading, setSettingsLoading] = useState(false);

  const fetchChannels = async () => {
    try {
      const data = await getChannels();
      setChannels(data);
      setError(null);
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Failed to load channels');
    } finally {
      setIsLoading(false);
    }
  };

  useEffect(() => {
    fetchChannels();
  }, []);

  const handleCreate = async () => {
    if (!newChannel.name.trim() || !newChannel.bot_token.trim()) {
      setError('Name and bot token are required');
      return;
    }

    if (newChannel.channel_type === 'slack' && !newChannel.app_token.trim()) {
      setError('Slack requires an app token');
      return;
    }

    setActionLoading(-1);
    try {
      await createChannel({
        channel_type: newChannel.channel_type,
        name: newChannel.name,
        bot_token: newChannel.bot_token,
        app_token: newChannel.channel_type === 'slack' ? newChannel.app_token : undefined,
      });
      setNewChannel(emptyForm);
      setShowAddForm(false);
      await fetchChannels();
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Failed to create channel');
    } finally {
      setActionLoading(null);
    }
  };

  const handleUpdate = async (id: number) => {
    setActionLoading(id);
    try {
      await updateChannel(id, {
        name: editForm.name || undefined,
        bot_token: editForm.bot_token || undefined,
        app_token: editForm.app_token || undefined,
      });
      setEditingId(null);
      await fetchChannels();
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Failed to update channel');
    } finally {
      setActionLoading(null);
    }
  };

  const handleDelete = async (id: number) => {
    if (!confirm('Are you sure you want to delete this channel?')) return;

    setActionLoading(id);
    try {
      await deleteChannel(id);
      await fetchChannels();
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Failed to delete channel');
    } finally {
      setActionLoading(null);
    }
  };

  const handleStart = async (id: number) => {
    setActionLoading(id);
    try {
      await startChannel(id);
      await fetchChannels();
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Failed to start channel');
    } finally {
      setActionLoading(null);
    }
  };

  const handleStop = async (id: number) => {
    setActionLoading(id);
    try {
      await stopChannel(id);
      await fetchChannels();
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Failed to stop channel');
    } finally {
      setActionLoading(null);
    }
  };

  const startEditing = (channel: ChannelInfo) => {
    setEditingId(channel.id);
    setEditForm({
      channel_type: channel.channel_type,
      name: channel.name,
      bot_token: channel.bot_token,
      app_token: channel.app_token || '',
    });
  };

  // Toggle settings mode for a channel
  const toggleSettingsMode = async (channel: ChannelInfo) => {
    if (settingsMode === channel.id) {
      // Close settings mode
      setSettingsMode(null);
      setSettingsSchema([]);
      setSettingsValues({});
      return;
    }

    // Open settings mode
    setSettingsLoading(true);
    setSettingsMode(channel.id);

    try {
      // Load schema and current values in parallel
      const [schema, currentSettings] = await Promise.all([
        getChannelSettingsSchema(channel.channel_type),
        getChannelSettings(channel.id),
      ]);

      setSettingsSchema(schema);

      // Convert current settings array to a key-value map
      const valuesMap: Record<string, string> = {};
      currentSettings.forEach((s: ChannelSetting) => {
        valuesMap[s.setting_key] = s.setting_value;
      });
      setSettingsValues(valuesMap);
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Failed to load settings');
      setSettingsMode(null);
    } finally {
      setSettingsLoading(false);
    }
  };

  // Save channel settings
  const saveSettings = async (channelId: number) => {
    setActionLoading(channelId);
    try {
      const settingsArray = Object.entries(settingsValues).map(([key, value]) => ({
        key,
        value,
      }));
      await updateChannelSettings(channelId, settingsArray);
      setError(null);
      // Close settings mode after successful save
      setSettingsMode(null);
      setSettingsSchema([]);
      setSettingsValues({});
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Failed to save settings');
    } finally {
      setActionLoading(null);
    }
  };

  const getChannelIcon = (type: string) => {
    const channelType = CHANNEL_TYPES.find(c => c.value === type);
    if (!channelType) return { Icon: Hash, color: 'gray' };
    return { Icon: channelType.icon, color: channelType.color };
  };

  if (isLoading) {
    return (
      <div className="p-4 sm:p-8 flex items-center justify-center">
        <div className="flex items-center gap-3">
          <div className="w-6 h-6 border-2 border-stark-500 border-t-transparent rounded-full animate-spin" />
          <span className="text-slate-400">Loading channels...</span>
        </div>
      </div>
    );
  }

  return (
    <div className="p-4 sm:p-8">
      <div className="mb-6 sm:mb-8 flex flex-col sm:flex-row sm:items-center justify-between gap-4">
        <div>
          <h1 className="text-xl sm:text-2xl font-bold text-white mb-1 sm:mb-2">Channels</h1>
          <p className="text-sm sm:text-base text-slate-400">Configure messaging platform integrations</p>
        </div>
        <Button onClick={() => setShowAddForm(!showAddForm)} className="w-full sm:w-auto">
          <Plus className="w-4 h-4 mr-2" />
          Add Channel
        </Button>
      </div>

      {error && (
        <div className="mb-6 bg-red-500/20 border border-red-500/50 text-red-400 px-4 py-3 rounded-lg flex items-center justify-between">
          <span>{error}</span>
          <button onClick={() => setError(null)} className="text-red-400 hover:text-red-300">
            &times;
          </button>
        </div>
      )}

      {/* Add Channel Form */}
      {showAddForm && (
        <Card className="mb-6">
          <CardHeader>
            <CardTitle>Add New Channel</CardTitle>
          </CardHeader>
          <CardContent>
            <div className="space-y-4">
              <div>
                <label className="block text-sm font-medium text-slate-300 mb-2">Channel Type</label>
                <select
                  value={newChannel.channel_type}
                  onChange={(e) => setNewChannel({ ...newChannel, channel_type: e.target.value })}
                  className="w-full px-3 py-2 bg-slate-800 border border-slate-700 rounded-lg text-white focus:ring-2 focus:ring-stark-500 focus:border-transparent"
                >
                  {CHANNEL_TYPES.map(type => (
                    <option key={type.value} value={type.value}>{type.label}</option>
                  ))}
                </select>
              </div>
              <Input
                label="Name"
                value={newChannel.name}
                onChange={(e) => setNewChannel({ ...newChannel, name: e.target.value })}
                placeholder="My Telegram Bot"
              />
              <Input
                label="Bot Token"
                value={newChannel.bot_token}
                onChange={(e) => setNewChannel({ ...newChannel, bot_token: e.target.value })}
                placeholder={newChannel.channel_type === 'telegram' ? '123456:ABC-DEF...' : 'xoxb-...'}
              />
              {newChannel.channel_type === 'slack' && (
                <Input
                  label="App Token (for Socket Mode)"
                  value={newChannel.app_token}
                  onChange={(e) => setNewChannel({ ...newChannel, app_token: e.target.value })}
                  placeholder="xapp-..."
                />
              )}
              <div className="flex gap-2 justify-end">
                <Button variant="secondary" onClick={() => setShowAddForm(false)}>
                  Cancel
                </Button>
                <Button onClick={handleCreate} disabled={actionLoading === -1}>
                  {actionLoading === -1 ? 'Creating...' : 'Create Channel'}
                </Button>
              </div>
            </div>
          </CardContent>
        </Card>
      )}

      {/* Channel List */}
      <div className="grid gap-6">
        {channels.length === 0 ? (
          <Card>
            <CardContent className="py-12 text-center">
              <p className="text-slate-400">No channels configured yet. Click "Add Channel" to get started.</p>
            </CardContent>
          </Card>
        ) : (
          channels.map((channel) => {
            const { Icon, color } = getChannelIcon(channel.channel_type);
            const isEditing = editingId === channel.id;
            const isActionLoading = actionLoading === channel.id;

            return (
              <Card key={channel.id}>
                <CardHeader>
                  <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-3">
                    <div className="flex items-center gap-3">
                      <div className={`p-2 bg-${color}-500/20 rounded-lg shrink-0`}>
                        <Icon className={`w-5 h-5 text-${color}-400`} />
                      </div>
                      <div className="min-w-0">
                        <CardTitle className="truncate">{channel.name}</CardTitle>
                        <span className="text-sm text-slate-400 capitalize">{channel.channel_type}</span>
                      </div>
                    </div>
                    <div className="flex items-center gap-2 flex-wrap">
                      <span className={`px-2 py-1 rounded text-xs font-medium ${
                        channel.running
                          ? 'bg-green-500/20 text-green-400'
                          : 'bg-slate-700 text-slate-400'
                      }`}>
                        {channel.running ? 'Running' : 'Stopped'}
                      </span>
                      <SettingsToggle
                        enabled={settingsMode === channel.id}
                        onToggle={() => toggleSettingsMode(channel)}
                      />
                      {channel.running ? (
                        <Button
                          variant="secondary"
                          size="sm"
                          onClick={() => handleStop(channel.id)}
                          disabled={isActionLoading}
                        >
                          <Square className="w-4 h-4 sm:mr-1" />
                          <span className="hidden sm:inline">Stop</span>
                        </Button>
                      ) : (
                        <Button
                          variant="secondary"
                          size="sm"
                          onClick={() => handleStart(channel.id)}
                          disabled={isActionLoading}
                        >
                          <Play className="w-4 h-4 sm:mr-1" />
                          <span className="hidden sm:inline">Start</span>
                        </Button>
                      )}
                      <Button
                        variant="secondary"
                        size="sm"
                        onClick={() => handleDelete(channel.id)}
                        disabled={isActionLoading}
                        className="text-red-400 hover:text-red-300"
                      >
                        <Trash2 className="w-4 h-4" />
                      </Button>
                    </div>
                  </div>
                </CardHeader>
                <CardContent>
                  {settingsMode === channel.id ? (
                    // Settings mode view
                    <div className="space-y-4">
                      {settingsLoading ? (
                        <div className="flex items-center justify-center py-4">
                          <div className="w-5 h-5 border-2 border-stark-500 border-t-transparent rounded-full animate-spin" />
                          <span className="ml-2 text-slate-400">Loading settings...</span>
                        </div>
                      ) : settingsSchema.length === 0 ? (
                        <div className="text-center py-4 text-slate-400">
                          No configurable settings for {channel.channel_type} channels.
                        </div>
                      ) : (
                        <>
                          {settingsSchema.map((setting) => (
                            <div key={setting.key}>
                              <Input
                                label={setting.label}
                                value={settingsValues[setting.key] || ''}
                                onChange={(e) =>
                                  setSettingsValues({
                                    ...settingsValues,
                                    [setting.key]: e.target.value,
                                  })
                                }
                                placeholder={setting.placeholder}
                              />
                              <p className="mt-1 text-xs text-slate-500">{setting.description}</p>
                            </div>
                          ))}
                          <div className="flex gap-2 justify-end pt-2">
                            <Button
                              variant="secondary"
                              onClick={() => {
                                setSettingsMode(null);
                                setSettingsSchema([]);
                                setSettingsValues({});
                              }}
                            >
                              Cancel
                            </Button>
                            <Button
                              onClick={() => saveSettings(channel.id)}
                              disabled={isActionLoading}
                            >
                              <Save className="w-4 h-4 mr-1" />
                              Save Settings
                            </Button>
                          </div>
                        </>
                      )}
                    </div>
                  ) : isEditing ? (
                    <div className="space-y-4">
                      <Input
                        label="Name"
                        value={editForm.name}
                        onChange={(e) => setEditForm({ ...editForm, name: e.target.value })}
                      />
                      <Input
                        label="Bot Token"
                        value={editForm.bot_token}
                        onChange={(e) => setEditForm({ ...editForm, bot_token: e.target.value })}
                      />
                      {channel.channel_type === 'slack' && (
                        <Input
                          label="App Token"
                          value={editForm.app_token}
                          onChange={(e) => setEditForm({ ...editForm, app_token: e.target.value })}
                        />
                      )}
                      <div className="flex gap-2 justify-end">
                        <Button variant="secondary" onClick={() => setEditingId(null)}>
                          Cancel
                        </Button>
                        <Button onClick={() => handleUpdate(channel.id)} disabled={isActionLoading}>
                          <Save className="w-4 h-4 mr-1" />
                          Save
                        </Button>
                      </div>
                    </div>
                  ) : (
                    <div className="space-y-3">
                      <div>
                        <label className="block text-xs sm:text-sm font-medium text-slate-400 mb-1">Bot Token</label>
                        <code className="block px-2 sm:px-3 py-2 bg-slate-800 rounded text-xs sm:text-sm text-slate-300 font-mono break-all overflow-hidden">
                          {channel.bot_token}
                        </code>
                      </div>
                      {getChannelHints(channel.channel_type).map((hint, idx) => (
                        <div key={idx} className="px-3 py-2 bg-slate-700/50 border border-slate-600/50 rounded-lg">
                          <p className="text-xs text-slate-300">{hint}</p>
                        </div>
                      ))}
                      {channel.app_token && (
                        <div>
                          <label className="block text-xs sm:text-sm font-medium text-slate-400 mb-1">App Token</label>
                          <code className="block px-2 sm:px-3 py-2 bg-slate-800 rounded text-xs sm:text-sm text-slate-300 font-mono break-all overflow-hidden">
                            {channel.app_token}
                          </code>
                        </div>
                      )}
                      <div className="flex justify-end">
                        <Button variant="secondary" size="sm" onClick={() => startEditing(channel)}>
                          Edit
                        </Button>
                      </div>
                    </div>
                  )}
                </CardContent>
              </Card>
            );
          })
        )}
      </div>
    </div>
  );
}
