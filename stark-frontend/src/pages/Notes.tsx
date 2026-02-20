import { useState, useEffect } from 'react';
import {
  FileText,
  RefreshCw,
  AlertCircle,
  Folder,
  FolderOpen,
  ChevronRight,
  ChevronDown,
  ArrowLeft,
  Search,
  Tag,
  X,
  Download,
  List,
  Hash,
  Trash2,
} from 'lucide-react';
import {
  listNotes,
  readNoteFile,
  searchNotes,
  getNotesTags,
  getNotesByTag,
  exportNotesZip,
  deleteNote,
  NoteEntry,
  TagItem,
  NotesByTagGroup,
} from '@/lib/api';

interface TreeNode {
  name: string;
  path: string;
  is_dir: boolean;
  size: number;
  modified?: string;
  children?: TreeNode[];
  expanded?: boolean;
  loaded?: boolean;
}

/** Deterministic tag-to-color: hashes the tag name to pick a hue, returns soft pastel colors */
function tagColor(tag: string): { bg: string; text: string; border: string } {
  let hash = 0;
  for (let i = 0; i < tag.length; i++) {
    hash = tag.charCodeAt(i) + ((hash << 5) - hash);
    hash = hash & hash;
  }
  const hue = ((hash % 360) + 360) % 360;
  // Pastel palette: low-saturation tinted bg, soft muted text, subtle border
  return {
    bg: `hsla(${hue}, 40%, 20%, 0.35)`,
    text: `hsl(${hue}, 55%, 80%)`,
    border: `hsla(${hue}, 35%, 45%, 0.4)`,
  };
}

export default function Notes() {
  const [tree, setTree] = useState<TreeNode[]>([]);
  const [selectedFile, setSelectedFile] = useState<string | null>(null);
  const [fileContent, setFileContent] = useState<string | null>(null);
  const [fileMeta, setFileMeta] = useState<{
    title?: string;
    tags?: string[];
    note_type?: string;
  } | null>(null);
  const [isLoading, setIsLoading] = useState(true);
  const [isLoadingFile, setIsLoadingFile] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [fileError, setFileError] = useState<string | null>(null);
  const [mobileView, setMobileView] = useState<'tree' | 'preview'>('tree');

  // Search state
  const [searchQuery, setSearchQuery] = useState('');
  const [searchResults, setSearchResults] = useState<
    { file_path: string; title: string; tags: string; snippet: string }[] | null
  >(null);
  const [isSearching, setIsSearching] = useState(false);

  // Tag filter
  const [tags, setTags] = useState<TagItem[]>([]);
  const [activeTag, setActiveTag] = useState<string | null>(null);

  // View mode: files or tags
  const [viewMode, setViewMode] = useState<'files' | 'tags'>('files');
  const [tagGroups, setTagGroups] = useState<NotesByTagGroup[]>([]);
  const [expandedTags, setExpandedTags] = useState<Set<string>>(new Set());
  const [isLoadingTags, setIsLoadingTags] = useState(false);

  // Export
  const [isExporting, setIsExporting] = useState(false);

  // Delete
  const [isDeleting, setIsDeleting] = useState(false);
  const [deleteConfirm, setDeleteConfirm] = useState<string | null>(null);

  const loadDirectory = async (path?: string): Promise<TreeNode[]> => {
    const response = await listNotes(path);
    if (!response.success) {
      throw new Error(response.error || 'Failed to load directory');
    }
    return response.entries.map((entry: NoteEntry) => ({
      ...entry,
      expanded: false,
      loaded: !entry.is_dir,
      children: entry.is_dir ? [] : undefined,
    }));
  };

  const loadRoot = async () => {
    setIsLoading(true);
    setError(null);
    try {
      const [nodes, tagsRes] = await Promise.all([loadDirectory(), getNotesTags()]);
      setTree(nodes);
      if (tagsRes.success) {
        setTags(tagsRes.tags);
      }
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to load notes');
    } finally {
      setIsLoading(false);
    }
  };

  const loadTagGroups = async () => {
    setIsLoadingTags(true);
    try {
      const res = await getNotesByTag();
      if (res.success) {
        setTagGroups(res.groups);
      }
    } catch {
      // ignore
    } finally {
      setIsLoadingTags(false);
    }
  };

  const toggleDirectory = async (node: TreeNode) => {
    if (!node.is_dir) return;

    if (!node.loaded) {
      try {
        const children = await loadDirectory(node.path);
        setTree((prevTree) =>
          updateNodeInTree(prevTree, node.path, {
            ...node,
            expanded: true,
            loaded: true,
            children,
          })
        );
      } catch (err) {
        console.error('Failed to load directory:', err);
      }
    } else {
      setTree((prevTree) =>
        updateNodeInTree(prevTree, node.path, {
          ...node,
          expanded: !node.expanded,
        })
      );
    }
  };

  const updateNodeInTree = (
    nodes: TreeNode[],
    targetPath: string,
    newNode: TreeNode
  ): TreeNode[] => {
    return nodes.map((n) => {
      if (n.path === targetPath) {
        return newNode;
      }
      if (
        n.children &&
        n.is_dir &&
        (targetPath.startsWith(n.path + '/') ||
          targetPath.startsWith(n.path + '\\'))
      ) {
        return {
          ...n,
          children: updateNodeInTree(n.children, targetPath, newNode),
        };
      }
      return n;
    });
  };

  const loadFile = async (path: string) => {
    setIsLoadingFile(true);
    setFileError(null);
    setFileContent(null);
    setFileMeta(null);
    setSelectedFile(path);
    setMobileView('preview');
    try {
      const response = await readNoteFile(path);
      if (response.success && response.content !== undefined) {
        setFileContent(response.content);
        setFileMeta({
          title: response.title,
          tags: response.tags,
          note_type: response.note_type,
        });
      } else {
        setFileError(response.error || 'Failed to load file');
      }
    } catch (err) {
      setFileError('Failed to load file');
    } finally {
      setIsLoadingFile(false);
    }
  };

  const handleSearch = async () => {
    if (!searchQuery.trim()) {
      setSearchResults(null);
      return;
    }
    setIsSearching(true);
    try {
      const res = await searchNotes(searchQuery.trim(), 20);
      if (res.success) {
        setSearchResults(res.results);
      }
    } catch {
      // ignore
    } finally {
      setIsSearching(false);
    }
  };

  const handleTagFilter = async (tag: string) => {
    if (activeTag === tag) {
      setActiveTag(null);
      setSearchResults(null);
      return;
    }
    setActiveTag(tag);
    setIsSearching(true);
    try {
      const res = await searchNotes(tag, 50);
      if (res.success) {
        setSearchResults(res.results);
      }
    } catch {
      // ignore
    } finally {
      setIsSearching(false);
    }
  };

  const clearSearch = () => {
    setSearchQuery('');
    setSearchResults(null);
    setActiveTag(null);
  };

  const handleExport = async () => {
    setIsExporting(true);
    try {
      const blob = await exportNotesZip();
      const url = URL.createObjectURL(blob);
      const a = document.createElement('a');
      a.href = url;
      a.download = `starkbot-notes-${new Date().toISOString().slice(0, 10)}.zip`;
      document.body.appendChild(a);
      a.click();
      document.body.removeChild(a);
      URL.revokeObjectURL(url);
    } catch (err) {
      console.error('Export failed:', err);
    } finally {
      setIsExporting(false);
    }
  };

  const handleDelete = async (path: string) => {
    if (deleteConfirm !== path) {
      setDeleteConfirm(path);
      return;
    }
    setIsDeleting(true);
    try {
      const res = await deleteNote(path);
      if (res.success) {
        // Clear selection and refresh
        setSelectedFile(null);
        setFileContent(null);
        setFileMeta(null);
        setDeleteConfirm(null);
        await loadRoot();
        if (viewMode === 'tags') {
          loadTagGroups();
        }
      }
    } catch (err) {
      console.error('Delete failed:', err);
    } finally {
      setIsDeleting(false);
    }
  };

  const toggleTagGroup = (tag: string) => {
    setExpandedTags((prev) => {
      const next = new Set(prev);
      if (next.has(tag)) {
        next.delete(tag);
      } else {
        next.add(tag);
      }
      return next;
    });
  };

  useEffect(() => {
    loadRoot();
  }, []);

  // Load tag groups when switching to tags view
  useEffect(() => {
    if (viewMode === 'tags' && tagGroups.length === 0) {
      loadTagGroups();
    }
  }, [viewMode]);

  const refresh = () => {
    loadRoot();
    if (viewMode === 'tags') {
      loadTagGroups();
    }
    setSelectedFile(null);
    setFileContent(null);
    setFileMeta(null);
    clearSearch();
  };

  const renderTree = (
    nodes: TreeNode[],
    depth: number = 0
  ): JSX.Element[] => {
    return nodes.map((node) => (
      <div key={node.path}>
        <button
          onClick={() => {
            if (node.is_dir) {
              toggleDirectory(node);
            } else {
              loadFile(node.path);
            }
          }}
          className={`w-full flex items-center gap-2 px-3 py-2 text-left transition-colors hover:bg-slate-700/50 ${
            selectedFile === node.path
              ? 'bg-stark-500/20 text-stark-400'
              : 'text-slate-300'
          }`}
          style={{ paddingLeft: `${depth * 16 + 12}px` }}
        >
          {node.is_dir ? (
            <>
              {node.expanded ? (
                <ChevronDown className="w-4 h-4 flex-shrink-0 text-slate-500" />
              ) : (
                <ChevronRight className="w-4 h-4 flex-shrink-0 text-slate-500" />
              )}
              {node.expanded ? (
                <FolderOpen className="w-4 h-4 flex-shrink-0 text-amber-400" />
              ) : (
                <Folder className="w-4 h-4 flex-shrink-0 text-amber-400" />
              )}
            </>
          ) : (
            <>
              <span className="w-4" />
              <FileText className="w-4 h-4 flex-shrink-0 text-slate-400" />
            </>
          )}
          <span className="truncate text-sm">{node.name}</span>
          {node.modified && !node.is_dir && (
            <span className="ml-auto text-xs text-slate-500 flex-shrink-0">
              {node.modified.split(' ')[0]}
            </span>
          )}
        </button>
        {node.is_dir && node.expanded && node.children && (
          <div>{renderTree(node.children, depth + 1)}</div>
        )}
      </div>
    ));
  };

  const renderTagGroups = (): JSX.Element => {
    if (isLoadingTags) {
      return (
        <div className="flex items-center justify-center h-32">
          <RefreshCw className="w-6 h-6 text-slate-400 animate-spin" />
        </div>
      );
    }

    if (tagGroups.length === 0) {
      return (
        <div className="flex flex-col items-center justify-center h-32 text-slate-400">
          <Tag className="w-8 h-8 mb-2" />
          <span className="text-sm">No tagged notes yet</span>
          <span className="text-xs mt-1">
            Tags will appear when notes have tags
          </span>
        </div>
      );
    }

    return (
      <div className="py-1">
        {tagGroups.map((group) => {
          const colors = tagColor(group.tag);
          const isExpanded = expandedTags.has(group.tag);
          return (
            <div key={group.tag}>
              <button
                onClick={() => toggleTagGroup(group.tag)}
                className="w-full flex items-center gap-2 px-3 py-2 text-left transition-colors hover:bg-slate-700/50"
              >
                {isExpanded ? (
                  <ChevronDown className="w-4 h-4 flex-shrink-0 text-slate-500" />
                ) : (
                  <ChevronRight className="w-4 h-4 flex-shrink-0 text-slate-500" />
                )}
                <span
                  className="inline-flex items-center gap-1 px-2 py-0.5 rounded text-xs font-medium border"
                  style={{
                    backgroundColor: colors.bg,
                    color: colors.text,
                    borderColor: colors.border,
                  }}
                >
                  <Hash className="w-3 h-3" />
                  {group.tag}
                </span>
                <span className="text-xs text-slate-500">
                  {group.count} note{group.count !== 1 ? 's' : ''}
                </span>
              </button>
              {isExpanded && (
                <div>
                  {group.notes.map((note) => (
                    <button
                      key={`${group.tag}-${note.file_path}`}
                      onClick={() => loadFile(note.file_path)}
                      className={`w-full flex items-center gap-2 px-3 py-2 text-left transition-colors hover:bg-slate-700/50 ${
                        selectedFile === note.file_path
                          ? 'bg-stark-500/20 text-stark-400'
                          : 'text-slate-300'
                      }`}
                      style={{ paddingLeft: '44px' }}
                    >
                      <FileText className="w-4 h-4 flex-shrink-0 text-slate-400" />
                      <div className="min-w-0 flex-1">
                        <div className="text-sm truncate">
                          {note.title || note.file_path}
                        </div>
                        {note.tags && (
                          <div className="flex flex-wrap gap-1 mt-0.5">
                            {note.tags
                              .split(',')
                              .map((t) => t.trim())
                              .filter((t) => t && t !== group.tag)
                              .slice(0, 3)
                              .map((t) => {
                                const c = tagColor(t);
                                return (
                                  <span
                                    key={t}
                                    className="text-[10px] px-1.5 py-0 rounded border"
                                    style={{
                                      backgroundColor: c.bg,
                                      color: c.text,
                                      borderColor: c.border,
                                    }}
                                  >
                                    {t}
                                  </span>
                                );
                              })}
                          </div>
                        )}
                      </div>
                    </button>
                  ))}
                </div>
              )}
            </div>
          );
        })}
      </div>
    );
  };

  // Strip frontmatter from display content
  const stripFrontmatter = (content: string): string => {
    const trimmed = content.trimStart();
    if (!trimmed.startsWith('---')) return content;
    const afterOpen = trimmed.slice(3);
    const closeIdx = afterOpen.indexOf('\n---');
    if (closeIdx === -1) return content;
    return afterOpen.slice(closeIdx + 4).trimStart();
  };

  return (
    <div className="h-full flex flex-col">
      {/* Header */}
      <div className="p-4 md:p-6 border-b border-slate-700">
        <div className="flex items-center justify-between mb-2">
          <div>
            <h1 className="text-2xl font-bold text-white">Notes</h1>
            <p className="text-slate-400 text-sm mt-1">
              Obsidian-compatible knowledge base
            </p>
          </div>
          <div className="flex items-center gap-1">
            <button
              onClick={handleExport}
              disabled={isExporting}
              className="p-2 text-slate-400 hover:text-white hover:bg-slate-700 rounded-lg transition-colors disabled:opacity-50"
              title="Export for Obsidian"
            >
              {isExporting ? (
                <RefreshCw className="w-5 h-5 animate-spin" />
              ) : (
                <Download className="w-5 h-5" />
              )}
            </button>
            <button
              onClick={refresh}
              className="p-2 text-slate-400 hover:text-white hover:bg-slate-700 rounded-lg transition-colors"
              title="Refresh"
            >
              <RefreshCw className="w-5 h-5" />
            </button>
          </div>
        </div>

        {/* Search bar */}
        <div className="flex gap-2 mt-3">
          <div className="relative flex-1">
            <Search className="absolute left-3 top-1/2 -translate-y-1/2 w-4 h-4 text-slate-500" />
            <input
              type="text"
              value={searchQuery}
              onChange={(e) => setSearchQuery(e.target.value)}
              onKeyDown={(e) => e.key === 'Enter' && handleSearch()}
              placeholder="Search notes..."
              className="w-full pl-9 pr-8 py-2 bg-slate-800 border border-slate-600 rounded-lg text-sm text-slate-200 placeholder-slate-500 focus:outline-none focus:border-stark-500"
            />
            {(searchQuery || searchResults) && (
              <button
                onClick={clearSearch}
                className="absolute right-2 top-1/2 -translate-y-1/2 text-slate-500 hover:text-slate-300"
              >
                <X className="w-4 h-4" />
              </button>
            )}
          </div>
          <button
            onClick={handleSearch}
            disabled={isSearching}
            className="px-4 py-2 bg-stark-600 hover:bg-stark-500 text-white rounded-lg text-sm transition-colors disabled:opacity-50"
          >
            {isSearching ? (
              <RefreshCw className="w-4 h-4 animate-spin" />
            ) : (
              'Search'
            )}
          </button>
        </div>

        {/* View toggle + Tag pills */}
        <div className="flex items-center gap-3 mt-3">
          {/* Files / Tags toggle */}
          <div className="flex bg-slate-800 rounded-lg p-0.5 flex-shrink-0">
            <button
              onClick={() => setViewMode('files')}
              className={`flex items-center gap-1.5 px-3 py-1 rounded-md text-xs font-medium transition-colors ${
                viewMode === 'files'
                  ? 'bg-slate-600 text-white'
                  : 'text-slate-400 hover:text-slate-300'
              }`}
            >
              <List className="w-3.5 h-3.5" />
              Files
            </button>
            <button
              onClick={() => setViewMode('tags')}
              className={`flex items-center gap-1.5 px-3 py-1 rounded-md text-xs font-medium transition-colors ${
                viewMode === 'tags'
                  ? 'bg-slate-600 text-white'
                  : 'text-slate-400 hover:text-slate-300'
              }`}
            >
              <Tag className="w-3.5 h-3.5" />
              Tags
            </button>
          </div>

          {/* Tag pills (scrollable) */}
          {tags.length > 0 && (
            <div className="flex flex-wrap gap-1.5 min-w-0">
              {tags.slice(0, 15).map((t) => {
                const colors = tagColor(t.tag);
                const isActive = activeTag === t.tag;
                return (
                  <button
                    key={t.tag}
                    onClick={() => handleTagFilter(t.tag)}
                    className="inline-flex items-center gap-1 px-2 py-0.5 rounded-full text-xs transition-colors border"
                    style={
                      isActive
                        ? {
                            backgroundColor: colors.bg,
                            color: colors.text,
                            borderColor: colors.text,
                          }
                        : {
                            backgroundColor: colors.bg,
                            color: colors.text,
                            borderColor: colors.border,
                          }
                    }
                  >
                    <Hash className="w-3 h-3" />
                    {t.tag}
                    <span style={{ opacity: 0.6 }}>{t.count}</span>
                  </button>
                );
              })}
            </div>
          )}
        </div>
      </div>

      {/* Content */}
      <div className="flex-1 flex overflow-hidden">
        {/* Left panel: Tree, Tags, or Search Results */}
        <div
          className={`w-full md:w-80 border-r border-slate-700 overflow-y-auto ${
            mobileView === 'preview' ? 'hidden md:block' : ''
          }`}
        >
          {searchResults !== null ? (
            // Show search results
            <div className="py-2">
              <div className="px-3 py-2 text-xs text-slate-500 uppercase tracking-wider">
                {searchResults.length} result
                {searchResults.length !== 1 ? 's' : ''}
              </div>
              {searchResults.length === 0 ? (
                <div className="px-4 py-6 text-center text-slate-500 text-sm">
                  No notes found
                </div>
              ) : (
                searchResults.map((r) => (
                  <button
                    key={r.file_path}
                    onClick={() => loadFile(r.file_path)}
                    className={`w-full text-left px-4 py-3 hover:bg-slate-700/50 transition-colors border-b border-slate-800 ${
                      selectedFile === r.file_path
                        ? 'bg-stark-500/20'
                        : ''
                    }`}
                  >
                    <div className="text-sm font-medium text-slate-200 truncate">
                      {r.title || r.file_path}
                    </div>
                    <div className="text-xs text-slate-500 truncate mt-0.5">
                      {r.file_path}
                    </div>
                    {r.tags && (
                      <div className="flex flex-wrap gap-1 mt-1">
                        {r.tags
                          .split(',')
                          .map((t) => t.trim())
                          .filter(Boolean)
                          .map((t) => {
                            const c = tagColor(t);
                            return (
                              <span
                                key={t}
                                className="text-[10px] px-1.5 py-0 rounded border"
                                style={{
                                  backgroundColor: c.bg,
                                  color: c.text,
                                  borderColor: c.border,
                                }}
                              >
                                {t}
                              </span>
                            );
                          })}
                      </div>
                    )}
                    {r.snippet && (
                      <div className="text-xs text-slate-400 mt-1 line-clamp-2">
                        {r.snippet
                          .replace(/>>>/g, '')
                          .replace(/<<</g, '')}
                      </div>
                    )}
                  </button>
                ))
              )}
            </div>
          ) : viewMode === 'tags' ? (
            renderTagGroups()
          ) : isLoading ? (
            <div className="flex items-center justify-center h-32">
              <RefreshCw className="w-6 h-6 text-slate-400 animate-spin" />
            </div>
          ) : error ? (
            <div className="p-4">
              <div className="flex items-center gap-2 text-amber-400 bg-amber-500/10 px-4 py-3 rounded-lg">
                <AlertCircle className="w-5 h-5 flex-shrink-0" />
                <span className="text-sm">{error}</span>
              </div>
            </div>
          ) : tree.length === 0 ? (
            <div className="flex flex-col items-center justify-center h-32 text-slate-400">
              <FileText className="w-8 h-8 mb-2" />
              <span className="text-sm">No notes yet</span>
              <span className="text-xs mt-1">
                Ask your agent to create a note
              </span>
            </div>
          ) : (
            <div className="py-2">{renderTree(tree)}</div>
          )}
        </div>

        {/* Right panel: File Preview */}
        <div
          className={`flex-1 overflow-hidden flex flex-col bg-slate-900 ${
            mobileView === 'tree' ? 'hidden md:flex' : ''
          }`}
        >
          {selectedFile ? (
            <>
              <div className="px-4 py-3 border-b border-slate-700 flex items-center gap-2">
                <button
                  onClick={() => setMobileView('tree')}
                  className="md:hidden p-1 -ml-1 mr-1 text-slate-400 hover:text-white hover:bg-slate-700 rounded-lg transition-colors"
                >
                  <ArrowLeft className="w-4 h-4" />
                </button>
                <FileText className="w-4 h-4 text-slate-400 flex-shrink-0" />
                <span className="text-sm text-slate-300 font-mono truncate flex-1">
                  {selectedFile}
                </span>
                <button
                  onClick={() => handleDelete(selectedFile!)}
                  disabled={isDeleting}
                  className={`p-1.5 rounded-lg transition-colors flex-shrink-0 ${
                    deleteConfirm === selectedFile
                      ? 'bg-red-500/20 text-red-400 hover:bg-red-500/30'
                      : 'text-slate-500 hover:text-red-400 hover:bg-slate-700'
                  }`}
                  title={deleteConfirm === selectedFile ? 'Click again to confirm delete' : 'Delete note'}
                >
                  {isDeleting ? (
                    <RefreshCw className="w-4 h-4 animate-spin" />
                  ) : (
                    <Trash2 className="w-4 h-4" />
                  )}
                </button>
              </div>
              {deleteConfirm === selectedFile && (
                <div className="px-4 py-2 bg-red-500/10 border-b border-red-500/20 flex items-center justify-between">
                  <span className="text-xs text-red-400">Click delete again to confirm</span>
                  <button
                    onClick={() => setDeleteConfirm(null)}
                    className="text-xs text-slate-500 hover:text-slate-300"
                  >
                    Cancel
                  </button>
                </div>
              )}

              {/* Metadata header */}
              {fileMeta && (fileMeta.title || fileMeta.tags) && (
                <div className="px-4 py-2 border-b border-slate-800 bg-slate-850">
                  {fileMeta.title && (
                    <h2 className="text-lg font-semibold text-white">
                      {fileMeta.title}
                    </h2>
                  )}
                  <div className="flex items-center gap-2 mt-1 flex-wrap">
                    {fileMeta.note_type && fileMeta.note_type !== 'note' && (
                      <span className="text-xs px-2 py-0.5 bg-stark-600/30 text-stark-400 rounded">
                        {fileMeta.note_type}
                      </span>
                    )}
                    {fileMeta.tags?.map((tag) => {
                      const colors = tagColor(tag);
                      return (
                        <span
                          key={tag}
                          className="text-xs px-2 py-0.5 rounded border"
                          style={{
                            backgroundColor: colors.bg,
                            color: colors.text,
                            borderColor: colors.border,
                          }}
                        >
                          #{tag}
                        </span>
                      );
                    })}
                  </div>
                </div>
              )}

              <div className="flex-1 overflow-auto">
                {isLoadingFile ? (
                  <div className="flex items-center justify-center h-32">
                    <RefreshCw className="w-6 h-6 text-slate-400 animate-spin" />
                  </div>
                ) : fileError ? (
                  <div className="p-4">
                    <div className="flex items-center gap-2 text-amber-400 bg-amber-500/10 px-4 py-3 rounded-lg">
                      <AlertCircle className="w-5 h-5 flex-shrink-0" />
                      <span className="text-sm">{fileError}</span>
                    </div>
                  </div>
                ) : fileContent !== null ? (
                  <div className="p-4">
                    <div className="prose prose-invert prose-sm max-w-none">
                      <pre className="whitespace-pre-wrap break-words text-slate-300 font-mono text-sm bg-transparent p-0 m-0">
                        {stripFrontmatter(fileContent)}
                      </pre>
                    </div>
                  </div>
                ) : null}
              </div>
            </>
          ) : (
            <div className="flex-1 flex flex-col items-center justify-center text-slate-500">
              <FileText className="w-12 h-12 mb-3 opacity-50" />
              <p>Select a note to view</p>
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
