import { useState, useEffect } from 'react';
import { ShieldCheck, Plus, Trash2, Save, X } from 'lucide-react';
import Card, { CardContent } from '@/components/ui/Card';
import Button from '@/components/ui/Button';
import {
  getSpecialRoles,
  createSpecialRole,
  updateSpecialRole,
  deleteSpecialRole,
  getSpecialRoleAssignments,
  createSpecialRoleAssignment,
  deleteSpecialRoleAssignment,
  SpecialRoleInfo,
  SpecialRoleAssignmentInfo,
} from '@/lib/api';

type Tab = 'roles' | 'assignments';

const CHANNEL_TYPES = ['discord', 'twitter', 'telegram', 'slack', 'external_channel'];
const MAX_ROLES = 10;
const MAX_ASSIGNMENTS = 100;

export default function SpecialRoles() {
  const [activeTab, setActiveTab] = useState<Tab>('roles');
  const [roles, setRoles] = useState<SpecialRoleInfo[]>([]);
  const [assignments, setAssignments] = useState<SpecialRoleAssignmentInfo[]>([]);
  const [isLoading, setIsLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [success, setSuccess] = useState<string | null>(null);

  // Roles state
  const [selectedRole, setSelectedRole] = useState<string | null>(null);
  const [editName, setEditName] = useState('');
  const [editDescription, setEditDescription] = useState('');
  const [editTools, setEditTools] = useState('');
  const [editSkills, setEditSkills] = useState('');
  const [isCreating, setIsCreating] = useState(false);
  const [isSaving, setIsSaving] = useState(false);

  // Assignments state
  const [newAssignChannelType, setNewAssignChannelType] = useState('discord');
  const [newAssignUserId, setNewAssignUserId] = useState('');
  const [newAssignRoleName, setNewAssignRoleName] = useState('');
  const [newAssignLabel, setNewAssignLabel] = useState('');

  useEffect(() => {
    loadData();
  }, []);

  useEffect(() => {
    if (success) {
      const t = setTimeout(() => setSuccess(null), 3000);
      return () => clearTimeout(t);
    }
  }, [success]);

  const loadData = async () => {
    try {
      const [rolesData, assignmentsData] = await Promise.all([
        getSpecialRoles(),
        getSpecialRoleAssignments(),
      ]);
      setRoles(rolesData);
      setAssignments(assignmentsData);
      if (rolesData.length > 0 && !selectedRole) {
        selectRole(rolesData[0]);
      }
    } catch (err) {
      setError('Failed to load special roles');
    } finally {
      setIsLoading(false);
    }
  };

  const selectRole = (role: SpecialRoleInfo) => {
    setSelectedRole(role.name);
    setEditName(role.name);
    setEditDescription(role.description || '');
    setEditTools(role.allowed_tools.join(', '));
    setEditSkills(role.allowed_skills.join(', '));
    setIsCreating(false);
  };

  const handleStartCreate = () => {
    setIsCreating(true);
    setSelectedRole(null);
    setEditName('');
    setEditDescription('');
    setEditTools('');
    setEditSkills('');
  };

  const handleCancelCreate = () => {
    setIsCreating(false);
    if (roles.length > 0) {
      selectRole(roles[0]);
    }
  };

  const parseCommaSeparated = (s: string): string[] =>
    s.split(',').map(t => t.trim()).filter(Boolean);

  const handleSave = async () => {
    setIsSaving(true);
    setError(null);

    try {
      const tools = parseCommaSeparated(editTools);
      const skills = parseCommaSeparated(editSkills);

      if (isCreating) {
        const created = await createSpecialRole({
          name: editName,
          allowed_tools: tools,
          allowed_skills: skills,
          description: editDescription || undefined,
        });
        setRoles(prev => [...prev, created]);
        selectRole(created);
        setSuccess(`Created role "${created.name}"`);
      } else if (selectedRole) {
        const updated = await updateSpecialRole(selectedRole, {
          allowed_tools: tools,
          allowed_skills: skills,
          description: editDescription || null,
        });
        setRoles(prev => prev.map(r => r.name === updated.name ? updated : r));
        selectRole(updated);
        setSuccess(`Updated role "${updated.name}"`);
      }
    } catch (err: unknown) {
      const msg = err instanceof Error ? err.message : 'Failed to save';
      setError(msg);
    } finally {
      setIsSaving(false);
    }
  };

  const handleDeleteRole = async (name: string) => {
    if (!confirm(`Delete special role "${name}"? This will also remove all its assignments.`)) return;
    try {
      await deleteSpecialRole(name);
      const remaining = roles.filter(r => r.name !== name);
      setRoles(remaining);
      setAssignments(prev => prev.filter(a => a.special_role_name !== name));
      if (selectedRole === name) {
        if (remaining.length > 0) {
          selectRole(remaining[0]);
        } else {
          setSelectedRole(null);
          setEditName('');
          setEditDescription('');
          setEditTools('');
          setEditSkills('');
        }
      }
      setSuccess('Role deleted');
    } catch (err) {
      setError('Failed to delete role');
    }
  };

  const handleCreateAssignment = async () => {
    if (!newAssignUserId.trim() || !newAssignRoleName) {
      setError('User ID and role name are required');
      return;
    }
    setError(null);
    try {
      const created = await createSpecialRoleAssignment({
        channel_type: newAssignChannelType,
        user_id: newAssignUserId.trim(),
        special_role_name: newAssignRoleName,
        label: newAssignLabel.trim() || undefined,
      });
      setAssignments(prev => [...prev, created]);
      setNewAssignUserId('');
      setNewAssignLabel('');
      setSuccess('Assignment created');
    } catch (err: unknown) {
      const msg = err instanceof Error ? err.message : 'Failed to create assignment';
      setError(msg);
    }
  };

  const handleDeleteAssignment = async (id: number) => {
    try {
      await deleteSpecialRoleAssignment(id);
      setAssignments(prev => prev.filter(a => a.id !== id));
      setSuccess('Assignment deleted');
    } catch (err) {
      setError('Failed to delete assignment');
    }
  };

  if (isLoading) {
    return (
      <div className="p-4 sm:p-8 flex items-center justify-center">
        <div className="flex items-center gap-3">
          <div className="w-6 h-6 border-2 border-stark-500 border-t-transparent rounded-full animate-spin" />
          <span className="text-slate-400">Loading special roles...</span>
        </div>
      </div>
    );
  }

  return (
    <div className="p-4 sm:p-8">
      {/* Header */}
      <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-4 mb-6 sm:mb-8">
        <div>
          <h1 className="text-xl sm:text-2xl font-bold text-white mb-1 sm:mb-2">Special Roles</h1>
          <p className="text-sm sm:text-base text-slate-400">
            Enrich safe mode with extra tools for specific users
          </p>
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

      {/* Tab bar */}
      <div className="flex items-center gap-1 border-b border-slate-700/50 mb-0 overflow-x-auto pb-px">
        <button
          onClick={() => setActiveTab('roles')}
          className={`px-4 py-2.5 text-sm font-medium whitespace-nowrap transition-colors border-b-2 -mb-px ${
            activeTab === 'roles'
              ? 'border-stark-500 text-white'
              : 'border-transparent text-slate-400 hover:text-slate-200 hover:border-slate-600'
          }`}
        >
          Roles ({roles.length})
        </button>
        <button
          onClick={() => setActiveTab('assignments')}
          className={`px-4 py-2.5 text-sm font-medium whitespace-nowrap transition-colors border-b-2 -mb-px ${
            activeTab === 'assignments'
              ? 'border-stark-500 text-white'
              : 'border-transparent text-slate-400 hover:text-slate-200 hover:border-slate-600'
          }`}
        >
          Assignments ({assignments.length})
        </button>
      </div>

      {/* Tab content */}
      {activeTab === 'roles' && (
        <Card className="rounded-t-none border-t-0">
          <CardContent>
            <div className="flex flex-col lg:flex-row gap-6">
              {/* Left: role list */}
              <div className="lg:w-56 shrink-0">
                <div className="space-y-1 mb-3">
                  {roles.map(role => (
                    <button
                      key={role.name}
                      onClick={() => selectRole(role)}
                      className={`w-full text-left px-3 py-2 rounded-lg text-sm transition-colors ${
                        !isCreating && selectedRole === role.name
                          ? 'bg-stark-500/20 text-stark-400'
                          : 'text-slate-400 hover:text-white hover:bg-slate-700/50'
                      }`}
                    >
                      <div className="font-medium">{role.name}</div>
                      <div className="text-xs text-slate-500 truncate">
                        {role.allowed_tools.length} tools, {role.allowed_skills.length} skills
                      </div>
                    </button>
                  ))}
                  {isCreating && (
                    <div className="px-3 py-2 rounded-lg text-sm bg-stark-500/20 text-stark-400 font-medium">
                      + New Role
                    </div>
                  )}
                </div>
                <Button
                  size="sm"
                  variant="secondary"
                  onClick={handleStartCreate}
                  disabled={isCreating || roles.length >= MAX_ROLES}
                  className="w-full"
                >
                  <Plus className="w-4 h-4 mr-1" />
                  Add Role ({roles.length}/{MAX_ROLES})
                </Button>
              </div>

              {/* Right: edit form */}
              <div className="flex-1 min-w-0">
                {(selectedRole || isCreating) ? (
                  <>
                    <div className="flex items-center justify-between mb-4">
                      <h2 className="text-lg font-semibold text-white">
                        {isCreating ? 'New Role' : selectedRole}
                      </h2>
                      <div className="flex items-center gap-2">
                        {isCreating && (
                          <Button variant="ghost" size="sm" onClick={handleCancelCreate}>
                            <X className="w-4 h-4 mr-1" /> Cancel
                          </Button>
                        )}
                        {!isCreating && selectedRole && (
                          <Button
                            variant="ghost"
                            size="sm"
                            onClick={() => handleDeleteRole(selectedRole)}
                            className="text-red-400 hover:text-red-300 hover:bg-red-500/20"
                          >
                            <Trash2 className="w-4 h-4 mr-1" />
                            Delete
                          </Button>
                        )}
                        <Button size="sm" onClick={handleSave} isLoading={isSaving}>
                          <Save className="w-4 h-4 mr-1" />
                          {isCreating ? 'Create' : 'Save'}
                        </Button>
                      </div>
                    </div>

                    <div className="space-y-4">
                      {isCreating && (
                        <div>
                          <label className="block text-xs text-slate-500 mb-1">Name (unique ID)</label>
                          <input
                            type="text"
                            value={editName}
                            onChange={e => setEditName(e.target.value.toLowerCase().replace(/[^a-z0-9_]/g, ''))}
                            placeholder="power_user"
                            className="w-full bg-slate-900/50 border border-slate-700 rounded-lg px-3 py-2 text-sm text-white placeholder-slate-600 focus:outline-none focus:border-stark-500"
                          />
                        </div>
                      )}

                      <div>
                        <label className="block text-xs text-slate-500 mb-1">Description</label>
                        <input
                          type="text"
                          value={editDescription}
                          onChange={e => setEditDescription(e.target.value)}
                          placeholder="Users with social media posting privileges"
                          className="w-full bg-slate-900/50 border border-slate-700 rounded-lg px-3 py-2 text-sm text-white placeholder-slate-600 focus:outline-none focus:border-stark-500"
                        />
                      </div>

                      <div>
                        <label className="block text-xs text-slate-500 mb-1">
                          Allowed Tools (comma-separated)
                          <span className="text-slate-600 ml-1">-- extra tools added to safe mode for this role</span>
                        </label>
                        <input
                          type="text"
                          value={editTools}
                          onChange={e => setEditTools(e.target.value)}
                          placeholder="twitter_post, discord_write"
                          className="w-full bg-slate-900/50 border border-slate-700 rounded-lg px-3 py-2 text-sm text-white placeholder-slate-600 focus:outline-none focus:border-stark-500"
                        />
                      </div>

                      <div>
                        <label className="block text-xs text-slate-500 mb-1">
                          Allowed Skills (comma-separated)
                          <span className="text-slate-600 ml-1">-- skill names available to this role (required tools are auto-granted)</span>
                        </label>
                        <input
                          type="text"
                          value={editSkills}
                          onChange={e => setEditSkills(e.target.value)}
                          placeholder="image_generation, weather"
                          className="w-full bg-slate-900/50 border border-slate-700 rounded-lg px-3 py-2 text-sm text-white placeholder-slate-600 focus:outline-none focus:border-stark-500"
                        />
                      </div>
                    </div>
                  </>
                ) : (
                  <div className="flex flex-col items-center justify-center py-12 text-slate-500">
                    <ShieldCheck className="w-12 h-12 mb-4 text-slate-600" />
                    <p>Select a role or create a new one</p>
                  </div>
                )}
              </div>
            </div>
          </CardContent>
        </Card>
      )}

      {activeTab === 'assignments' && (
        <Card className="rounded-t-none border-t-0">
          <CardContent>
            {/* Add assignment form */}
            <div className="flex flex-col sm:flex-row gap-3 mb-6 p-4 bg-slate-900/30 rounded-lg border border-slate-700/50">
              <div className="flex-1 min-w-0">
                <label className="block text-xs text-slate-500 mb-1">Channel Type</label>
                <select
                  value={newAssignChannelType}
                  onChange={e => setNewAssignChannelType(e.target.value)}
                  className="w-full bg-slate-900/50 border border-slate-700 rounded-lg px-3 py-2 text-sm text-white focus:outline-none focus:border-stark-500"
                >
                  {CHANNEL_TYPES.map(ct => (
                    <option key={ct} value={ct}>{ct}</option>
                  ))}
                </select>
              </div>
              <div className="flex-1 min-w-0">
                <label className="block text-xs text-slate-500 mb-1">User ID</label>
                <input
                  type="text"
                  value={newAssignUserId}
                  onChange={e => setNewAssignUserId(e.target.value)}
                  placeholder="123456789"
                  className="w-full bg-slate-900/50 border border-slate-700 rounded-lg px-3 py-2 text-sm text-white placeholder-slate-600 focus:outline-none focus:border-stark-500"
                />
              </div>
              <div className="flex-1 min-w-0">
                <label className="block text-xs text-slate-500 mb-1">Role</label>
                <select
                  value={newAssignRoleName}
                  onChange={e => setNewAssignRoleName(e.target.value)}
                  className="w-full bg-slate-900/50 border border-slate-700 rounded-lg px-3 py-2 text-sm text-white focus:outline-none focus:border-stark-500"
                >
                  <option value="">Select role...</option>
                  {roles.map(r => (
                    <option key={r.name} value={r.name}>{r.name}</option>
                  ))}
                </select>
              </div>
              <div className="flex-1 min-w-0">
                <label className="block text-xs text-slate-500 mb-1">Label</label>
                <input
                  type="text"
                  value={newAssignLabel}
                  onChange={e => setNewAssignLabel(e.target.value)}
                  placeholder="optional label"
                  className="w-full bg-slate-900/50 border border-slate-700 rounded-lg px-3 py-2 text-sm text-white placeholder-slate-600 focus:outline-none focus:border-stark-500"
                />
              </div>
              <div className="flex items-end">
                <Button onClick={handleCreateAssignment} size="sm" disabled={assignments.length >= MAX_ASSIGNMENTS}>
                  <Plus className="w-4 h-4 mr-1" />
                  Assign
                </Button>
              </div>
            </div>

            {/* Assignments table */}
            {assignments.length > 0 ? (
              <div className="overflow-x-auto">
                <table className="w-full text-sm">
                  <thead>
                    <tr className="border-b border-slate-700/50">
                      <th className="text-left py-2 px-3 text-slate-500 font-medium">Channel</th>
                      <th className="text-left py-2 px-3 text-slate-500 font-medium">User ID</th>
                      <th className="text-left py-2 px-3 text-slate-500 font-medium">Label</th>
                      <th className="text-left py-2 px-3 text-slate-500 font-medium">Role</th>
                      <th className="text-left py-2 px-3 text-slate-500 font-medium">Created</th>
                      <th className="text-right py-2 px-3 text-slate-500 font-medium">Actions</th>
                    </tr>
                  </thead>
                  <tbody>
                    {assignments.map(a => (
                      <tr key={a.id} className="border-b border-slate-800/50 hover:bg-slate-800/30">
                        <td className="py-2 px-3 text-slate-300">{a.channel_type}</td>
                        <td className="py-2 px-3 text-slate-300 font-mono text-xs">{a.user_id}</td>
                        <td className="py-2 px-3 text-slate-400 text-xs">{a.label || '—'}</td>
                        <td className="py-2 px-3">
                          <span className="px-2 py-0.5 bg-stark-500/20 text-stark-400 rounded text-xs font-medium">
                            {a.special_role_name}
                          </span>
                        </td>
                        <td className="py-2 px-3 text-slate-500 text-xs">
                          {new Date(a.created_at).toLocaleDateString()}
                        </td>
                        <td className="py-2 px-3 text-right">
                          <button
                            onClick={() => handleDeleteAssignment(a.id)}
                            className="text-red-400 hover:text-red-300 p-1"
                            title="Delete assignment"
                          >
                            <Trash2 className="w-4 h-4" />
                          </button>
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            ) : (
              <div className="text-center py-8 text-slate-500">
                <p>No role assignments yet. Create a role first, then assign it to users.</p>
              </div>
            )}
          </CardContent>
        </Card>
      )}

      {/* FAQ */}
      <div className="mt-8 p-4 bg-slate-800/50 border border-slate-700/50 rounded-lg text-sm text-slate-400 space-y-2">
        <p className="text-slate-300 font-medium">How Special Roles Work</p>
        <p>
          By default, safe mode channels are fully locked down to a fixed set of read-only tools (say_to_user, web_fetch, token_lookup, memory/discord/telegram read, etc.). No special roles exist out of the box.
        </p>
        <p>
          <strong className="text-slate-300">Roles</strong> are named permission bundles (e.g. <code className="text-xs bg-slate-700 px-1 rounded">power_user</code>) that grant extra tools and skills by name. They do nothing until assigned to a user.
        </p>
        <p>
          <strong className="text-slate-300">Assignments</strong> link a role to a specific (channel type, user ID) pair. Each user can have at most one role per channel type.
        </p>
        <p>
          <strong className="text-slate-300">Allowed Tools</strong> are standalone tools added directly to the user's safe mode allow list (e.g. giving someone <code className="text-xs bg-slate-700 px-1 rounded">web_fetch</code> access without a specific skill).
        </p>
        <p>
          <strong className="text-slate-300">Allowed Skills</strong> are granted by exact skill name. When a skill is granted, its required tools are <em>automatically</em> added to the allow list &mdash; you don't need to add them separately. For example, granting the <code className="text-xs bg-slate-700 px-1 rounded">image_generation</code> skill auto-grants its required <code className="text-xs bg-slate-700 px-1 rounded">x402_preset_fetch</code> tool.
        </p>
        <p>
          When an assigned user messages a safe-mode channel, the dispatcher enriches their session with the role's tools and skills. Unassigned users get vanilla safe mode. Sessions with enriched permissions show a badge in the session list.
        </p>
      </div>

      {/* Warning */}
      <div className="mt-4 p-4 bg-amber-500/10 border border-amber-500/40 rounded-lg text-sm flex items-start gap-3">
        <span className="text-amber-400 text-lg leading-none mt-0.5">&#9888;</span>
        <div>
          <p className="text-amber-300 font-semibold mb-1">Warning</p>
          <p className="text-amber-200/80">
            Enabling tools for external users can be extremely hazardous. Granting write-capable tools (posting, transactions, file operations) to an outside user means their messages can trigger real-world actions through the agent. Only grant special roles if you know exactly what you are doing.
          </p>
        </div>
      </div>
    </div>
  );
}
