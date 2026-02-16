import { useState, useEffect } from 'react';
import { Bot, Plus, Trash2, RotateCcw, Save, X } from 'lucide-react';
import Card, { CardContent } from '@/components/ui/Card';
import Button from '@/components/ui/Button';
import {
  getAgentSubtypes,
  createAgentSubtype,
  updateAgentSubtype,
  deleteAgentSubtype,
  resetAgentSubtypeDefaults,
  getToolGroups,
  AgentSubtypeInfo,
  ToolGroupInfo,
} from '@/lib/api';

const MAX_SUBTYPES = 10;

const EMPTY_SUBTYPE: AgentSubtypeInfo = {
  key: '',
  label: '',
  emoji: '',
  description: '',
  tool_groups: [],
  skill_tags: [],
  prompt: '',
  sort_order: 0,
  enabled: true,
};

export default function AgentSubtypes() {
  const [subtypes, setSubtypes] = useState<AgentSubtypeInfo[]>([]);
  const [toolGroups, setToolGroups] = useState<ToolGroupInfo[]>([]);
  const [isLoading, setIsLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [success, setSuccess] = useState<string | null>(null);

  const [selectedKey, setSelectedKey] = useState<string | null>(null);
  const [editForm, setEditForm] = useState<AgentSubtypeInfo | null>(null);
  const [isCreating, setIsCreating] = useState(false);
  const [isSaving, setIsSaving] = useState(false);
  const [isResetting, setIsResetting] = useState(false);

  useEffect(() => {
    loadData();
  }, []);

  useEffect(() => {
    if (success) {
      const t = setTimeout(() => setSuccess(null), 3000);
      return () => clearTimeout(t);
    }
  }, [success]);

  // When subtypes load or change, select the first one if nothing is selected
  useEffect(() => {
    if (!isCreating && subtypes.length > 0 && (!selectedKey || !subtypes.find(s => s.key === selectedKey))) {
      const firstKey = subtypes[0].key;
      setSelectedKey(firstKey);
      setEditForm({ ...subtypes[0] });
    }
  }, [subtypes]);

  const loadData = async () => {
    try {
      const [subtypesData, groupsData] = await Promise.all([
        getAgentSubtypes(),
        getToolGroups(),
      ]);
      setSubtypes(subtypesData);
      setToolGroups(groupsData);
    } catch (err) {
      setError('Failed to load agent subtypes');
    } finally {
      setIsLoading(false);
    }
  };

  const handleSelectTab = (key: string) => {
    if (isCreating) setIsCreating(false);
    const subtype = subtypes.find(s => s.key === key);
    if (subtype) {
      setSelectedKey(key);
      setEditForm({ ...subtype });
    }
  };

  const handleStartCreate = () => {
    const nextOrder = subtypes.length > 0
      ? Math.max(...subtypes.map(s => s.sort_order)) + 1
      : 0;
    setEditForm({ ...EMPTY_SUBTYPE, sort_order: nextOrder });
    setIsCreating(true);
    setSelectedKey(null);
  };

  const handleCancelCreate = () => {
    setIsCreating(false);
    if (subtypes.length > 0) {
      setSelectedKey(subtypes[0].key);
      setEditForm({ ...subtypes[0] });
    } else {
      setSelectedKey(null);
      setEditForm(null);
    }
  };

  const handleSave = async () => {
    if (!editForm) return;
    setIsSaving(true);
    setError(null);

    try {
      if (isCreating) {
        const created = await createAgentSubtype(editForm);
        setSubtypes(prev => [...prev, created]);
        setSuccess(`Created "${created.label}"`);
        setIsCreating(false);
        setSelectedKey(created.key);
        setEditForm({ ...created });
      } else {
        const updated = await updateAgentSubtype(editForm.key, editForm);
        setSubtypes(prev => prev.map(s => s.key === updated.key ? updated : s));
        setSuccess(`Updated "${updated.label}"`);
        setEditForm({ ...updated });
      }
    } catch (err: unknown) {
      const msg = err instanceof Error ? err.message : 'Failed to save';
      setError(msg);
    } finally {
      setIsSaving(false);
    }
  };

  const handleDelete = async (key: string) => {
    const subtype = subtypes.find(s => s.key === key);
    if (!confirm(`Delete agent subtype "${subtype?.label || key}"?`)) return;

    try {
      await deleteAgentSubtype(key);
      const remaining = subtypes.filter(s => s.key !== key);
      setSubtypes(remaining);
      if (selectedKey === key) {
        if (remaining.length > 0) {
          setSelectedKey(remaining[0].key);
          setEditForm({ ...remaining[0] });
        } else {
          setSelectedKey(null);
          setEditForm(null);
        }
      }
      setSuccess('Deleted successfully');
    } catch (err) {
      setError('Failed to delete');
    }
  };

  const handleToggleEnabled = async () => {
    if (!editForm) return;
    try {
      const updated = await updateAgentSubtype(editForm.key, { enabled: !editForm.enabled });
      setSubtypes(prev => prev.map(s => s.key === updated.key ? updated : s));
      setEditForm({ ...updated });
    } catch (err) {
      setError('Failed to toggle enabled state');
    }
  };

  const handleResetDefaults = async () => {
    if (!confirm('Reset all agent subtypes to defaults? This will delete any custom subtypes.')) return;
    setIsResetting(true);
    setError(null);

    try {
      const result = await resetAgentSubtypeDefaults();
      setSuccess(result.message);
      setSelectedKey(null);
      setEditForm(null);
      setIsCreating(false);
      await loadData();
    } catch (err) {
      setError('Failed to reset defaults');
    } finally {
      setIsResetting(false);
    }
  };

  const handleToolGroupToggle = (group: string) => {
    if (!editForm) return;
    const groups = editForm.tool_groups.includes(group)
      ? editForm.tool_groups.filter(g => g !== group)
      : [...editForm.tool_groups, group];
    setEditForm({ ...editForm, tool_groups: groups });
  };

  if (isLoading) {
    return (
      <div className="p-4 sm:p-8 flex items-center justify-center">
        <div className="flex items-center gap-3">
          <div className="w-6 h-6 border-2 border-stark-500 border-t-transparent rounded-full animate-spin" />
          <span className="text-slate-400">Loading agent subtypes...</span>
        </div>
      </div>
    );
  }

  return (
    <div className="p-4 sm:p-8">
      {/* Header */}
      <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-4 mb-6 sm:mb-8">
        <div>
          <h1 className="text-xl sm:text-2xl font-bold text-white mb-1 sm:mb-2">Agent Subtypes</h1>
          <p className="text-sm sm:text-base text-slate-400">
            Configure agent modes ({subtypes.length}/{MAX_SUBTYPES})
          </p>
        </div>
        <div className="flex gap-2">
          <Button
            variant="secondary"
            onClick={handleResetDefaults}
            isLoading={isResetting}
            className="w-auto"
          >
            <RotateCcw className="w-4 h-4 mr-2" />
            Reset Defaults
          </Button>
          <Button
            onClick={handleStartCreate}
            disabled={subtypes.length >= MAX_SUBTYPES || isCreating}
            className="w-auto"
          >
            <Plus className="w-4 h-4 mr-2" />
            Add Subtype
          </Button>
        </div>
      </div>

      {/* Messages */}
      {error && (
        <div className="mb-6 bg-red-500/20 border border-red-500/50 text-red-400 px-4 py-3 rounded-lg">
          {error}
          <button onClick={() => setError(null)} className="ml-2 text-red-300 hover:text-red-200">
            <X className="w-4 h-4 inline" />
          </button>
        </div>
      )}
      {success && (
        <div className="mb-6 bg-green-500/20 border border-green-500/50 text-green-400 px-4 py-3 rounded-lg">
          {success}
        </div>
      )}

      {subtypes.length > 0 || isCreating ? (
        <>
          {/* Tabs */}
          <div className="flex items-center gap-1 border-b border-slate-700/50 mb-0 overflow-x-auto pb-px">
            {subtypes.map(subtype => {
              const isActive = !isCreating && selectedKey === subtype.key;
              return (
                <button
                  key={subtype.key}
                  onClick={() => handleSelectTab(subtype.key)}
                  className={`flex items-center gap-2 px-4 py-2.5 text-sm font-medium whitespace-nowrap transition-colors border-b-2 -mb-px ${
                    isActive
                      ? 'border-stark-500 text-white'
                      : 'border-transparent text-slate-400 hover:text-slate-200 hover:border-slate-600'
                  }`}
                >
                  <span>{subtype.emoji}</span>
                  <span>{subtype.label}</span>
                  {!subtype.enabled && (
                    <span className="text-[10px] px-1.5 py-0.5 bg-slate-700 text-slate-500 rounded">off</span>
                  )}
                </button>
              );
            })}
            {isCreating && (
              <button
                className="flex items-center gap-2 px-4 py-2.5 text-sm font-medium whitespace-nowrap border-b-2 -mb-px border-stark-500 text-white"
              >
                <Plus className="w-3.5 h-3.5" />
                <span>New Subtype</span>
              </button>
            )}
          </div>

          {/* Content area */}
          {editForm && (
            <Card className="rounded-t-none border-t-0">
              <CardContent>
                {/* Action bar */}
                <div className="flex items-center justify-between mb-6">
                  <div className="flex items-center gap-3">
                    {!isCreating && (
                      <>
                        <span className="text-2xl">{editForm.emoji}</span>
                        <div>
                          <h2 className="text-lg font-semibold text-white">{editForm.label}</h2>
                          <span className="text-xs font-mono text-slate-500">{editForm.key}</span>
                        </div>
                      </>
                    )}
                    {isCreating && (
                      <h2 className="text-lg font-semibold text-white">New Agent Subtype</h2>
                    )}
                  </div>
                  <div className="flex items-center gap-2">
                    {!isCreating && (
                      <>
                        <button
                          onClick={handleToggleEnabled}
                          className={`px-2.5 py-1 text-xs rounded cursor-pointer transition-colors ${
                            editForm.enabled
                              ? 'bg-green-500/20 text-green-400 hover:bg-green-500/30'
                              : 'bg-slate-700 text-slate-400 hover:bg-slate-600'
                          }`}
                        >
                          {editForm.enabled ? 'Enabled' : 'Disabled'}
                        </button>
                        <Button
                          variant="ghost"
                          size="sm"
                          onClick={() => handleDelete(editForm.key)}
                          className="text-red-400 hover:text-red-300 hover:bg-red-500/20"
                        >
                          <Trash2 className="w-4 h-4 mr-1" />
                          Delete
                        </Button>
                      </>
                    )}
                    {isCreating && (
                      <Button variant="ghost" size="sm" onClick={handleCancelCreate}>
                        <X className="w-4 h-4 mr-1" /> Cancel
                      </Button>
                    )}
                    <Button size="sm" onClick={handleSave} isLoading={isSaving}>
                      <Save className="w-4 h-4 mr-1" />
                      {isCreating ? 'Create' : 'Save'}
                    </Button>
                  </div>
                </div>

                {/* Form */}
                <SubtypeForm
                  form={editForm}
                  setForm={setEditForm}
                  toolGroups={toolGroups}
                  onToolGroupToggle={handleToolGroupToggle}
                  isNew={isCreating}
                />
              </CardContent>
            </Card>
          )}
        </>
      ) : (
        <Card>
          <CardContent className="text-center py-12">
            <Bot className="w-12 h-12 text-slate-600 mx-auto mb-4" />
            <p className="text-slate-400 mb-4">No agent subtypes configured</p>
            <Button variant="secondary" onClick={handleResetDefaults} isLoading={isResetting}>
              <RotateCcw className="w-4 h-4 mr-2" />
              Load Defaults
            </Button>
          </CardContent>
        </Card>
      )}
    </div>
  );
}

// --- Subtype Form Component ---

interface SubtypeFormProps {
  form: AgentSubtypeInfo;
  setForm: (form: AgentSubtypeInfo) => void;
  toolGroups: ToolGroupInfo[];
  onToolGroupToggle: (group: string) => void;
  isNew?: boolean;
}

function SubtypeForm({ form, setForm, toolGroups, onToolGroupToggle, isNew }: SubtypeFormProps) {
  return (
    <div className="space-y-4">
      {/* Row: Key + Label + Emoji */}
      <div className="grid grid-cols-1 sm:grid-cols-3 gap-4">
        {isNew && (
          <div>
            <label className="block text-xs text-slate-500 mb-1">Key (unique ID)</label>
            <input
              type="text"
              value={form.key}
              onChange={e => setForm({ ...form, key: e.target.value.toLowerCase().replace(/[^a-z0-9_]/g, '') })}
              placeholder="my_subtype"
              className="w-full bg-slate-900/50 border border-slate-700 rounded-lg px-3 py-2 text-sm text-white placeholder-slate-600 focus:outline-none focus:border-stark-500"
            />
          </div>
        )}
        <div>
          <label className="block text-xs text-slate-500 mb-1">Label</label>
          <input
            type="text"
            value={form.label}
            onChange={e => setForm({ ...form, label: e.target.value })}
            placeholder="My Subtype"
            className="w-full bg-slate-900/50 border border-slate-700 rounded-lg px-3 py-2 text-sm text-white placeholder-slate-600 focus:outline-none focus:border-stark-500"
          />
        </div>
        <div>
          <label className="block text-xs text-slate-500 mb-1">Emoji</label>
          <input
            type="text"
            value={form.emoji}
            onChange={e => setForm({ ...form, emoji: e.target.value })}
            placeholder=""
            className="w-full bg-slate-900/50 border border-slate-700 rounded-lg px-3 py-2 text-sm text-white placeholder-slate-600 focus:outline-none focus:border-stark-500"
          />
        </div>
      </div>

      {/* Description */}
      <div>
        <label className="block text-xs text-slate-500 mb-1">Description</label>
        <input
          type="text"
          value={form.description}
          onChange={e => setForm({ ...form, description: e.target.value })}
          placeholder="Short description of this agent mode"
          className="w-full bg-slate-900/50 border border-slate-700 rounded-lg px-3 py-2 text-sm text-white placeholder-slate-600 focus:outline-none focus:border-stark-500"
        />
      </div>

      {/* Tool Groups */}
      <div>
        <label className="block text-xs text-slate-500 mb-2">Tool Groups</label>
        <div className="flex flex-wrap gap-2">
          {toolGroups.map(g => {
            const isActive = form.tool_groups.includes(g.key);
            return (
              <button
                key={g.key}
                type="button"
                onClick={() => onToolGroupToggle(g.key)}
                className={`px-3 py-1.5 text-sm rounded-full transition-colors ${
                  isActive
                    ? 'bg-stark-500 text-white'
                    : 'bg-slate-800 text-slate-400 hover:bg-slate-700 hover:text-slate-300'
                }`}
                title={g.description}
              >
                {g.label}
              </button>
            );
          })}
        </div>
      </div>

      {/* Skill Tags */}
      <div>
        <label className="block text-xs text-slate-500 mb-1">Skill Tags (comma-separated)</label>
        <input
          type="text"
          value={form.skill_tags.join(', ')}
          onChange={e => setForm({
            ...form,
            skill_tags: e.target.value.split(',').map(t => t.trim()).filter(Boolean),
          })}
          placeholder="crypto, defi, swap"
          className="w-full bg-slate-900/50 border border-slate-700 rounded-lg px-3 py-2 text-sm text-white placeholder-slate-600 focus:outline-none focus:border-stark-500"
        />
      </div>

      {/* Sort Order */}
      <div className="w-32">
        <label className="block text-xs text-slate-500 mb-1">Sort Order</label>
        <input
          type="number"
          value={form.sort_order}
          onChange={e => setForm({ ...form, sort_order: parseInt(e.target.value) || 0 })}
          className="w-full bg-slate-900/50 border border-slate-700 rounded-lg px-3 py-2 text-sm text-white focus:outline-none focus:border-stark-500"
        />
      </div>

      {/* Prompt */}
      <div>
        <label className="block text-xs text-slate-500 mb-1">Toolbox Activation Prompt</label>
        <textarea
          value={form.prompt}
          onChange={e => setForm({ ...form, prompt: e.target.value })}
          className="w-full h-48 bg-slate-900/50 border border-slate-700 rounded-lg p-3 text-sm text-slate-300 font-mono resize-none focus:outline-none focus:border-stark-500"
          spellCheck={false}
          placeholder="The prompt shown to the agent when this subtype is activated..."
        />
      </div>
    </div>
  );
}
