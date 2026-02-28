import { useState, useEffect, useCallback, useRef } from 'react';
import { useSearchParams } from 'react-router-dom';
import {
  FileCode,
  RefreshCw,
  AlertCircle,
  Save,
  Edit2,
  X,
  Check,
  Lock,
  Sparkles,
  ClipboardList,
  Trash2,
  ArrowLeft,
  FolderOpen,
  Folder,
  ChevronRight,
  ChevronDown,
  FilePlus,
} from 'lucide-react';
import {
  listIntrinsicFiles,
  listIntrinsicDir,
  readIntrinsicFile,
  writeIntrinsicFile,
  deleteIntrinsicFile,
  IntrinsicFileInfo,
} from '@/lib/api';

interface TreeNode {
  name: string;
  path: string; // full path relative to /api/intrinsic/, e.g. "skills/aave/SKILL.md"
  is_dir: boolean;
  writable: boolean;
  deletable: boolean;
  description?: string;
  children?: TreeNode[];
  loaded?: boolean;
}

export default function SystemFiles() {
  const [searchParams, setSearchParams] = useSearchParams();
  const [rootNodes, setRootNodes] = useState<TreeNode[]>([]);
  const [expandedFolders, setExpandedFolders] = useState<Set<string>>(new Set());
  const [currentPath, setCurrentPath] = useState<string | null>(null);
  const [fileContent, setFileContent] = useState<string | null>(null);
  const [editedContent, setEditedContent] = useState<string>('');
  const [isWritable, setIsWritable] = useState(false);
  const [isDeletable, setIsDeletable] = useState(false);
  const [isLoading, setIsLoading] = useState(true);
  const [isLoadingFile, setIsLoadingFile] = useState(false);
  const [isSaving, setIsSaving] = useState(false);
  const [isEditing, setIsEditing] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [fileError, setFileError] = useState<string | null>(null);
  const [isDeleting, setIsDeleting] = useState(false);
  const [saveMessage, setSaveMessage] = useState<string | null>(null);
  const [mobileView, setMobileView] = useState<'list' | 'preview'>('list');
  const deepLinkHandled = useRef(false);

  const loadRootNodes = useCallback(async () => {
    setIsLoading(true);
    setError(null);
    try {
      const fileList = await listIntrinsicFiles();
      const nodes: TreeNode[] = fileList.map((f: IntrinsicFileInfo) => ({
        name: f.name,
        path: f.name,
        is_dir: f.is_dir ?? false,
        writable: f.writable,
        deletable: f.deletable ?? false,
        description: f.description,
        children: f.is_dir ? [] : undefined,
        loaded: !f.is_dir,
      }));
      setRootNodes(nodes);
    } catch {
      setError('Failed to load system files');
    } finally {
      setIsLoading(false);
    }
  }, []);

  const loadDirChildren = useCallback(async (dirPath: string): Promise<TreeNode[]> => {
    try {
      const entries = await listIntrinsicDir(dirPath);
      return entries.map((e: IntrinsicFileInfo) => ({
        name: e.name,
        path: `${dirPath}/${e.name}`,
        is_dir: e.is_dir ?? false,
        writable: e.writable,
        deletable: e.deletable ?? false,
        description: e.description,
        children: e.is_dir ? [] : undefined,
        loaded: !e.is_dir,
      }));
    } catch {
      return [];
    }
  }, []);

  const updateNodeChildren = useCallback((nodes: TreeNode[], targetPath: string, children: TreeNode[]): TreeNode[] => {
    return nodes.map(node => {
      if (node.path === targetPath) {
        return { ...node, children, loaded: true };
      }
      if (node.children && targetPath.startsWith(node.path + '/')) {
        return { ...node, children: updateNodeChildren(node.children, targetPath, children) };
      }
      return node;
    });
  }, []);

  const toggleFolder = useCallback(async (path: string) => {
    const newExpanded = new Set(expandedFolders);
    if (newExpanded.has(path)) {
      newExpanded.delete(path);
    } else {
      newExpanded.add(path);
      // Lazy-load children
      const children = await loadDirChildren(path);
      setRootNodes(prev => updateNodeChildren(prev, path, children));
    }
    setExpandedFolders(newExpanded);
  }, [expandedFolders, loadDirChildren, updateNodeChildren]);

  const loadFile = useCallback(async (path: string) => {
    setIsLoadingFile(true);
    setFileError(null);
    setFileContent(null);
    setCurrentPath(path);
    setIsEditing(false);
    setSaveMessage(null);
    setMobileView('preview');
    try {
      const response = await readIntrinsicFile(path);
      if (response.success && response.content !== undefined) {
        setFileContent(response.content);
        setEditedContent(response.content);
        setIsWritable(response.writable);
        // Determine deletability from tree node
        const node = findNode(rootNodes, path);
        setIsDeletable(node?.deletable ?? false);
      } else {
        setFileError(response.error || 'Failed to load file');
      }
    } catch {
      setFileError('Failed to load file');
    } finally {
      setIsLoadingFile(false);
    }
  }, [rootNodes]);

  const findNode = (nodes: TreeNode[], path: string): TreeNode | undefined => {
    for (const node of nodes) {
      if (node.path === path) return node;
      if (node.children) {
        const found = findNode(node.children, path);
        if (found) return found;
      }
    }
    return undefined;
  };

  const handleSave = async () => {
    if (!currentPath || !isWritable) return;

    setIsSaving(true);
    setSaveMessage(null);
    try {
      const response = await writeIntrinsicFile(currentPath, editedContent);
      if (response.success) {
        setFileContent(editedContent);
        setIsEditing(false);
        setSaveMessage('Saved successfully');
        setTimeout(() => setSaveMessage(null), 3000);
      } else {
        setFileError(response.error || 'Failed to save file');
      }
    } catch {
      setFileError('Failed to save file');
    } finally {
      setIsSaving(false);
    }
  };

  const handleDelete = async () => {
    if (!currentPath) return;
    if (!confirm(`Delete ${currentPath}?`)) return;

    setIsDeleting(true);
    setFileError(null);
    try {
      const response = await deleteIntrinsicFile(currentPath);
      if (response.success) {
        setFileContent(null);
        setCurrentPath(null);
        setIsEditing(false);
        setSaveMessage(null);
        setMobileView('list');
        // Refresh parent folder
        const parentPath = currentPath.split('/').slice(0, -1).join('/');
        if (parentPath) {
          const children = await loadDirChildren(parentPath);
          setRootNodes(prev => updateNodeChildren(prev, parentPath, children));
        } else {
          loadRootNodes();
        }
      } else {
        setFileError(response.error || 'Failed to delete');
      }
    } catch {
      setFileError('Failed to delete');
    } finally {
      setIsDeleting(false);
    }
  };

  const handleCancelEdit = () => {
    setEditedContent(fileContent || '');
    setIsEditing(false);
  };

  const handleNewFile = async (dirPath: string) => {
    const filename = prompt('New file name:');
    if (!filename || !filename.trim()) return;

    const newPath = `${dirPath}/${filename.trim()}`;
    try {
      const response = await writeIntrinsicFile(newPath, '');
      if (response.success) {
        // Refresh the parent directory
        const children = await loadDirChildren(dirPath);
        setRootNodes(prev => updateNodeChildren(prev, dirPath, children));
        // Open the new file
        loadFile(newPath);
      } else {
        setFileError(response.error || 'Failed to create file');
      }
    } catch {
      setFileError('Failed to create file');
    }
  };

  useEffect(() => {
    loadRootNodes();
  }, [loadRootNodes]);

  // Deep-link: auto-expand folders and load file from ?path= query param
  useEffect(() => {
    if (isLoading || deepLinkHandled.current || rootNodes.length === 0) return;
    const targetPath = searchParams.get('path');
    if (!targetPath) return;
    deepLinkHandled.current = true;
    // Clear the query param so refreshing doesn't re-trigger
    searchParams.delete('path');
    setSearchParams(searchParams, { replace: true });

    (async () => {
      const segments = targetPath.split('/').filter(Boolean);
      const expanded = new Set(expandedFolders);
      let currentNodes = rootNodes;
      let updatedRoot = [...rootNodes];

      // Walk each segment, expanding directories along the way
      for (let i = 0; i < segments.length; i++) {
        const partialPath = segments.slice(0, i + 1).join('/');
        const node = currentNodes.find((n) => n.path === partialPath || n.name === segments[i]);
        if (!node) break;

        const isLast = i === segments.length - 1;
        if (node.is_dir) {
          expanded.add(node.path);
          if (!node.loaded || !node.children?.length) {
            const children = await loadDirChildren(node.path);
            updatedRoot = updateNodeChildren(updatedRoot, node.path, children);
            currentNodes = children;
          } else {
            currentNodes = node.children || [];
          }
          // If this directory is the final segment, try to find the first .md file inside
          if (isLast) {
            if (!currentNodes.length) {
              const children = await loadDirChildren(node.path);
              updatedRoot = updateNodeChildren(updatedRoot, node.path, children);
              currentNodes = children;
            }
            const mdFile = currentNodes.find((n) => !n.is_dir && n.name.endsWith('.md'));
            if (mdFile) {
              setRootNodes(updatedRoot);
              setExpandedFolders(expanded);
              loadFile(mdFile.path);
              return;
            }
          }
        } else if (isLast) {
          setRootNodes(updatedRoot);
          setExpandedFolders(expanded);
          loadFile(node.path);
          return;
        }
      }

      setRootNodes(updatedRoot);
      setExpandedFolders(expanded);
    })();
  }, [isLoading, rootNodes, searchParams, setSearchParams, expandedFolders, loadDirChildren, updateNodeChildren, loadFile]);

  const refresh = () => {
    setExpandedFolders(new Set());
    loadRootNodes();
    if (currentPath) {
      loadFile(currentPath);
    }
  };

  const getFileIcon = (name: string, isDir: boolean, isExpanded: boolean) => {
    if (isDir) {
      return isExpanded
        ? <FolderOpen className="w-4 h-4 flex-shrink-0 text-amber-400" />
        : <Folder className="w-4 h-4 flex-shrink-0 text-amber-400" />;
    }
    if (name === 'soul.md') return <Sparkles className="w-4 h-4 flex-shrink-0 text-purple-400" />;
    if (name === 'guidelines.md') return <ClipboardList className="w-4 h-4 flex-shrink-0 text-blue-400" />;
    return <FileCode className="w-4 h-4 flex-shrink-0 text-slate-400" />;
  };

  const renderTreeNode = (node: TreeNode, depth: number = 0) => {
    const isExpanded = expandedFolders.has(node.path);
    const isSelected = currentPath === node.path;
    const indent = depth * 16;

    return (
      <div key={node.path}>
        <button
          onClick={() => {
            if (node.is_dir) {
              toggleFolder(node.path);
            } else {
              loadFile(node.path);
            }
          }}
          className={`w-full flex items-center gap-2 px-3 py-2 text-left text-sm transition-colors ${
            isSelected
              ? 'bg-stark-500/20 text-stark-400'
              : 'text-slate-300 hover:bg-slate-700/50'
          }`}
          style={{ paddingLeft: `${12 + indent}px` }}
        >
          {node.is_dir ? (
            isExpanded
              ? <ChevronDown className="w-3.5 h-3.5 flex-shrink-0 text-slate-500" />
              : <ChevronRight className="w-3.5 h-3.5 flex-shrink-0 text-slate-500" />
          ) : (
            <span className="w-3.5" />
          )}
          {getFileIcon(node.name, node.is_dir, isExpanded)}
          <span className="truncate flex-1">{node.name}</span>
          {!node.writable && !node.is_dir && (
            <Lock className="w-3 h-3 text-slate-500 flex-shrink-0" />
          )}
        </button>
        {node.is_dir && isExpanded && node.children && (
          <div>
            {node.children.length === 0 ? (
              <div className="text-xs text-slate-500 py-1" style={{ paddingLeft: `${28 + indent}px` }}>
                Empty
              </div>
            ) : (
              node.children.map(child => renderTreeNode(child, depth + 1))
            )}
            {node.path.startsWith('modules/') && node.path.split('/').length >= 2 && (
              <button
                onClick={(e) => { e.stopPropagation(); handleNewFile(node.path); }}
                className="w-full flex items-center gap-2 px-3 py-1.5 text-left text-xs text-slate-500 hover:text-stark-400 hover:bg-slate-700/50 transition-colors"
                style={{ paddingLeft: `${28 + indent}px` }}
              >
                <FilePlus className="w-3.5 h-3.5" />
                New File
              </button>
            )}
          </div>
        )}
      </div>
    );
  };

  const displayName = currentPath?.split('/').pop() || currentPath || '';

  return (
    <div className="h-full flex flex-col">
      {/* Header */}
      <div className="p-4 md:p-6 border-b border-slate-700">
        <div className="flex items-center justify-between mb-2">
          <div>
            <h1 className="text-2xl font-bold text-white">System Files</h1>
            <p className="text-slate-400 text-sm mt-1 hidden md:block">
              Core configuration files and installed skills
            </p>
          </div>
          <button
            onClick={refresh}
            className="p-2 text-slate-400 hover:text-white hover:bg-slate-700 rounded-lg transition-colors"
            title="Refresh"
          >
            <RefreshCw className="w-5 h-5" />
          </button>
        </div>
      </div>

      {/* Content */}
      <div className="flex-1 flex overflow-hidden">
        {/* Tree Sidebar */}
        <div className={`w-full md:w-72 lg:w-80 border-r border-slate-700 overflow-y-auto ${mobileView === 'preview' ? 'hidden md:block' : ''}`}>
          {isLoading ? (
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
          ) : rootNodes.length === 0 ? (
            <div className="flex flex-col items-center justify-center h-32 text-slate-400">
              <FileCode className="w-8 h-8 mb-2" />
              <span className="text-sm">No system files found</span>
            </div>
          ) : (
            <div className="py-1">
              {rootNodes.map(node => renderTreeNode(node))}
            </div>
          )}
        </div>

        {/* File Preview/Editor */}
        <div className={`flex-1 overflow-hidden flex flex-col bg-slate-900 ${mobileView === 'list' ? 'hidden md:flex' : ''}`}>
          {currentPath ? (
            <>
              <div className="px-4 py-3 border-b border-slate-700 flex items-center justify-between">
                <div className="flex items-center gap-2 min-w-0">
                  <button
                    onClick={() => { setMobileView('list'); }}
                    className="md:hidden p-1 -ml-1 mr-1 text-slate-400 hover:text-white hover:bg-slate-700 rounded-lg transition-colors flex-shrink-0"
                  >
                    <ArrowLeft className="w-4 h-4" />
                  </button>
                  {getFileIcon(displayName, false, false)}
                  <span className="text-sm text-slate-300 font-mono truncate" title={currentPath}>
                    {currentPath}
                  </span>
                  {!isWritable && (
                    <span className="px-2 py-0.5 text-xs bg-slate-700 text-slate-400 rounded flex-shrink-0">
                      Read-only
                    </span>
                  )}
                  {saveMessage && (
                    <span className="flex items-center gap-1 px-2 py-0.5 text-xs bg-green-500/20 text-green-400 rounded flex-shrink-0">
                      <Check className="w-3 h-3" />
                      {saveMessage}
                    </span>
                  )}
                </div>
                <div className="flex items-center gap-2 flex-shrink-0">
                  {isWritable && !isEditing && (
                    <button
                      onClick={() => setIsEditing(true)}
                      className="flex items-center gap-1.5 px-3 py-1.5 text-sm text-slate-300 hover:text-white hover:bg-slate-700 rounded-lg transition-colors"
                    >
                      <Edit2 className="w-4 h-4" />
                      <span className="hidden sm:inline">Edit</span>
                    </button>
                  )}
                  {!isEditing && isDeletable && (
                    <button
                      onClick={handleDelete}
                      disabled={isDeleting}
                      className="flex items-center gap-1.5 px-3 py-1.5 text-sm text-red-400 hover:text-red-300 hover:bg-red-500/10 rounded-lg transition-colors disabled:opacity-50"
                    >
                      {isDeleting ? (
                        <RefreshCw className="w-4 h-4 animate-spin" />
                      ) : (
                        <Trash2 className="w-4 h-4" />
                      )}
                      <span className="hidden sm:inline">Delete</span>
                    </button>
                  )}
                  {isEditing && (
                    <>
                      <button
                        onClick={handleCancelEdit}
                        className="flex items-center gap-1.5 px-3 py-1.5 text-sm text-slate-400 hover:text-white hover:bg-slate-700 rounded-lg transition-colors"
                      >
                        <X className="w-4 h-4" />
                        <span className="hidden sm:inline">Cancel</span>
                      </button>
                      <button
                        onClick={handleSave}
                        disabled={isSaving}
                        className="flex items-center gap-1.5 px-3 py-1.5 text-sm bg-stark-500 hover:bg-stark-600 text-white rounded-lg transition-colors disabled:opacity-50"
                      >
                        {isSaving ? (
                          <RefreshCw className="w-4 h-4 animate-spin" />
                        ) : (
                          <Save className="w-4 h-4" />
                        )}
                        <span className="hidden sm:inline">Save</span>
                      </button>
                    </>
                  )}
                </div>
              </div>
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
                ) : isEditing ? (
                  <textarea
                    value={editedContent}
                    onChange={(e) => setEditedContent(e.target.value)}
                    className="w-full h-full p-4 bg-transparent text-sm text-slate-300 font-mono resize-none focus:outline-none"
                    spellCheck={false}
                  />
                ) : fileContent !== null ? (
                  <pre className="p-4 text-sm text-slate-300 font-mono whitespace-pre-wrap break-words">
                    {fileContent}
                  </pre>
                ) : null}
              </div>
            </>
          ) : (
            <div className="flex-1 flex flex-col items-center justify-center text-slate-500">
              <Sparkles className="w-12 h-12 mb-3 opacity-50" />
              <p>Select a file to view</p>
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
