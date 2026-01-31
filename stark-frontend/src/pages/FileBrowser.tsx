import { useState, useEffect } from 'react';
import {
  Folder,
  File,
  ChevronRight,
  Home,
  RefreshCw,
  ArrowLeft,
  FileCode,
  FileText,
  FileJson,
  Image,
  AlertCircle,
  FolderOpen,
} from 'lucide-react';
import { listFiles, readFile, getWorkspaceInfo, FileEntry } from '@/lib/api';

function formatSize(bytes: number): string {
  if (bytes === 0) return '-';
  const units = ['B', 'KB', 'MB', 'GB'];
  let i = 0;
  let size = bytes;
  while (size >= 1024 && i < units.length - 1) {
    size /= 1024;
    i++;
  }
  return `${size.toFixed(i > 0 ? 1 : 0)} ${units[i]}`;
}

function getFileIcon(name: string, isDir: boolean) {
  if (isDir) return Folder;

  const ext = name.split('.').pop()?.toLowerCase() || '';

  // Code files
  if (['ts', 'tsx', 'js', 'jsx', 'rs', 'py', 'go', 'java', 'c', 'cpp', 'h', 'rb', 'php', 'swift', 'kt'].includes(ext)) {
    return FileCode;
  }
  // JSON/Config files
  if (['json', 'yaml', 'yml', 'toml', 'xml', 'ini', 'env'].includes(ext)) {
    return FileJson;
  }
  // Text files
  if (['txt', 'md', 'log', 'csv'].includes(ext)) {
    return FileText;
  }
  // Image files
  if (['png', 'jpg', 'jpeg', 'gif', 'svg', 'ico', 'webp'].includes(ext)) {
    return Image;
  }

  return File;
}

export default function FileBrowser() {
  const [currentPath, setCurrentPath] = useState('');
  const [entries, setEntries] = useState<FileEntry[]>([]);
  const [selectedFile, setSelectedFile] = useState<string | null>(null);
  const [fileContent, setFileContent] = useState<string | null>(null);
  const [isLoading, setIsLoading] = useState(true);
  const [isLoadingFile, setIsLoadingFile] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [fileError, setFileError] = useState<string | null>(null);
  const [workspaceInfo, setWorkspaceInfo] = useState<{ path: string; exists: boolean } | null>(null);

  const loadDirectory = async (path: string = '') => {
    setIsLoading(true);
    setError(null);
    try {
      const response = await listFiles(path);
      if (response.success) {
        setEntries(response.entries);
        setCurrentPath(path);
      } else {
        setError(response.error || 'Failed to load directory');
        setEntries([]);
      }
    } catch (err) {
      setError('Failed to load directory');
      setEntries([]);
    } finally {
      setIsLoading(false);
    }
  };

  const loadFile = async (path: string) => {
    setIsLoadingFile(true);
    setFileError(null);
    setFileContent(null);
    setSelectedFile(path);
    try {
      const response = await readFile(path);
      if (response.success && response.content !== undefined) {
        setFileContent(response.content);
      } else {
        setFileError(response.error || 'Failed to load file');
      }
    } catch (err) {
      setFileError('Failed to load file');
    } finally {
      setIsLoadingFile(false);
    }
  };

  const loadWorkspaceInfo = async () => {
    try {
      const info = await getWorkspaceInfo();
      setWorkspaceInfo({ path: info.workspace_path, exists: info.exists });
    } catch (err) {
      console.error('Failed to load workspace info:', err);
    }
  };

  useEffect(() => {
    loadWorkspaceInfo();
    loadDirectory();
  }, []);

  // Auto-refresh directory listing every 5 seconds
  useEffect(() => {
    const interval = setInterval(() => {
      // Silently refresh without showing loading state
      listFiles(currentPath).then((response) => {
        if (response.success) {
          setEntries(response.entries);
        }
      }).catch(() => {
        // Silently ignore refresh errors
      });
    }, 5000);

    return () => clearInterval(interval);
  }, [currentPath]);

  const handleEntryClick = (entry: FileEntry) => {
    if (entry.is_dir) {
      loadDirectory(entry.path);
      setSelectedFile(null);
      setFileContent(null);
    } else {
      loadFile(entry.path);
    }
  };

  const navigateUp = () => {
    if (!currentPath) return;
    const parts = currentPath.split('/');
    parts.pop();
    const parentPath = parts.join('/');
    loadDirectory(parentPath);
    setSelectedFile(null);
    setFileContent(null);
  };

  const navigateHome = () => {
    loadDirectory('');
    setSelectedFile(null);
    setFileContent(null);
  };

  const refresh = () => {
    loadDirectory(currentPath);
    if (selectedFile) {
      loadFile(selectedFile);
    }
  };

  const pathParts = currentPath ? currentPath.split('/') : [];

  return (
    <div className="h-full flex flex-col">
      {/* Header */}
      <div className="p-6 border-b border-slate-700">
        <div className="flex items-center justify-between mb-4">
          <div>
            <h1 className="text-2xl font-bold text-white">File Browser</h1>
            <p className="text-slate-400 text-sm mt-1">
              Explore files in the agent workspace
              {workspaceInfo && (
                <span className="ml-2 text-slate-500">({workspaceInfo.path})</span>
              )}
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

        {/* Breadcrumb */}
        <div className="flex items-center gap-1 text-sm">
          <button
            onClick={navigateHome}
            className="p-1.5 text-slate-400 hover:text-white hover:bg-slate-700 rounded transition-colors"
            title="Go to workspace root"
          >
            <Home className="w-4 h-4" />
          </button>
          {currentPath && (
            <button
              onClick={navigateUp}
              className="p-1.5 text-slate-400 hover:text-white hover:bg-slate-700 rounded transition-colors"
              title="Go up"
            >
              <ArrowLeft className="w-4 h-4" />
            </button>
          )}
          <ChevronRight className="w-4 h-4 text-slate-600" />
          <span className="text-slate-400">workspace</span>
          {pathParts.map((part, index) => (
            <span key={index} className="flex items-center gap-1">
              <ChevronRight className="w-4 h-4 text-slate-600" />
              <button
                onClick={() => {
                  const newPath = pathParts.slice(0, index + 1).join('/');
                  loadDirectory(newPath);
                  setSelectedFile(null);
                  setFileContent(null);
                }}
                className="text-slate-300 hover:text-white transition-colors"
              >
                {part}
              </button>
            </span>
          ))}
        </div>
      </div>

      {/* Content */}
      <div className="flex-1 flex overflow-hidden">
        {/* File List */}
        <div className="w-80 border-r border-slate-700 overflow-y-auto">
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
          ) : entries.length === 0 ? (
            <div className="flex flex-col items-center justify-center h-32 text-slate-400">
              <FolderOpen className="w-8 h-8 mb-2" />
              <span className="text-sm">Empty directory</span>
            </div>
          ) : (
            <div className="divide-y divide-slate-700/50">
              {entries.map((entry) => {
                const Icon = getFileIcon(entry.name, entry.is_dir);
                const isSelected = selectedFile === entry.path;
                return (
                  <button
                    key={entry.path}
                    onClick={() => handleEntryClick(entry)}
                    className={`w-full flex items-center gap-3 px-4 py-3 text-left transition-colors ${
                      isSelected
                        ? 'bg-stark-500/20 text-stark-400'
                        : 'text-slate-300 hover:bg-slate-700/50'
                    }`}
                  >
                    <Icon
                      className={`w-5 h-5 flex-shrink-0 ${
                        entry.is_dir ? 'text-amber-400' : 'text-slate-400'
                      }`}
                    />
                    <div className="flex-1 min-w-0">
                      <div className="truncate font-medium">{entry.name}</div>
                      <div className="text-xs text-slate-500 flex items-center gap-2">
                        {!entry.is_dir && <span>{formatSize(entry.size)}</span>}
                        {entry.modified && <span>{entry.modified}</span>}
                      </div>
                    </div>
                    {entry.is_dir && (
                      <ChevronRight className="w-4 h-4 text-slate-500" />
                    )}
                  </button>
                );
              })}
            </div>
          )}
        </div>

        {/* File Preview */}
        <div className="flex-1 overflow-hidden flex flex-col bg-slate-900">
          {selectedFile ? (
            <>
              <div className="px-4 py-3 border-b border-slate-700 flex items-center gap-2">
                <FileCode className="w-4 h-4 text-slate-400" />
                <span className="text-sm text-slate-300 font-mono truncate">
                  {selectedFile}
                </span>
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
                ) : fileContent !== null ? (
                  <pre className="p-4 text-sm text-slate-300 font-mono whitespace-pre-wrap break-words">
                    {fileContent}
                  </pre>
                ) : null}
              </div>
            </>
          ) : (
            <div className="flex-1 flex flex-col items-center justify-center text-slate-500">
              <File className="w-12 h-12 mb-3 opacity-50" />
              <p>Select a file to preview</p>
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
