import React, { useState, useEffect, useRef, useMemo } from 'react';
import { Zap, Upload, Trash2, ExternalLink, Code, X, Save, Edit2 } from 'lucide-react';
import Card, { CardContent } from '@/components/ui/Card';
import Button from '@/components/ui/Button';
import { getSkills, uploadSkill, deleteSkill, setSkillEnabled, getSkillDetail, updateSkillBody, SkillInfo, SkillDetail } from '@/lib/api';

export default function Skills() {
  const [skills, setSkills] = useState<SkillInfo[]>([]);
  const [isLoading, setIsLoading] = useState(true);
  const [isUploading, setIsUploading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const fileInputRef = useRef<HTMLInputElement>(null);

  // Filter state
  const [activeFilter, setActiveFilter] = useState('All');

  const FILTER_CATEGORIES: Record<string, string[]> = {
    'All': [],
    'Finance': ['crypto', 'defi', 'finance', 'trading', 'swap', 'transfer', 'wallet', 'yield', 'lending', 'bridge', 'payments'],
    'Code': ['development', 'git', 'code', 'debugging', 'testing', 'deployment', 'ci-cd', 'devops', 'infrastructure'],
    'Social': ['social', 'messaging', 'twitter', 'discord', 'telegram', 'communication', 'social-media'],
    'Secretary': ['secretary', 'productivity', 'notes', 'scheduling', 'cron', 'automation'],
  };

  const filteredSkills = useMemo(() => {
    if (activeFilter === 'All') return skills;
    const tags = FILTER_CATEGORIES[activeFilter] || [];
    return skills.filter((s) =>
      s.tags?.some((t) => tags.includes(t.toLowerCase()))
    );
  }, [skills, activeFilter]);

  // Editor state
  const [selectedSkill, setSelectedSkill] = useState<SkillDetail | null>(null);
  const [, setIsLoadingDetail] = useState(false);
  const [isEditing, setIsEditing] = useState(false);
  const [editedBody, setEditedBody] = useState('');
  const [isSaving, setIsSaving] = useState(false);
  const [saveMessage, setSaveMessage] = useState<string | null>(null);

  useEffect(() => {
    loadSkills();
  }, []);

  const loadSkills = async () => {
    try {
      const data = await getSkills();
      setSkills(data);
    } catch (err) {
      setError('Failed to load skills');
    } finally {
      setIsLoading(false);
    }
  };

  const handleUpload = async (e: React.ChangeEvent<HTMLInputElement>) => {
    const file = e.target.files?.[0];
    if (!file) return;

    setIsUploading(true);
    setError(null);

    try {
      await uploadSkill(file);
      await loadSkills();
    } catch (err) {
      setError('Failed to upload skill');
    } finally {
      setIsUploading(false);
      if (fileInputRef.current) {
        fileInputRef.current.value = '';
      }
    }
  };

  const handleDelete = async (name: string) => {
    if (!confirm(`Are you sure you want to delete the skill "${name}"?`)) return;

    try {
      await deleteSkill(name);
      setSkills((prev) => prev.filter((s) => s.name !== name));
      if (selectedSkill?.name === name) {
        setSelectedSkill(null);
        setIsEditing(false);
      }
    } catch (err) {
      setError('Failed to delete skill');
    }
  };

  const handleToggleEnabled = async (name: string, currentEnabled: boolean) => {
    try {
      await setSkillEnabled(name, !currentEnabled);
      setSkills((prev) =>
        prev.map((s) => (s.name === name ? { ...s, enabled: !currentEnabled } : s))
      );
    } catch (err) {
      setError('Failed to update skill');
    }
  };

  const handleOpenDetail = async (name: string) => {
    setIsLoadingDetail(true);
    setError(null);
    setIsEditing(false);
    setSaveMessage(null);

    try {
      const detail = await getSkillDetail(name);
      setSelectedSkill(detail);
      setEditedBody(detail.prompt_template);
    } catch (err) {
      setError('Failed to load skill details');
    } finally {
      setIsLoadingDetail(false);
    }
  };

  const handleCloseDetail = () => {
    setSelectedSkill(null);
    setIsEditing(false);
    setSaveMessage(null);
  };

  const handleStartEdit = () => {
    if (selectedSkill) {
      setEditedBody(selectedSkill.prompt_template);
      setIsEditing(true);
      setSaveMessage(null);
    }
  };

  const handleCancelEdit = () => {
    if (selectedSkill) {
      setEditedBody(selectedSkill.prompt_template);
    }
    setIsEditing(false);
  };

  const handleSave = async () => {
    if (!selectedSkill) return;
    setIsSaving(true);
    setSaveMessage(null);

    try {
      await updateSkillBody(selectedSkill.name, editedBody);
      setSelectedSkill({ ...selectedSkill, prompt_template: editedBody });
      setIsEditing(false);
      setSaveMessage('Saved successfully');
      setTimeout(() => setSaveMessage(null), 3000);
    } catch (err) {
      setError('Failed to save skill');
    } finally {
      setIsSaving(false);
    }
  };

  if (isLoading) {
    return (
      <div className="p-4 sm:p-8 flex items-center justify-center">
        <div className="flex items-center gap-3">
          <div className="w-6 h-6 border-2 border-stark-500 border-t-transparent rounded-full animate-spin" />
          <span className="text-slate-400">Loading skills...</span>
        </div>
      </div>
    );
  }

  return (
    <div className="p-4 sm:p-8">
      <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-4 mb-6 sm:mb-8">
        <div>
          <h1 className="text-xl sm:text-2xl font-bold text-white mb-1 sm:mb-2">Skills</h1>
          <p className="text-sm sm:text-base text-slate-400">Extend your agent with custom skills</p>
        </div>
        <div>
          <input
            ref={fileInputRef}
            type="file"
            accept=".zip,.md"
            onChange={handleUpload}
            className="hidden"
          />
          <Button
            onClick={() => fileInputRef.current?.click()}
            isLoading={isUploading}
            className="w-full sm:w-auto"
          >
            <Upload className="w-4 h-4 mr-2" />
            Upload Skill
          </Button>
        </div>
      </div>

      {error && (
        <div className="mb-6 bg-red-500/20 border border-red-500/50 text-red-400 px-4 py-3 rounded-lg">
          {error}
        </div>
      )}

      {/* Filter Pills */}
      <div className="flex flex-wrap gap-2 mb-6">
        {Object.keys(FILTER_CATEGORIES).map((category) => (
          <button
            key={category}
            onClick={() => setActiveFilter(category)}
            className={`px-3 py-1.5 text-sm rounded-full transition-colors ${
              activeFilter === category
                ? 'bg-stark-500 text-white'
                : 'bg-slate-800 text-slate-400 hover:bg-slate-700 hover:text-slate-300'
            }`}
          >
            {category}
          </button>
        ))}
      </div>

      {filteredSkills.length > 0 ? (
        <div className="grid gap-4">
          {filteredSkills.map((skill) => {
            const isSelected = selectedSkill?.name === skill.name;
            return (
            <Card key={skill.name} className={isSelected ? 'border-stark-500/50' : ''}>
              <CardContent>
                {/* Mobile: stacked layout, Desktop: side by side */}
                <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-3 sm:gap-4">
                  {/* Main content */}
                  <div
                    className="flex items-start sm:items-center gap-2 sm:gap-4 min-w-0 cursor-pointer"
                    onClick={() => isSelected ? handleCloseDetail() : handleOpenDetail(skill.name)}
                  >
                    {/* Icon - smaller on mobile */}
                    <div className="p-1.5 sm:p-3 bg-amber-500/20 rounded-lg shrink-0">
                      <Zap className="w-4 h-4 sm:w-6 sm:h-6 text-amber-400" />
                    </div>
                    <div className="min-w-0 flex-1">
                      {/* Title row */}
                      <div className="flex items-center gap-2 flex-wrap">
                        <h3 className="font-semibold text-white text-sm sm:text-base">{skill.name}</h3>
                        {skill.version && (
                          <span className="text-xs px-1.5 py-0.5 bg-slate-700 text-slate-400 rounded">
                            v{skill.version}
                          </span>
                        )}
                        {skill.homepage && (
                          <a
                            href={skill.homepage}
                            target="_blank"
                            rel="noopener noreferrer"
                            className="text-slate-400 hover:text-stark-400"
                            onClick={(e) => e.stopPropagation()}
                          >
                            <ExternalLink className="w-3.5 h-3.5 sm:w-4 sm:h-4" />
                          </a>
                        )}
                      </div>
                      {/* Source badge on separate line on mobile */}
                      {skill.source && (
                        <span className="inline-block text-xs px-1.5 py-0.5 bg-slate-700/50 text-slate-500 rounded mt-1">
                          {skill.source}
                        </span>
                      )}
                      {/* Description */}
                      {skill.description && (
                        <p className="text-xs sm:text-sm text-slate-400 mt-1.5">{skill.description}</p>
                      )}
                      {/* Tags */}
                      {skill.tags && skill.tags.length > 0 && (
                        <div className="flex flex-wrap gap-1 mt-2">
                          {skill.tags.map((tag) => (
                            <span
                              key={tag}
                              className="text-xs px-1.5 py-0.5 bg-stark-500/10 text-stark-400 rounded"
                            >
                              {tag}
                            </span>
                          ))}
                        </div>
                      )}
                    </div>
                  </div>
                  {/* Action buttons - bottom right on mobile */}
                  <div className="flex items-center gap-2 self-end sm:self-center shrink-0">
                    <Button
                      variant="ghost"
                      size="sm"
                      onClick={() => isSelected ? handleCloseDetail() : handleOpenDetail(skill.name)}
                      className="text-slate-400 hover:text-stark-400 hover:bg-stark-500/20 p-1.5 sm:p-2"
                    >
                      <Code className="w-4 h-4" />
                    </Button>
                    <button
                      onClick={() => handleToggleEnabled(skill.name, skill.enabled)}
                      className={`px-2 py-1 text-xs rounded cursor-pointer transition-colors ${
                        skill.enabled
                          ? 'bg-green-500/20 text-green-400 hover:bg-green-500/30'
                          : 'bg-slate-700 text-slate-400 hover:bg-slate-600'
                      }`}
                    >
                      {skill.enabled ? 'Enabled' : 'Disabled'}
                    </button>
                    <Button
                      variant="ghost"
                      size="sm"
                      onClick={() => handleDelete(skill.name)}
                      className="text-red-400 hover:text-red-300 hover:bg-red-500/20 p-1.5 sm:p-2"
                    >
                      <Trash2 className="w-4 h-4" />
                    </Button>
                  </div>
                </div>

                {/* Inline detail/editor - expands inside the same card */}
                {isSelected && (
                  <div className="mt-4 pt-4 border-t border-slate-700/50">
                    {/* Edit/Save/Cancel toolbar */}
                    <div className="flex items-center justify-between mb-3">
                      <span className="text-xs text-slate-500 uppercase tracking-wider">Prompt Template</span>
                      <div className="flex items-center gap-2">
                        {saveMessage && (
                          <span className="text-xs text-green-400">{saveMessage}</span>
                        )}
                        {!isEditing ? (
                          <button
                            onClick={handleStartEdit}
                            className="flex items-center gap-1.5 px-3 py-1.5 text-sm text-slate-300 hover:text-white hover:bg-slate-700 rounded-lg transition-colors"
                          >
                            <Edit2 className="w-3.5 h-3.5" />
                            Edit
                          </button>
                        ) : (
                          <>
                            <button
                              onClick={handleCancelEdit}
                              className="flex items-center gap-1.5 px-3 py-1.5 text-sm text-slate-300 hover:text-white hover:bg-slate-700 rounded-lg transition-colors"
                            >
                              <X className="w-3.5 h-3.5" />
                              Cancel
                            </button>
                            <button
                              onClick={handleSave}
                              disabled={isSaving}
                              className="flex items-center gap-1.5 px-3 py-1.5 text-sm text-stark-400 hover:text-white hover:bg-stark-500/20 rounded-lg transition-colors disabled:opacity-50"
                            >
                              <Save className="w-3.5 h-3.5" />
                              {isSaving ? 'Saving...' : 'Save'}
                            </button>
                          </>
                        )}
                      </div>
                    </div>

                    {/* Body Editor/Viewer */}
                    <div className="border border-slate-700 rounded-lg overflow-hidden bg-slate-900/50">
                      {isEditing ? (
                        <textarea
                          value={editedBody}
                          onChange={(e) => setEditedBody(e.target.value)}
                          className="w-full h-80 p-4 bg-transparent text-sm text-slate-300 font-mono resize-none focus:outline-none"
                          spellCheck={false}
                        />
                      ) : (
                        <pre className="p-4 text-sm text-slate-300 font-mono whitespace-pre-wrap break-words max-h-80 overflow-y-auto">
                          {selectedSkill.prompt_template}
                        </pre>
                      )}
                    </div>

                    {/* Scripts info */}
                    {selectedSkill.scripts && selectedSkill.scripts.length > 0 && (
                      <div className="mt-3">
                        <span className="text-xs text-slate-500">Scripts: </span>
                        {selectedSkill.scripts.map((s) => (
                          <span key={s.name} className="text-xs px-1.5 py-0.5 bg-slate-700 text-slate-400 rounded mr-1">
                            {s.name} ({s.language})
                          </span>
                        ))}
                      </div>
                    )}
                  </div>
                )}
              </CardContent>
            </Card>
            );
          })}
        </div>
      ) : (
        <Card>
          <CardContent className="text-center py-12">
            <Zap className="w-12 h-12 text-slate-600 mx-auto mb-4" />
            {skills.length > 0 ? (
              <p className="text-slate-400">No skills matching "{activeFilter}"</p>
            ) : (
              <>
                <p className="text-slate-400 mb-4">No skills installed</p>
                <Button
                  variant="secondary"
                  onClick={() => fileInputRef.current?.click()}
                >
                  <Upload className="w-4 h-4 mr-2" />
                  Upload Your First Skill
                </Button>
              </>
            )}
          </CardContent>
        </Card>
      )}
    </div>
  );
}
