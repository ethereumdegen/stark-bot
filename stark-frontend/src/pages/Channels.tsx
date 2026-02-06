import { useState, useEffect } from 'react';
import { MessageSquare, Hash, Plus, Play, Square, Trash2, Save, Pencil, Twitter, AlertTriangle } from 'lucide-react';
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
  getApiKeys,
  ApiKey,
} from '@/lib/api';

const CHANNEL_TYPES = [
  { value: 'telegram', label: 'Telegram', icon: MessageSquare, color: 'blue' },
  { value: 'slack', label: 'Slack', icon: Hash, color: 'purple' },
  { value: 'discord', label: 'Discord', icon: MessageSquare, color: 'indigo' },
  { value: 'twitter', label: 'Twitter / X', icon: Twitter, color: 'sky' },
];

function getChannelHints(channelType: string): string[] {
  switch (channelType) {
    case 'discord':
      return [
        'In the Discord Developer Portal, enable Presence Intent, Server Members Intent, and Message Content Intent under Bot settings.',
        'Warning: Only install Starkbot in your own Discord server. The admin will have full control over the Agentic Loop and Tools.',
      ];
    case 'twitter':
      return [
        'Requires the X API v2 pay-per-usage plan. Buy credits and check your balance at <a href="https://console.x.com" target="_blank">console.x.com</a>.',
        'Set your 4 OAuth 1.0a keys (Consumer Key, Consumer Secret, Access Token, Access Token Secret) on the API Keys page.',
        'Configure the Bot Handle (e.g. "starkbot") and Bot User ID (numeric) in channel settings after creation.',
      ];
    case 'telegram':
      return [
        'To use in a group, set an <strong>Admin User ID</strong> in channel settings. Only the admin gets full agent access; all other users are restricted to safe mode. Without an admin configured, all users have full unrestricted access.',
      ];
    default:
      return [];
  }
}

interface ChannelFormData {
  channel_type: string;
  name: string;
  settings: Record<string, string>;
}

const emptyForm: ChannelFormData = {
  channel_type: 'telegram',
  name: '',
  settings: {},
};

const TWITTER_REQUIRED_KEYS = [
  'TWITTER_CONSUMER_KEY',
  'TWITTER_CONSUMER_SECRET',
  'TWITTER_ACCESS_TOKEN',
  'TWITTER_ACCESS_TOKEN_SECRET',
];

function TwitterKeyWarning({ apiKeys }: { apiKeys: ApiKey[] }) {
  const installedNames = new Set(apiKeys.map(k => k.key_name));
  const missing = TWITTER_REQUIRED_KEYS.filter(k => !installedNames.has(k));

  if (missing.length === 0) return null;

  return (
    <div className="px-4 py-3 bg-amber-500/10 border border-amber-500/40 rounded-lg flex items-start gap-3">
      <AlertTriangle className="w-5 h-5 text-amber-400 flex-shrink-0 mt-0.5" />
      <div className="text-sm">
        <p className="text-amber-300 font-medium mb-1">
          Twitter API keys not configured
        </p>
        <p className="text-slate-400 mb-2">
          {missing.length === TWITTER_REQUIRED_KEYS.length
            ? 'None of the required OAuth 1.0a keys are installed.'
            : `Missing ${missing.length} of ${TWITTER_REQUIRED_KEYS.length} required keys.`}
        </p>
        <p className="text-slate-400">
          Go to the{' '}
          <a href="/api-keys" className="text-blue-400 underline hover:text-blue-300">
            API Keys
          </a>{' '}
          page to add your Twitter Consumer Key, Consumer Secret, Access Token, and Access Token Secret before starting this channel.
        </p>
      </div>
    </div>
  );
}

export default function Channels() {
  const [channels, setChannels] = useState<ChannelInfo[]>([]);
  const [isLoading, setIsLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [showAddForm, setShowAddForm] = useState(false);
  const [newChannel, setNewChannel] = useState<ChannelFormData>(emptyForm);
  const [newChannelSchema, setNewChannelSchema] = useState<ChannelSettingDefinition[]>([]);
  const [editingId, setEditingId] = useState<number | null>(null);
  const [editForm, setEditForm] = useState<ChannelFormData>(emptyForm);
  const [editSchema, setEditSchema] = useState<ChannelSettingDefinition[]>([]);
  const [editLoading, setEditLoading] = useState(false);
  const [actionLoading, setActionLoading] = useState<number | null>(null);
  const [apiKeys, setApiKeys] = useState<ApiKey[]>([]);

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
    getApiKeys().then(setApiKeys).catch(() => {});
  }, []);

  const handleCreate = async () => {
    if (!newChannel.name.trim()) {
      setError('Name is required');
      return;
    }

    setActionLoading(-1);
    try {
      const createdChannel = await createChannel({
        channel_type: newChannel.channel_type,
        name: newChannel.name,
      });

      // Save settings if any were configured
      const settingsToSave = Object.entries(newChannel.settings)
        .filter(([, value]) => value.trim() !== '')
        .map(([key, value]) => ({ key, value }));
      if (settingsToSave.length > 0) {
        await updateChannelSettings(createdChannel.id, settingsToSave);
      }

      setNewChannel(emptyForm);
      setNewChannelSchema([]);
      setShowAddForm(false);
      await fetchChannels();
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Failed to create channel');
    } finally {
      setActionLoading(null);
    }
  };

  // Fetch settings schema when channel type changes (for new channel form)
  const handleChannelTypeChange = async (channelType: string) => {
    setNewChannel({ ...newChannel, channel_type: channelType, settings: {} });
    try {
      const schema = await getChannelSettingsSchema(channelType);
      setNewChannelSchema(schema);
    } catch (e) {
      setNewChannelSchema([]);
    }
  };

  const handleUpdate = async (id: number) => {
    setActionLoading(id);
    try {
      // Update channel data
      await updateChannel(id, {
        name: editForm.name || undefined,
      });

      // Update settings
      const settingsToSave = Object.entries(editForm.settings).map(([key, value]) => ({
        key,
        value,
      }));
      await updateChannelSettings(id, settingsToSave);

      setEditingId(null);
      setEditSchema([]);
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

  // Toggle edit mode for a channel (opens modal with channel data + settings)
  const toggleEditMode = async (channel: ChannelInfo) => {
    if (editingId === channel.id) {
      // Close edit mode
      setEditingId(null);
      setEditSchema([]);
      return;
    }

    // Open edit mode and load data
    setEditLoading(true);
    setEditingId(channel.id);

    try {
      // Load schema and current values in parallel
      const [schema, currentSettings] = await Promise.all([
        getChannelSettingsSchema(channel.channel_type),
        getChannelSettings(channel.id),
      ]);

      setEditSchema(schema);

      // Convert current settings array to a key-value map
      const settingsMap: Record<string, string> = {};
      currentSettings.forEach((s: ChannelSetting) => {
        settingsMap[s.setting_key] = s.setting_value;
      });

      setEditForm({
        channel_type: channel.channel_type,
        name: channel.name,
        settings: settingsMap,
      });
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Failed to load channel data');
      setEditingId(null);
    } finally {
      setEditLoading(false);
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
                  onChange={(e) => handleChannelTypeChange(e.target.value)}
                  className="w-full px-3 py-2 bg-slate-800 border border-slate-700 rounded-lg text-white focus:ring-2 focus:ring-stark-500 focus:border-transparent"
                >
                  {CHANNEL_TYPES.map(type => (
                    <option key={type.value} value={type.value}>{type.label}</option>
                  ))}
                </select>
              </div>
              {newChannel.channel_type === 'twitter' && (
                <TwitterKeyWarning apiKeys={apiKeys} />
              )}
              <Input
                label="Name"
                value={newChannel.name}
                onChange={(e) => setNewChannel({ ...newChannel, name: e.target.value })}
                placeholder="My Telegram Bot"
              />
              {/* Settings section for new channel */}
              {newChannelSchema.length > 0 && (
                <>
                  <div className="border-t border-slate-700 pt-4 mt-4">
                    <h4 className="text-sm font-medium text-slate-300 mb-3">Settings</h4>
                  </div>
                  {newChannelSchema.map((setting) => (
                    <div key={setting.key}>
                      {setting.input_type === 'toggle' ? (
                        <div className="flex items-center justify-between">
                          <div>
                            <label className="block text-sm font-medium text-slate-300">
                              {setting.label}
                            </label>
                            <p className="mt-1 text-xs text-slate-500">{setting.description}</p>
                          </div>
                          <button
                            type="button"
                            onClick={() =>
                              setNewChannel({
                                ...newChannel,
                                settings: {
                                  ...newChannel.settings,
                                  [setting.key]: newChannel.settings[setting.key] === 'true' ? 'false' : 'true',
                                },
                              })
                            }
                            className={`relative inline-flex h-6 w-11 items-center rounded-full transition-colors ${
                              newChannel.settings[setting.key] === 'true' ? 'bg-stark-500' : 'bg-slate-600'
                            }`}
                          >
                            <span
                              className={`inline-block h-4 w-4 rounded-full bg-white transition-transform ${
                                newChannel.settings[setting.key] === 'true' ? 'translate-x-6' : 'translate-x-1'
                              }`}
                            />
                          </button>
                        </div>
                      ) : setting.input_type === 'select' && setting.options ? (
                        <>
                          <label className="block text-sm font-medium text-slate-300 mb-1">{setting.label}</label>
                          <select
                            value={newChannel.settings[setting.key] || setting.default_value || ''}
                            onChange={(e) =>
                              setNewChannel({
                                ...newChannel,
                                settings: {
                                  ...newChannel.settings,
                                  [setting.key]: e.target.value,
                                },
                              })
                            }
                            className="w-full px-3 py-2 bg-slate-800 border border-slate-700 rounded-lg text-white focus:ring-2 focus:ring-stark-500 focus:border-transparent"
                          >
                            {setting.options.map((opt) => (
                              <option key={opt.value} value={opt.value}>{opt.label}</option>
                            ))}
                          </select>
                          <p className="mt-1 text-xs text-slate-500">{setting.description}</p>
                        </>
                      ) : (
                        <>
                          <Input
                            label={setting.label}
                            value={newChannel.settings[setting.key] || ''}
                            onChange={(e) =>
                              setNewChannel({
                                ...newChannel,
                                settings: {
                                  ...newChannel.settings,
                                  [setting.key]: e.target.value,
                                },
                              })
                            }
                            placeholder={setting.placeholder}
                            type={setting.input_type === 'number' ? 'number' : 'text'}
                          />
                          <p className="mt-1 text-xs text-slate-500">{setting.description}</p>
                        </>
                      )}
                    </div>
                  ))}
                </>
              )}
              <div className="flex gap-2 justify-end">
                <Button variant="secondary" onClick={() => { setShowAddForm(false); setNewChannelSchema([]); }}>
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
                  {isEditing ? (
                    // Edit mode - unified form with channel data + settings
                    <div className="space-y-4">
                      {editLoading ? (
                        <div className="flex items-center justify-center py-4">
                          <div className="w-5 h-5 border-2 border-stark-500 border-t-transparent rounded-full animate-spin" />
                          <span className="ml-2 text-slate-400">Loading...</span>
                        </div>
                      ) : (
                        <>
                          <Input
                            label="Name"
                            value={editForm.name}
                            onChange={(e) => setEditForm({ ...editForm, name: e.target.value })}
                          />
                          {/* Settings section */}
                          {editSchema.length > 0 && (
                            <>
                              <div className="border-t border-slate-700 pt-4 mt-4">
                                <h4 className="text-sm font-medium text-slate-300 mb-3">Settings</h4>
                              </div>
                              {editSchema.map((setting) => (
                                <div key={setting.key}>
                                  {setting.input_type === 'toggle' ? (
                                    <div className="flex items-center justify-between">
                                      <div>
                                        <label className="block text-sm font-medium text-slate-300">
                                          {setting.label}
                                        </label>
                                        <p className="mt-1 text-xs text-slate-500">{setting.description}</p>
                                      </div>
                                      <button
                                        type="button"
                                        onClick={() =>
                                          setEditForm({
                                            ...editForm,
                                            settings: {
                                              ...editForm.settings,
                                              [setting.key]: editForm.settings[setting.key] === 'true' ? 'false' : 'true',
                                            },
                                          })
                                        }
                                        className={`relative inline-flex h-6 w-11 items-center rounded-full transition-colors ${
                                          editForm.settings[setting.key] === 'true' ? 'bg-stark-500' : 'bg-slate-600'
                                        }`}
                                      >
                                        <span
                                          className={`inline-block h-4 w-4 rounded-full bg-white transition-transform ${
                                            editForm.settings[setting.key] === 'true' ? 'translate-x-6' : 'translate-x-1'
                                          }`}
                                        />
                                      </button>
                                    </div>
                                  ) : setting.input_type === 'select' && setting.options ? (
                                    <>
                                      <label className="block text-sm font-medium text-slate-300 mb-1">{setting.label}</label>
                                      <select
                                        value={editForm.settings[setting.key] || setting.default_value || ''}
                                        onChange={(e) =>
                                          setEditForm({
                                            ...editForm,
                                            settings: {
                                              ...editForm.settings,
                                              [setting.key]: e.target.value,
                                            },
                                          })
                                        }
                                        className="w-full px-3 py-2 bg-slate-800 border border-slate-700 rounded-lg text-white focus:ring-2 focus:ring-stark-500 focus:border-transparent"
                                      >
                                        {setting.options.map((opt) => (
                                          <option key={opt.value} value={opt.value}>{opt.label}</option>
                                        ))}
                                      </select>
                                      <p className="mt-1 text-xs text-slate-500">{setting.description}</p>
                                    </>
                                  ) : (
                                    <>
                                      <Input
                                        label={setting.label}
                                        value={editForm.settings[setting.key] || ''}
                                        onChange={(e) =>
                                          setEditForm({
                                            ...editForm,
                                            settings: {
                                              ...editForm.settings,
                                              [setting.key]: e.target.value,
                                            },
                                          })
                                        }
                                        placeholder={setting.placeholder}
                                        type={setting.input_type === 'number' ? 'number' : 'text'}
                                      />
                                      <p className="mt-1 text-xs text-slate-500">{setting.description}</p>
                                    </>
                                  )}
                                </div>
                              ))}
                            </>
                          )}
                          <div className="flex gap-2 justify-end pt-2">
                            <Button
                              variant="secondary"
                              onClick={() => {
                                setEditingId(null);
                                setEditSchema([]);
                              }}
                            >
                              Cancel
                            </Button>
                            <Button onClick={() => handleUpdate(channel.id)} disabled={isActionLoading}>
                              <Save className="w-4 h-4 mr-1" />
                              Save
                            </Button>
                          </div>
                        </>
                      )}
                    </div>
                  ) : (
                    // View mode - display channel info
                    <div className="space-y-3">
                      {channel.channel_type === 'twitter' && (
                        <TwitterKeyWarning apiKeys={apiKeys} />
                      )}
                      {getChannelHints(channel.channel_type).map((hint, idx) => (
                        <div key={idx} className="px-3 py-2 bg-slate-700/50 border border-slate-600/50 rounded-lg">
                          <p className="text-xs text-slate-300 [&_a]:text-blue-400 [&_a]:underline [&_a]:hover:text-blue-300" dangerouslySetInnerHTML={{ __html: hint }} />
                        </div>
                      ))}
                      <div className="flex justify-end pt-2">
                        <Button
                          variant="secondary"
                          size="sm"
                          onClick={() => toggleEditMode(channel)}
                        >
                          <Pencil className="w-4 h-4 mr-1" />
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
