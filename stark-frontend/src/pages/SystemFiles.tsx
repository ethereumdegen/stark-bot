import { useState, useEffect } from 'react';
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
} from 'lucide-react';
import {
  listIntrinsicFiles,
  readIntrinsicFile,
  writeIntrinsicFile,
  IntrinsicFileInfo,
} from '@/lib/api';

export default function SystemFiles() {
  const [files, setFiles] = useState<IntrinsicFileInfo[]>([]);
  const [selectedFile, setSelectedFile] = useState<string | null>(null);
  const [fileContent, setFileContent] = useState<string | null>(null);
  const [editedContent, setEditedContent] = useState<string>('');
  const [isWritable, setIsWritable] = useState(false);
  const [isLoading, setIsLoading] = useState(true);
  const [isLoadingFile, setIsLoadingFile] = useState(false);
  const [isSaving, setIsSaving] = useState(false);
  const [isEditing, setIsEditing] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [fileError, setFileError] = useState<string | null>(null);
  const [saveMessage, setSaveMessage] = useState<string | null>(null);

  const loadFiles = async () => {
    setIsLoading(true);
    setError(null);
    try {
      const fileList = await listIntrinsicFiles();
      setFiles(fileList);
    } catch (err) {
      setError('Failed to load system files');
    } finally {
      setIsLoading(false);
    }
  };

  const loadFile = async (name: string) => {
    setIsLoadingFile(true);
    setFileError(null);
    setFileContent(null);
    setSelectedFile(name);
    setIsEditing(false);
    setSaveMessage(null);
    try {
      const response = await readIntrinsicFile(name);
      if (response.success && response.content !== undefined) {
        setFileContent(response.content);
        setEditedContent(response.content);
        setIsWritable(response.writable);
      } else {
        setFileError(response.error || 'Failed to load file');
      }
    } catch (err) {
      setFileError('Failed to load file');
    } finally {
      setIsLoadingFile(false);
    }
  };

  const handleSave = async () => {
    if (!selectedFile || !isWritable) return;

    setIsSaving(true);
    setSaveMessage(null);
    try {
      const response = await writeIntrinsicFile(selectedFile, editedContent);
      if (response.success) {
        setFileContent(editedContent);
        setIsEditing(false);
        setSaveMessage('Saved successfully');
        setTimeout(() => setSaveMessage(null), 3000);
      } else {
        setFileError(response.error || 'Failed to save file');
      }
    } catch (err) {
      setFileError('Failed to save file');
    } finally {
      setIsSaving(false);
    }
  };

  const handleCancelEdit = () => {
    setEditedContent(fileContent || '');
    setIsEditing(false);
  };

  useEffect(() => {
    loadFiles();
  }, []);

  const refresh = () => {
    loadFiles();
    if (selectedFile) {
      loadFile(selectedFile);
    }
  };

  return (
    <div className="h-full flex flex-col">
      {/* Header */}
      <div className="p-6 border-b border-slate-700">
        <div className="flex items-center justify-between mb-2">
          <div>
            <h1 className="text-2xl font-bold text-white">System Files</h1>
            <p className="text-slate-400 text-sm mt-1">
              Core configuration files that define the agent's behavior
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
          ) : files.length === 0 ? (
            <div className="flex flex-col items-center justify-center h-32 text-slate-400">
              <FileCode className="w-8 h-8 mb-2" />
              <span className="text-sm">No system files found</span>
            </div>
          ) : (
            <div className="divide-y divide-slate-700/50">
              {files.map((file) => {
                const isSelected = selectedFile === file.name;
                return (
                  <button
                    key={file.name}
                    onClick={() => loadFile(file.name)}
                    className={`w-full flex items-center gap-3 px-4 py-4 text-left transition-colors ${
                      isSelected
                        ? 'bg-stark-500/20 text-stark-400'
                        : 'text-slate-300 hover:bg-slate-700/50'
                    }`}
                  >
                    {file.name === 'soul.md' ? (
                      <Sparkles className="w-5 h-5 flex-shrink-0 text-purple-400" />
                    ) : (
                      <FileCode className="w-5 h-5 flex-shrink-0 text-slate-400" />
                    )}
                    <div className="flex-1 min-w-0">
                      <div className="flex items-center gap-2">
                        <span className="font-medium truncate">{file.name}</span>
                        {!file.writable && (
                          <span title="Read-only">
                            <Lock className="w-3.5 h-3.5 text-slate-500" />
                          </span>
                        )}
                      </div>
                      <div className="text-xs text-slate-500 truncate">
                        {file.description}
                      </div>
                    </div>
                  </button>
                );
              })}
            </div>
          )}
        </div>

        {/* File Preview/Editor */}
        <div className="flex-1 overflow-hidden flex flex-col bg-slate-900">
          {selectedFile ? (
            <>
              <div className="px-4 py-3 border-b border-slate-700 flex items-center justify-between">
                <div className="flex items-center gap-2">
                  {selectedFile === 'soul.md' ? (
                    <Sparkles className="w-4 h-4 text-purple-400" />
                  ) : (
                    <FileCode className="w-4 h-4 text-slate-400" />
                  )}
                  <span className="text-sm text-slate-300 font-mono">
                    {selectedFile}
                  </span>
                  {!isWritable && (
                    <span className="px-2 py-0.5 text-xs bg-slate-700 text-slate-400 rounded">
                      Read-only
                    </span>
                  )}
                  {saveMessage && (
                    <span className="flex items-center gap-1 px-2 py-0.5 text-xs bg-green-500/20 text-green-400 rounded">
                      <Check className="w-3 h-3" />
                      {saveMessage}
                    </span>
                  )}
                </div>
                <div className="flex items-center gap-2">
                  {isWritable && !isEditing && (
                    <button
                      onClick={() => setIsEditing(true)}
                      className="flex items-center gap-1.5 px-3 py-1.5 text-sm text-slate-300 hover:text-white hover:bg-slate-700 rounded-lg transition-colors"
                    >
                      <Edit2 className="w-4 h-4" />
                      Edit
                    </button>
                  )}
                  {isEditing && (
                    <>
                      <button
                        onClick={handleCancelEdit}
                        className="flex items-center gap-1.5 px-3 py-1.5 text-sm text-slate-400 hover:text-white hover:bg-slate-700 rounded-lg transition-colors"
                      >
                        <X className="w-4 h-4" />
                        Cancel
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
                        Save
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
