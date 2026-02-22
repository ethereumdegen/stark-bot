import { useState, useEffect, useRef } from 'react';
import { Bot, Plus, Trash2, RotateCcw, Save, X, Download, Upload } from 'lucide-react';
import Card, { CardContent } from '@/components/ui/Card';
import Button from '@/components/ui/Button';
import {
  getAgentSubtypes,
  createAgentSubtype,
  updateAgentSubtype,
  deleteAgentSubtype,
  resetAgentSubtypeDefaults,
  exportAgentSubtype,
  importAgentSubtypes,
  getToolGroups,
  readIntrinsicFile,
  writeIntrinsicFile,
  getAiEndpointPresets,
  AgentSubtypeInfo,
  ToolGroupInfo,
  AiEndpointPreset,
} from '@/lib/api';

const MAX_SUBTYPES = 10;

const EMPTY_SUBTYPE: AgentSubtypeInfo = {
  key: '',
  label: '',
  emoji: '',
  description: '',
  tool_groups: [],
  skill_tags: [],
  additional_tools: [],
  prompt: '',
  sort_order: 0,
  enabled: true,
  max_iterations: 90,
  skip_task_planner: false,
};

export default function AgentSubtypes() {
  const [subtypes, setSubtypes] = useState<AgentSubtypeInfo[]>([]);
  const [toolGroups, setToolGroups] = useState<ToolGroupInfo[]>([]);
  const [endpointPresets, setEndpointPresets] = useState<AiEndpointPreset[]>([]);
  const [isLoading, setIsLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [success, setSuccess] = useState<string | null>(null);

  const [selectedKey, setSelectedKey] = useState<string | null>(null);
  const [editForm, setEditForm] = useState<AgentSubtypeInfo | null>(null);
  const [isCreating, setIsCreating] = useState(false);
  const [isSaving, setIsSaving] = useState(false);
  const [isResetting, setIsResetting] = useState(false);

  const [goalsContent, setGoalsContent] = useState<string | null>(null);
  const [goalsLoading, setGoalsLoading] = useState(false);
  const [goalsSaving, setGoalsSaving] = useState(false);

  const [heartbeatContent, setHeartbeatContent] = useState<string | null>(null);
  const [heartbeatLoading, setHeartbeatLoading] = useState(false);
  const [heartbeatSaving, setHeartbeatSaving] = useState(false);

  const fileInputRef = useRef<HTMLInputElement>(null);

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
      loadGoals(firstKey);
      loadHeartbeat(firstKey);
    }
  }, [subtypes]);

  const loadData = async () => {
    try {
      const [subtypesData, groupsData, presetsData] = await Promise.all([
        getAgentSubtypes(),
        getToolGroups(),
        getAiEndpointPresets().catch(() => [] as AiEndpointPreset[]),
      ]);
      setSubtypes(subtypesData);
      setToolGroups(groupsData);
      setEndpointPresets(presetsData);
    } catch (err) {
      setError('Failed to load agent subtypes');
    } finally {
      setIsLoading(false);
    }
  };

  const loadGoals = async (key: string) => {
    setGoalsLoading(true);
    try {
      const result = await readIntrinsicFile(`agents/${key}/goals.md`);
      setGoalsContent(result.content ?? null);
    } catch {
      setGoalsContent(null);
    } finally {
      setGoalsLoading(false);
    }
  };

  const loadHeartbeat = async (key: string) => {
    setHeartbeatLoading(true);
    try {
      const result = await readIntrinsicFile(`agents/${key}/heartbeat.md`);
      setHeartbeatContent(result.content ?? null);
    } catch {
      setHeartbeatContent(null);
    } finally {
      setHeartbeatLoading(false);
    }
  };

  const handleSelectTab = (key: string) => {
    if (isCreating) setIsCreating(false);
    const subtype = subtypes.find(s => s.key === key);
    if (subtype) {
      setSelectedKey(key);
      setEditForm({ ...subtype });
      loadGoals(key);
      loadHeartbeat(key);
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

  const handleExport = async () => {
    if (!editForm || isCreating) return;
    try {
      const ron = await exportAgentSubtype(editForm.key);
      const blob = new Blob([ron], { type: 'application/ron' });
      const url = URL.createObjectURL(blob);
      const a = document.createElement('a');
      a.href = url;
      a.download = `${editForm.key}.ron`;
      a.click();
      URL.revokeObjectURL(url);
      setSuccess(`Exported "${editForm.label}"`);
    } catch (err) {
      setError('Failed to export');
    }
  };

  const handleImportClick = () => {
    fileInputRef.current?.click();
  };

  const handleImportFile = async (e: React.ChangeEvent<HTMLInputElement>) => {
    const file = e.target.files?.[0];
    if (!file) return;

    try {
      const ron = await file.text();
      const replace = confirm(
        'Replace all existing subtypes with imported ones?\n\nOK = Replace all\nCancel = Merge (add/update only)'
      );
      const result = await importAgentSubtypes(ron, replace);
      setSuccess(result.message);
      setSelectedKey(null);
      setEditForm(null);
      setIsCreating(false);
      await loadData();
    } catch (err: unknown) {
      const msg = err instanceof Error ? err.message : 'Failed to import';
      setError(msg);
    } finally {
      // Reset input so same file can be re-selected
      if (fileInputRef.current) fileInputRef.current.value = '';
    }
  };

  const handleActivateHeartbeat = async () => {
    if (!selectedKey) return;
    const defaultContent = '# Heartbeat\n\nDescribe what this agent should do on each heartbeat cycle.\nRespond with HEARTBEAT_OK if no action needed.';
    try {
      await writeIntrinsicFile(`agents/${selectedKey}/heartbeat.md`, defaultContent);
      setHeartbeatContent(defaultContent);
      setSuccess('Heartbeat activated');
    } catch {
      setError('Failed to create heartbeat file');
    }
  };

  const handleSaveHeartbeat = async () => {
    if (!selectedKey || heartbeatContent === null) return;
    setHeartbeatSaving(true);
    try {
      await writeIntrinsicFile(`agents/${selectedKey}/heartbeat.md`, heartbeatContent);
      setSuccess('Heartbeat saved');
    } catch {
      setError('Failed to save heartbeat');
    } finally {
      setHeartbeatSaving(false);
    }
  };

  const handleActivateGoals = async () => {
    if (!selectedKey) return;
    const defaultContent = '# Goals\n\nDefine the overall strategic goals for this agent.\nThese provide awareness context prepended to each heartbeat prompt.';
    try {
      await writeIntrinsicFile(`agents/${selectedKey}/goals.md`, defaultContent);
      setGoalsContent(defaultContent);
      setSuccess('Goals activated');
    } catch {
      setError('Failed to create goals file');
    }
  };

  const handleSaveGoals = async () => {
    if (!selectedKey || goalsContent === null) return;
    setGoalsSaving(true);
    try {
      await writeIntrinsicFile(`agents/${selectedKey}/goals.md`, goalsContent);
      setSuccess('Goals saved');
    } catch {
      setError('Failed to save goals');
    } finally {
      setGoalsSaving(false);
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
      {/* Hidden file input for import */}
      <input
        ref={fileInputRef}
        type="file"
        accept=".ron,.txt"
        className="hidden"
        onChange={handleImportFile}
      />

      {/* Header */}
      <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-4 mb-6 sm:mb-8">
        <div>
          <h1 className="text-xl sm:text-2xl font-bold text-white mb-1 sm:mb-2">Agent Subtypes</h1>
          <p className="text-sm sm:text-base text-slate-400">
            Configure agent modes ({subtypes.length}/{MAX_SUBTYPES})
          </p>
        </div>
        <div className="flex flex-wrap gap-2">
          <Button variant="secondary" onClick={handleImportClick} className="w-auto">
            <Upload className="w-4 h-4 mr-2" />
            Import
          </Button>
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
                          onClick={handleExport}
                        >
                          <Download className="w-4 h-4 mr-1" />
                          Export
                        </Button>
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
                  endpointPresets={endpointPresets}
                />

                {/* Goals Section */}
                {!isCreating && selectedKey && (
                  <div className="mt-6 pt-6 border-t border-slate-700/50">
                    <div className="flex items-center justify-between mb-2">
                      <label className="block text-xs text-slate-500">
                        Goals
                        <span className="text-slate-600 ml-1">— strategic context prepended to each heartbeat prompt</span>
                      </label>
                      {goalsContent !== null && (
                        <Button size="sm" variant="ghost" onClick={handleSaveGoals} isLoading={goalsSaving}>
                          <Save className="w-3.5 h-3.5 mr-1" /> Save Goals
                        </Button>
                      )}
                    </div>
                    {goalsLoading ? (
                      <div className="text-xs text-slate-500">Loading...</div>
                    ) : goalsContent === null ? (
                      <Button variant="secondary" size="sm" onClick={handleActivateGoals}>
                        Activate Goals
                      </Button>
                    ) : (
                      <textarea
                        value={goalsContent}
                        onChange={e => setGoalsContent(e.target.value)}
                        className="w-full h-36 bg-slate-900/50 border border-slate-700 rounded-lg p-3 text-sm text-slate-300 font-mono resize-none focus:outline-none focus:border-stark-500"
                        spellCheck={false}
                        placeholder="Overall strategic goals for this agent..."
                      />
                    )}
                  </div>
                )}

                {/* Heartbeat Section */}
                {!isCreating && selectedKey && (
                  <div className="mt-6 pt-6 border-t border-slate-700/50">
                    <div className="flex items-center justify-between mb-2">
                      <label className="block text-xs text-slate-500">
                        Heartbeat Prompt
                        <span className="text-slate-600 ml-1">— runs on each heartbeat cycle if present</span>
                      </label>
                      {heartbeatContent !== null && (
                        <Button size="sm" variant="ghost" onClick={handleSaveHeartbeat} isLoading={heartbeatSaving}>
                          <Save className="w-3.5 h-3.5 mr-1" /> Save Heartbeat
                        </Button>
                      )}
                    </div>
                    {heartbeatLoading ? (
                      <div className="text-xs text-slate-500">Loading...</div>
                    ) : heartbeatContent === null ? (
                      <Button variant="secondary" size="sm" onClick={handleActivateHeartbeat}>
                        Activate Heartbeat
                      </Button>
                    ) : (
                      <textarea
                        value={heartbeatContent}
                        onChange={e => setHeartbeatContent(e.target.value)}
                        className="w-full h-36 bg-slate-900/50 border border-slate-700 rounded-lg p-3 text-sm text-slate-300 font-mono resize-none focus:outline-none focus:border-stark-500"
                        spellCheck={false}
                        placeholder="Heartbeat prompt for this agent..."
                      />
                    )}
                  </div>
                )}
              </CardContent>
            </Card>
          )}
        </>
      ) : (
        <Card>
          <CardContent className="text-center py-12">
            <Bot className="w-12 h-12 text-slate-600 mx-auto mb-4" />
            <p className="text-slate-400 mb-4">No agent subtypes configured</p>
            <div className="flex justify-center gap-3">
              <Button variant="secondary" onClick={handleResetDefaults} isLoading={isResetting}>
                <RotateCcw className="w-4 h-4 mr-2" />
                Load Defaults
              </Button>
              <Button variant="secondary" onClick={handleImportClick}>
                <Upload className="w-4 h-4 mr-2" />
                Import
              </Button>
            </div>
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
  endpointPresets?: AiEndpointPreset[];
}

function SubtypeForm({ form, setForm, toolGroups, onToolGroupToggle, isNew, endpointPresets = [] }: SubtypeFormProps) {
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

      {/* Row: Sort Order + Max Iterations + Skip Task Planner */}
      <div className="grid grid-cols-2 sm:grid-cols-4 gap-4 items-end">
        <div>
          <label className="block text-xs text-slate-500 mb-1">Sort Order</label>
          <input
            type="number"
            value={form.sort_order}
            onChange={e => setForm({ ...form, sort_order: parseInt(e.target.value) || 0 })}
            className="w-full bg-slate-900/50 border border-slate-700 rounded-lg px-3 py-2 text-sm text-white focus:outline-none focus:border-stark-500"
          />
        </div>
        <div>
          <label className="block text-xs text-slate-500 mb-1">Max Iterations</label>
          <input
            type="number"
            value={form.max_iterations}
            onChange={e => setForm({ ...form, max_iterations: parseInt(e.target.value) || 90 })}
            min={1}
            max={500}
            className="w-full bg-slate-900/50 border border-slate-700 rounded-lg px-3 py-2 text-sm text-white focus:outline-none focus:border-stark-500"
          />
        </div>
        <div>
          <label className="block text-xs text-slate-500 mb-1">Skip Task Planner</label>
          <button
            type="button"
            onClick={() => setForm({ ...form, skip_task_planner: !form.skip_task_planner })}
            className={`px-3 py-2 text-sm rounded-lg transition-colors w-full ${
              form.skip_task_planner
                ? 'bg-stark-500/20 text-stark-400 border border-stark-500/50'
                : 'bg-slate-900/50 text-slate-400 border border-slate-700 hover:border-slate-600'
            }`}
          >
            {form.skip_task_planner ? 'Yes (skip planning)' : 'No (plan first)'}
          </button>
        </div>
        <div>
          <label className="block text-xs text-slate-500 mb-1">Preferred AI Model</label>
          <select
            value={form.preferred_ai_model || ''}
            onChange={e => setForm({ ...form, preferred_ai_model: e.target.value })}
            className="w-full bg-slate-900/50 border border-slate-700 rounded-lg px-3 py-2 text-sm text-white focus:outline-none focus:border-stark-500"
          >
            <option value="">Global default</option>
            {endpointPresets.map(p => (
              <option key={p.id} value={p.id}>{p.display_name}</option>
            ))}
          </select>
        </div>
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

      {/* Additional Tools */}
      <div>
        <label className="block text-xs text-slate-500 mb-1">
          Additional Tools (comma-separated)
          <span className="text-slate-600 ml-1">— extra tools added on top of the selected groups</span>
        </label>
        <input
          type="text"
          value={(form.additional_tools || []).join(', ')}
          onChange={e => setForm({
            ...form,
            additional_tools: e.target.value.split(',').map(t => t.trim()).filter(Boolean),
          })}
          placeholder="spawn_subagent, say_to_user, ask_user"
          className="w-full bg-slate-900/50 border border-slate-700 rounded-lg px-3 py-2 text-sm text-white placeholder-slate-600 focus:outline-none focus:border-stark-500"
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
