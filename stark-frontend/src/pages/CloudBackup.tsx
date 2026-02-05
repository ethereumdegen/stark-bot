import { useState, useEffect } from 'react';
import { Cloud, Upload, Download, Shield, AlertCircle, CheckCircle, X, Key, Brain, Settings, Link2, RefreshCw, Clock, AlertTriangle, Heart, MessageSquare, Sparkles } from 'lucide-react';
import Card, { CardContent, CardHeader, CardTitle } from '@/components/ui/Card';
import Button from '@/components/ui/Button';
import { backupKeysToCloud, restoreKeysFromCloud, previewCloudBackup, CloudBackupPreview } from '@/lib/api';

export default function CloudBackup() {
  const [isUploading, setIsUploading] = useState(false);
  const [isDownloading, setIsDownloading] = useState(false);
  const [isPreviewing, setIsPreviewing] = useState(false);
  const [message, setMessage] = useState<{ type: 'success' | 'error'; text: string } | null>(null);
  const [confirmBackupModalOpen, setConfirmBackupModalOpen] = useState(false);
  const [confirmRestoreModalOpen, setConfirmRestoreModalOpen] = useState(false);
  const [previewData, setPreviewData] = useState<CloudBackupPreview | null>(null);
  const [noBackupWarning, setNoBackupWarning] = useState(false);

  // Backup cost in STARKBOT tokens
  const BACKUP_COST_STARKBOT = 1000;

  useEffect(() => {
    // Load preview on mount
    loadPreview();
  }, []);

  const loadPreview = async () => {
    setIsPreviewing(true);
    setMessage(null);

    try {
      const result = await previewCloudBackup();
      setPreviewData(result);
      setNoBackupWarning(false);
    } catch (err) {
      const errorMsg = err instanceof Error ? err.message : 'Unknown error';
      if (errorMsg.includes('No backup found')) {
        setNoBackupWarning(true);
      } else {
        setMessage({ type: 'error', text: formatKeystoreError(err) });
      }
      setPreviewData(null);
    } finally {
      setIsPreviewing(false);
    }
  };

  const formatKeystoreError = (err: unknown): string => {
    const message = err instanceof Error ? err.message : 'Unknown error';
    if (message.includes('keystore') || message.includes('connect') || message.includes('BadGateway')) {
      return 'Keystore server is unreachable. Please try again later.';
    }
    return message;
  };

  const handleUploadBackup = () => {
    setConfirmBackupModalOpen(true);
  };

  const confirmAndUploadBackup = async () => {
    setConfirmBackupModalOpen(false);
    setIsUploading(true);
    setMessage(null);

    try {
      const result = await backupKeysToCloud();
      setMessage({
        type: 'success',
        text: `Backup complete! ${result.key_count || 0} keys, ${result.node_count || 0} mind nodes, ${result.connection_count || 0} connections, ${result.cron_job_count || 0} cron jobs, ${result.channel_count || 0} channels${result.has_settings ? ', settings' : ''}${result.has_heartbeat ? ', heartbeat' : ''}${result.has_soul ? ', soul' : ''}`
      });
      setNoBackupWarning(false);
      // Refresh preview after successful backup
      await loadPreview();
    } catch (err) {
      setMessage({ type: 'error', text: formatKeystoreError(err) });
    } finally {
      setIsUploading(false);
    }
  };

  const handleDownloadBackup = () => {
    setConfirmRestoreModalOpen(true);
  };

  const confirmAndRestoreBackup = async () => {
    setConfirmRestoreModalOpen(false);
    setIsDownloading(true);
    setMessage(null);

    try {
      const result = await restoreKeysFromCloud();
      setMessage({
        type: 'success',
        text: `Restore complete! ${result.key_count || 0} keys, ${result.node_count || 0} mind nodes, ${result.connection_count || 0} connections, ${result.cron_job_count || 0} cron jobs, ${result.channel_count || 0} channels${result.has_settings ? ', settings' : ''}${result.has_heartbeat ? ', heartbeat' : ''}${result.has_soul ? ', soul' : ''}`
      });
    } catch (err) {
      setMessage({ type: 'error', text: formatKeystoreError(err) });
    } finally {
      setIsDownloading(false);
    }
  };

  return (
    <div className="p-8">
      <div className="mb-8">
        <h1 className="text-2xl font-bold text-white mb-2">Cloud Backup</h1>
        <p className="text-slate-400">
          Securely backup and restore your StarkBot data using encrypted cloud storage.
        </p>
      </div>

      {message && (
        <div
          className={`mb-6 px-4 py-3 rounded-lg flex items-start gap-3 ${
            message.type === 'success'
              ? 'bg-green-500/20 border border-green-500/50 text-green-400'
              : 'bg-red-500/20 border border-red-500/50 text-red-400'
          }`}
        >
          {message.type === 'success' ? (
            <CheckCircle className="w-5 h-5 flex-shrink-0 mt-0.5" />
          ) : (
            <AlertCircle className="w-5 h-5 flex-shrink-0 mt-0.5" />
          )}
          <span>{message.text}</span>
        </div>
      )}

      {/* No Backup Warning Banner */}
      {noBackupWarning && !isPreviewing && (
        <div className="mb-6 px-4 py-3 rounded-lg flex items-start gap-3 bg-yellow-500/20 border border-yellow-500/50 text-yellow-400">
          <AlertTriangle className="w-5 h-5 flex-shrink-0 mt-0.5" />
          <div>
            <p className="font-medium">No cloud backup found</p>
            <p className="text-sm text-yellow-300/80 mt-1">
              Your data is not backed up to the cloud yet. Create a backup to protect your API keys, mind map, cron jobs, and settings.
            </p>
          </div>
        </div>
      )}

      <div className="grid grid-cols-1 lg:grid-cols-2 gap-6">
        {/* Backup Actions */}
        <Card className="border-stark-500/30">
          <CardHeader>
            <div className="flex items-center gap-2">
              <Cloud className="w-5 h-5 text-stark-400" />
              <CardTitle>Backup & Restore</CardTitle>
            </div>
          </CardHeader>
          <CardContent>
            <div className="flex items-start gap-2 mb-6 p-3 bg-stark-500/10 rounded-lg">
              <Shield className="w-5 h-5 text-stark-400 mt-0.5 flex-shrink-0" />
              <div>
                <p className="text-sm text-slate-300 font-medium mb-1">End-to-End Encryption</p>
                <p className="text-xs text-slate-400">
                  Your data is encrypted with your burner wallet key using ECIES.
                  Only this StarkBot instance can decrypt the backup.
                </p>
              </div>
            </div>

            <div className="space-y-4">
              <div className="p-4 bg-slate-900/50 rounded-lg border border-slate-700">
                <div className="flex items-center justify-between mb-3">
                  <div>
                    <h3 className="text-sm font-medium text-white">Backup to Cloud</h3>
                    <p className="text-xs text-slate-400 mt-1">
                      Upload encrypted backup of all your data
                    </p>
                  </div>
                  <div className="text-right">
                    <span className="text-sm font-bold text-stark-400">{BACKUP_COST_STARKBOT}</span>
                    <span className="text-xs text-stark-300 ml-1">STARKBOT</span>
                  </div>
                </div>
                <Button
                  variant="primary"
                  size="sm"
                  onClick={handleUploadBackup}
                  isLoading={isUploading}
                  className="w-full"
                >
                  <Upload className="w-4 h-4 mr-2" />
                  Create Backup
                </Button>
              </div>

              <div className="p-4 bg-slate-900/50 rounded-lg border border-slate-700">
                <div className="mb-3">
                  <h3 className="text-sm font-medium text-white">Restore from Cloud</h3>
                  <p className="text-xs text-slate-400 mt-1">
                    Download and restore your encrypted backup
                  </p>
                </div>
                <Button
                  variant="secondary"
                  size="sm"
                  onClick={handleDownloadBackup}
                  isLoading={isDownloading}
                  disabled={!previewData}
                  className="w-full"
                >
                  <Download className="w-4 h-4 mr-2" />
                  Restore Backup
                </Button>
              </div>
            </div>
          </CardContent>
        </Card>

        {/* Cloud Backup Contents */}
        <Card>
          <CardHeader>
            <div className="flex items-center justify-between">
              <div className="flex items-center gap-2">
                <Cloud className="w-5 h-5 text-slate-400" />
                <CardTitle>Cloud Backup Contents</CardTitle>
              </div>
              <Button
                variant="ghost"
                size="sm"
                onClick={loadPreview}
                isLoading={isPreviewing}
                className="text-slate-400 hover:text-white"
              >
                <RefreshCw className="w-4 h-4" />
              </Button>
            </div>
          </CardHeader>
          <CardContent>
            {isPreviewing ? (
              <div className="flex items-center justify-center py-12">
                <div className="w-6 h-6 border-2 border-stark-500 border-t-transparent rounded-full animate-spin" />
              </div>
            ) : previewData ? (
              <div className="space-y-4">
                {/* Summary Stats */}
                <div className="grid grid-cols-2 sm:grid-cols-3 gap-3">
                  <div className="p-3 bg-slate-900/50 rounded-lg border border-slate-700">
                    <div className="flex items-center gap-2 mb-1">
                      <Key className="w-4 h-4 text-stark-400" />
                      <span className="text-xs text-slate-400">API Keys</span>
                    </div>
                    <span className="text-xl font-bold text-white">{previewData.key_count}</span>
                  </div>
                  <div className="p-3 bg-slate-900/50 rounded-lg border border-slate-700">
                    <div className="flex items-center gap-2 mb-1">
                      <Brain className="w-4 h-4 text-purple-400" />
                      <span className="text-xs text-slate-400">Mind Nodes</span>
                    </div>
                    <span className="text-xl font-bold text-white">{previewData.node_count || 0}</span>
                  </div>
                  <div className="p-3 bg-slate-900/50 rounded-lg border border-slate-700">
                    <div className="flex items-center gap-2 mb-1">
                      <Link2 className="w-4 h-4 text-blue-400" />
                      <span className="text-xs text-slate-400">Connections</span>
                    </div>
                    <span className="text-xl font-bold text-white">{previewData.connection_count || 0}</span>
                  </div>
                  <div className="p-3 bg-slate-900/50 rounded-lg border border-slate-700">
                    <div className="flex items-center gap-2 mb-1">
                      <Clock className="w-4 h-4 text-orange-400" />
                      <span className="text-xs text-slate-400">Cron Jobs</span>
                    </div>
                    <span className="text-xl font-bold text-white">{previewData.cron_job_count || 0}</span>
                  </div>
                  <div className="p-3 bg-slate-900/50 rounded-lg border border-slate-700">
                    <div className="flex items-center gap-2 mb-1">
                      <MessageSquare className="w-4 h-4 text-indigo-400" />
                      <span className="text-xs text-slate-400">Channels</span>
                    </div>
                    <span className="text-xl font-bold text-white">{previewData.channel_count || 0}</span>
                  </div>
                  <div className="p-3 bg-slate-900/50 rounded-lg border border-slate-700">
                    <div className="flex items-center gap-2 mb-1">
                      <Settings className="w-4 h-4 text-green-400" />
                      <span className="text-xs text-slate-400">Bot Settings</span>
                    </div>
                    <span className="text-xl font-bold text-white">
                      {previewData.has_settings ? 'Yes' : 'No'}
                    </span>
                  </div>
                  <div className="p-3 bg-slate-900/50 rounded-lg border border-slate-700">
                    <div className="flex items-center gap-2 mb-1">
                      <Heart className="w-4 h-4 text-red-400" />
                      <span className="text-xs text-slate-400">Heartbeat</span>
                    </div>
                    <span className="text-xl font-bold text-white">
                      {previewData.has_heartbeat ? 'Yes' : 'No'}
                    </span>
                  </div>
                  <div className="p-3 bg-slate-900/50 rounded-lg border border-slate-700">
                    <div className="flex items-center gap-2 mb-1">
                      <Sparkles className="w-4 h-4 text-yellow-400" />
                      <span className="text-xs text-slate-400">Soul Doc</span>
                    </div>
                    <span className="text-xl font-bold text-white">
                      {previewData.has_soul ? 'Yes' : 'No'}
                    </span>
                  </div>
                </div>

                {/* API Keys List */}
                {previewData.keys.length > 0 && (
                  <div>
                    <h4 className="text-sm font-medium text-slate-300 mb-2">API Keys</h4>
                    <div className="space-y-2 max-h-48 overflow-y-auto">
                      {previewData.keys.map((key) => (
                        <div
                          key={key.key_name}
                          className="flex items-center justify-between p-2 bg-slate-900/30 rounded border border-slate-700/50"
                        >
                          <div className="flex items-center gap-2">
                            <Key className="w-3 h-3 text-slate-500" />
                            <span className="text-sm text-slate-300">{key.key_name}</span>
                          </div>
                          <span className="text-xs font-mono text-slate-500">{key.key_preview}</span>
                        </div>
                      ))}
                    </div>
                  </div>
                )}

                {/* Backup Version */}
                {previewData.backup_version && (
                  <p className="text-xs text-slate-500 text-center">
                    Backup format version: {previewData.backup_version}
                  </p>
                )}
              </div>
            ) : (
              <div className="text-center py-12 text-slate-500">
                <Cloud className="w-12 h-12 mx-auto mb-3 opacity-50" />
                <p className="font-medium">No cloud backup found</p>
                <p className="text-sm mt-1">Create your first backup to protect your data</p>
              </div>
            )}
          </CardContent>
        </Card>
      </div>

      {/* What's Backed Up Info */}
      <Card className="mt-6 border-stark-500/30 bg-stark-500/5">
        <CardContent className="pt-6">
          <div className="flex items-start gap-4">
            <Shield className="w-6 h-6 text-stark-400 flex-shrink-0" />
            <div>
              <h4 className="font-medium text-white mb-3">What's included in your backup</h4>
              <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4 text-sm">
                <div className="flex items-start gap-2">
                  <Key className="w-4 h-4 text-stark-400 mt-0.5" />
                  <div>
                    <p className="text-slate-300 font-medium">API Keys</p>
                    <p className="text-slate-400 text-xs">All your configured service API keys</p>
                  </div>
                </div>
                <div className="flex items-start gap-2">
                  <Brain className="w-4 h-4 text-purple-400 mt-0.5" />
                  <div>
                    <p className="text-slate-300 font-medium">Mind Map</p>
                    <p className="text-slate-400 text-xs">All nodes and connections</p>
                  </div>
                </div>
                <div className="flex items-start gap-2">
                  <Clock className="w-4 h-4 text-orange-400 mt-0.5" />
                  <div>
                    <p className="text-slate-300 font-medium">Cron Jobs</p>
                    <p className="text-slate-400 text-xs">Scheduled tasks and automation</p>
                  </div>
                </div>
                <div className="flex items-start gap-2">
                  <MessageSquare className="w-4 h-4 text-indigo-400 mt-0.5" />
                  <div>
                    <p className="text-slate-300 font-medium">Channels</p>
                    <p className="text-slate-400 text-xs">Telegram, Discord, Slack channels with tokens</p>
                  </div>
                </div>
                <div className="flex items-start gap-2">
                  <Settings className="w-4 h-4 text-green-400 mt-0.5" />
                  <div>
                    <p className="text-slate-300 font-medium">Bot Settings</p>
                    <p className="text-slate-400 text-xs">Name, email, RPC settings, and preferences</p>
                  </div>
                </div>
                <div className="flex items-start gap-2">
                  <Heart className="w-4 h-4 text-red-400 mt-0.5" />
                  <div>
                    <p className="text-slate-300 font-medium">Heartbeat</p>
                    <p className="text-slate-400 text-xs">Heartbeat schedule and settings</p>
                  </div>
                </div>
                <div className="flex items-start gap-2">
                  <Sparkles className="w-4 h-4 text-yellow-400 mt-0.5" />
                  <div>
                    <p className="text-slate-300 font-medium">Soul Document</p>
                    <p className="text-slate-400 text-xs">Agent personality and core truths</p>
                  </div>
                </div>
                <div className="flex items-start gap-2">
                  <Shield className="w-4 h-4 text-blue-400 mt-0.5" />
                  <div>
                    <p className="text-slate-300 font-medium">x402 Payment</p>
                    <p className="text-slate-400 text-xs">Gasless payment via permit signature</p>
                  </div>
                </div>
              </div>
            </div>
          </div>
        </CardContent>
      </Card>

      {/* Backup Cost Confirmation Modal */}
      {confirmBackupModalOpen && (
        <div className="fixed inset-0 z-50 flex items-center justify-center p-4">
          {/* Backdrop */}
          <div
            className="absolute inset-0 bg-black/70 backdrop-blur-sm"
            onClick={() => setConfirmBackupModalOpen(false)}
          />
          {/* Modal */}
          <div className="relative bg-slate-800 border border-stark-500/50 rounded-xl shadow-2xl w-full max-w-md overflow-hidden">
            {/* Header */}
            <div className="flex items-center justify-between px-6 py-4 border-b border-slate-700">
              <div className="flex items-center gap-3">
                <div className="p-2 bg-stark-500/20 rounded-lg">
                  <Upload className="w-5 h-5 text-stark-400" />
                </div>
                <h2 className="text-lg font-semibold text-white">Confirm Backup</h2>
              </div>
              <button
                onClick={() => setConfirmBackupModalOpen(false)}
                className="text-slate-400 hover:text-white p-1"
              >
                <X className="w-5 h-5" />
              </button>
            </div>
            {/* Content */}
            <div className="p-6">
              <div className="text-center mb-6">
                <p className="text-slate-300 mb-4">
                  This action will cost:
                </p>
                <div className="inline-flex items-center gap-2 px-4 py-3 bg-stark-500/20 border border-stark-500/50 rounded-lg">
                  <span className="text-2xl font-bold text-stark-400">{BACKUP_COST_STARKBOT}</span>
                  <span className="text-lg text-stark-300">STARKBOT</span>
                </div>
              </div>
              <p className="text-sm text-slate-400 text-center">
                The payment will be processed using the x402 protocol from your burner wallet.
              </p>
            </div>
            {/* Footer */}
            <div className="flex justify-end gap-3 px-6 py-4 border-t border-slate-700 bg-slate-900/30">
              <Button
                variant="secondary"
                size="sm"
                onClick={() => setConfirmBackupModalOpen(false)}
              >
                Cancel
              </Button>
              <Button
                size="sm"
                onClick={confirmAndUploadBackup}
              >
                <Upload className="w-4 h-4 mr-2" />
                Continue
              </Button>
            </div>
          </div>
        </div>
      )}

      {/* Restore Confirmation Modal */}
      {confirmRestoreModalOpen && previewData && (
        <div className="fixed inset-0 z-50 flex items-center justify-center p-4">
          {/* Backdrop */}
          <div
            className="absolute inset-0 bg-black/70 backdrop-blur-sm"
            onClick={() => setConfirmRestoreModalOpen(false)}
          />
          {/* Modal */}
          <div className="relative bg-slate-800 border border-orange-500/50 rounded-xl shadow-2xl w-full max-w-lg overflow-hidden">
            {/* Header */}
            <div className="flex items-center justify-between px-6 py-4 border-b border-slate-700">
              <div className="flex items-center gap-3">
                <div className="p-2 bg-orange-500/20 rounded-lg">
                  <AlertTriangle className="w-5 h-5 text-orange-400" />
                </div>
                <h2 className="text-lg font-semibold text-white">Confirm Restore</h2>
              </div>
              <button
                onClick={() => setConfirmRestoreModalOpen(false)}
                className="text-slate-400 hover:text-white p-1"
              >
                <X className="w-5 h-5" />
              </button>
            </div>
            {/* Content */}
            <div className="p-6">
              <div className="mb-4 p-3 bg-orange-500/10 border border-orange-500/30 rounded-lg">
                <p className="text-sm text-orange-300">
                  <strong>Warning:</strong> Restoring from backup will overwrite your existing data.
                  This action cannot be undone.
                </p>
              </div>

              <p className="text-slate-300 mb-4">
                The following data will be restored:
              </p>

              {/* Preview Stats */}
              <div className="grid grid-cols-2 gap-2 mb-4">
                <div className="flex items-center gap-2 p-2 bg-slate-900/50 rounded-lg">
                  <Key className="w-4 h-4 text-stark-400" />
                  <span className="text-sm text-slate-300">{previewData.key_count} API Keys</span>
                </div>
                <div className="flex items-center gap-2 p-2 bg-slate-900/50 rounded-lg">
                  <Brain className="w-4 h-4 text-purple-400" />
                  <span className="text-sm text-slate-300">{previewData.node_count || 0} Mind Nodes</span>
                </div>
                <div className="flex items-center gap-2 p-2 bg-slate-900/50 rounded-lg">
                  <Link2 className="w-4 h-4 text-blue-400" />
                  <span className="text-sm text-slate-300">{previewData.connection_count || 0} Connections</span>
                </div>
                <div className="flex items-center gap-2 p-2 bg-slate-900/50 rounded-lg">
                  <Clock className="w-4 h-4 text-orange-400" />
                  <span className="text-sm text-slate-300">{previewData.cron_job_count || 0} Cron Jobs</span>
                </div>
                {(previewData.channel_count ?? 0) > 0 && (
                  <div className="flex items-center gap-2 p-2 bg-slate-900/50 rounded-lg">
                    <MessageSquare className="w-4 h-4 text-indigo-400" />
                    <span className="text-sm text-slate-300">{previewData.channel_count} Channels</span>
                  </div>
                )}
                {previewData.has_settings && (
                  <div className="flex items-center gap-2 p-2 bg-slate-900/50 rounded-lg">
                    <Settings className="w-4 h-4 text-green-400" />
                    <span className="text-sm text-slate-300">Bot Settings</span>
                  </div>
                )}
                {previewData.has_heartbeat && (
                  <div className="flex items-center gap-2 p-2 bg-slate-900/50 rounded-lg">
                    <Heart className="w-4 h-4 text-red-400" />
                    <span className="text-sm text-slate-300">Heartbeat Config</span>
                  </div>
                )}
                {previewData.has_soul && (
                  <div className="flex items-center gap-2 p-2 bg-slate-900/50 rounded-lg">
                    <Sparkles className="w-4 h-4 text-yellow-400" />
                    <span className="text-sm text-slate-300">Soul Document</span>
                  </div>
                )}
              </div>
            </div>
            {/* Footer */}
            <div className="flex justify-end gap-3 px-6 py-4 border-t border-slate-700 bg-slate-900/30">
              <Button
                variant="secondary"
                size="sm"
                onClick={() => setConfirmRestoreModalOpen(false)}
              >
                Cancel
              </Button>
              <Button
                size="sm"
                onClick={confirmAndRestoreBackup}
                className="bg-orange-500 hover:bg-orange-600"
              >
                <Download className="w-4 h-4 mr-2" />
                Restore Data
              </Button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
