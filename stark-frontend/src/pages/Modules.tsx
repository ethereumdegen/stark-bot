import { useState, useEffect } from 'react';
import {
  Package,
  Check,
  Play,
  Pause,
  Wrench,
  RefreshCw,
  ExternalLink,
  Globe,
  Circle,
  Zap,
  Download,
  Star,
  FileText,
} from 'lucide-react';
import Card, { CardContent } from '@/components/ui/Card';
import Button from '@/components/ui/Button';
import Modal from '@/components/ui/Modal';
import { apiFetch } from '@/lib/api';

interface ModuleInfo {
  name: string;
  description: string;
  version: string;
  installed: boolean;
  enabled: boolean;
  has_tools: boolean;
  has_dashboard: boolean;
  has_skill: boolean;
  service_url: string;
  service_port: number;
  installed_at: string | null;
}

interface FeaturedModule {
  id: string;
  slug: string;
  name: string;
  description: string;
  version: string;
  author_address: string;
  author_username: string | null;
  tools_provided: string[];
  install_count: number;
  featured: boolean;
  license: string | null;
  x402_cost: string | null;
}

export default function Modules() {
  const [modules, setModules] = useState<ModuleInfo[]>([]);
  const [isLoading, setIsLoading] = useState(true);
  const [actionLoading, setActionLoading] = useState<string | null>(null);
  const [reloadLoading, setReloadLoading] = useState(false);
  const [message, setMessage] = useState<{ type: 'success' | 'error'; text: string } | null>(null);
  const [serviceHealth, setServiceHealth] = useState<Record<string, boolean>>({});
  const [featuredModules, setFeaturedModules] = useState<FeaturedModule[]>([]);
  const [featuredLoading, setFeaturedLoading] = useState(true);
  const [fetchingRemote, setFetchingRemote] = useState<string | null>(null);
  const [logsModule, setLogsModule] = useState<string | null>(null);
  const [logLines, setLogLines] = useState<string[]>([]);
  const [logsLoading, setLogsLoading] = useState(false);

  const openLogs = async (name: string) => {
    setLogsModule(name);
    setLogsLoading(true);
    setLogLines([]);
    try {
      const data = await apiFetch<{ module: string; lines: string[] }>(
        `/modules/${encodeURIComponent(name)}/logs`
      );
      setLogLines(data.lines || []);
    } catch {
      setLogLines(['(Failed to fetch logs)']);
    } finally {
      setLogsLoading(false);
    }
  };

  const refreshLogs = async () => {
    if (!logsModule) return;
    setLogsLoading(true);
    try {
      const data = await apiFetch<{ module: string; lines: string[] }>(
        `/modules/${encodeURIComponent(logsModule)}/logs`
      );
      setLogLines(data.lines || []);
    } catch {
      setLogLines(['(Failed to fetch logs)']);
    } finally {
      setLogsLoading(false);
    }
  };

  useEffect(() => {
    loadModules();
    loadFeatured();
  }, []);

  const loadModules = async () => {
    try {
      // Cache-bust to avoid stale browser-cached responses
      const data = await apiFetch<ModuleInfo[]>(`/modules?_t=${Date.now()}`);
      const list = Array.isArray(data) ? data : [];
      setModules(list);
      // Check health of each service (fire-and-forget, never crashes)
      checkServiceHealth(list).catch(() => {});
    } catch (err) {
      setMessage({ type: 'error', text: 'Failed to load modules' });
    } finally {
      setIsLoading(false);
    }
  };

  const checkServiceHealth = async (mods: ModuleInfo[]) => {
    const health: Record<string, boolean> = {};
    await Promise.all(
      mods.map(async (m) => {
        try {
          await apiFetch(`/modules/${encodeURIComponent(m.name)}/status`);
          health[m.name] = true;
        } catch {
          health[m.name] = false;
        }
      })
    );
    setServiceHealth(health);
  };

  const performAction = async (name: string, action: string) => {
    setActionLoading(`${name}-${action}`);
    setMessage(null);
    try {
      const result = await apiFetch<{ status: string; message: string; error?: string }>(
        `/modules/${encodeURIComponent(name)}`,
        { method: 'POST', body: JSON.stringify({ action }) }
      );
      setMessage({ type: 'success', text: result.message || `Module ${action}ed successfully` });

      // Optimistically update local state so the UI reflects the change immediately
      if (action === 'enable' || action === 'disable') {
        setModules((prev) =>
          prev.map((m) =>
            m.name === name
              ? { ...m, enabled: action === 'enable', installed: true }
              : m
          )
        );
      }

      // Also re-fetch from server for full sync
      await loadModules();
    } catch (err: any) {
      let errorMsg = err.message || `Failed to ${action} module`;
      try {
        const parsed = JSON.parse(errorMsg);
        errorMsg = parsed.error || errorMsg;
      } catch {}
      setMessage({ type: 'error', text: errorMsg });
    } finally {
      setActionLoading(null);
    }
  };

  const reloadModules = async () => {
    setReloadLoading(true);
    setMessage(null);
    try {
      const result = await apiFetch<{ status: string; message: string; activated: string[] }>(
        '/modules/reload',
        { method: 'POST' }
      );
      setMessage({ type: 'success', text: result.message || 'Modules reloaded' });
      await loadModules();
    } catch (err: any) {
      let errorMsg = err.message || 'Failed to reload modules';
      try {
        const parsed = JSON.parse(errorMsg);
        errorMsg = parsed.error || errorMsg;
      } catch {}
      setMessage({ type: 'error', text: errorMsg });
    } finally {
      setReloadLoading(false);
    }
  };

  const loadFeatured = async () => {
    setFeaturedLoading(true);
    try {
      const data = await apiFetch<FeaturedModule[]>('/modules/featured_remote');
      setFeaturedModules(Array.isArray(data) ? data : []);
    } catch {
      // Silently fail — featured section is optional
      setFeaturedModules([]);
    } finally {
      setFeaturedLoading(false);
    }
  };

  const fetchRemoteModule = async (mod: FeaturedModule) => {
    const username = mod.author_username;
    if (!username) {
      setMessage({ type: 'error', text: 'Module author has no username — cannot fetch.' });
      return;
    }
    setFetchingRemote(mod.slug);
    setMessage(null);
    try {
      const result = await apiFetch<{ status: string; message: string; module: string }>(
        '/modules/fetch_remote',
        { method: 'POST', body: JSON.stringify({ username, slug: mod.slug }) }
      );
      setMessage({ type: 'success', text: result.message || `Module '${mod.name}' installed!` });
      // Remove from featured list and reload installed modules
      setFeaturedModules((prev) => prev.filter((m) => m.slug !== mod.slug));
      await loadModules();
    } catch (err: any) {
      let errorMsg = err.message || 'Failed to fetch module';
      try {
        const parsed = JSON.parse(errorMsg);
        errorMsg = parsed.error || errorMsg;
      } catch {}
      setMessage({ type: 'error', text: errorMsg });
    } finally {
      setFetchingRemote(null);
    }
  };

  if (isLoading) {
    return (
      <div className="p-8 flex items-center justify-center min-h-[400px]">
        <div className="flex items-center gap-3">
          <div className="w-6 h-6 border-2 border-stark-500 border-t-transparent rounded-full animate-spin" />
          <span className="text-slate-400">Loading modules...</span>
        </div>
      </div>
    );
  }

  return (
    <div className="p-8">
      {/* Header */}
      <div className="mb-8 flex items-start justify-between">
        <div>
          <h1 className="text-2xl font-bold text-white mb-2">Modules</h1>
          <p className="text-slate-400">
            Standalone microservices that extend StarkBot. Each module runs as its own binary
            with a dedicated database, dashboard, and HTTP API.
          </p>
        </div>
        <Button
          size="sm"
          variant="secondary"
          disabled={reloadLoading || actionLoading !== null}
          onClick={reloadModules}
        >
          <RefreshCw className={`w-4 h-4 mr-1.5 ${reloadLoading ? 'animate-spin' : ''}`} />
          Reload Modules
        </Button>
      </div>

      {/* Messages */}
      {message && (
        <div
          className={`mb-6 px-4 py-3 rounded-lg ${
            message.type === 'success'
              ? 'bg-green-500/20 border border-green-500/50 text-green-400'
              : 'bg-red-500/20 border border-red-500/50 text-red-400'
          }`}
        >
          {message.text}
        </div>
      )}

      {/* Module Cards */}
      <div className="space-y-4">
        {modules.length === 0 ? (
          <Card>
            <CardContent>
              <p className="text-slate-400 text-center py-8">No modules available.</p>
            </CardContent>
          </Card>
        ) : (
          [...modules].sort((a, b) => a.name.localeCompare(b.name)).map((module) => {
            const isHealthy = serviceHealth[module.name] === true;
            const healthChecked = module.name in serviceHealth;

            return (
              <Card key={module.name} variant="elevated">
                <CardContent>
                  <div className="flex items-start justify-between gap-4 py-2">
                    {/* Left: Module info */}
                    <div className="flex-1 min-w-0">
                      <div className="flex items-center gap-3 mb-2">
                        <Package className="w-5 h-5 text-stark-400 flex-shrink-0" />
                        <h3 className="text-lg font-semibold text-white">
                          {formatModuleName(module.name)}
                        </h3>
                        <span className="text-xs text-slate-500 bg-slate-700 px-2 py-0.5 rounded">
                          v{module.version}
                        </span>
                        {module.enabled ? (
                          <span className="text-xs text-green-400 bg-green-500/20 px-2 py-0.5 rounded flex items-center gap-1">
                            <Check className="w-3 h-3" /> Active
                          </span>
                        ) : (
                          <span className="text-xs text-slate-400 bg-slate-700 px-2 py-0.5 rounded flex items-center gap-1">
                            <Pause className="w-3 h-3" /> Disabled
                          </span>
                        )}
                      </div>

                      <p className="text-slate-400 text-sm mb-3">{module.description}</p>

                      {/* Feature badges */}
                      <div className="flex flex-wrap gap-2 mb-3">
                        {module.has_tools && (
                          <span className="text-xs text-slate-300 bg-slate-700/50 px-2 py-1 rounded flex items-center gap-1">
                            <Wrench className="w-3 h-3" /> AI Tools
                          </span>
                        )}
                        {module.has_dashboard && (
                          <span className="text-xs text-slate-300 bg-slate-700/50 px-2 py-1 rounded flex items-center gap-1">
                            <Globe className="w-3 h-3" /> Dashboard
                          </span>
                        )}
                        {module.has_skill && (
                          <span className="text-xs text-purple-300 bg-purple-500/20 px-2 py-1 rounded flex items-center gap-1">
                            <Zap className="w-3 h-3" /> Skill
                          </span>
                        )}
                      </div>

                      {/* Service status */}
                      <div className="flex items-center gap-3 text-sm">
                        <div className="flex items-center gap-1.5">
                          <Circle
                            className={`w-2.5 h-2.5 ${
                              !healthChecked
                                ? 'text-slate-500'
                                : isHealthy
                                ? 'text-green-400 fill-green-400'
                                : 'text-red-400 fill-red-400'
                            }`}
                          />
                          <span className="text-slate-400">
                            {!healthChecked
                              ? 'Checking...'
                              : isHealthy
                              ? 'Service running'
                              : 'Service offline'}
                          </span>
                        </div>
                        <code className="text-xs text-slate-500 bg-slate-800 px-2 py-0.5 rounded">
                          :{module.service_port}
                        </code>
                      </div>

                      {module.installed_at && (
                        <p className="text-xs text-slate-500 mt-2">
                          Installed: {new Date(module.installed_at).toLocaleDateString()}
                        </p>
                      )}
                    </div>

                    {/* Right: Actions */}
                    <div className="flex flex-col gap-2 flex-shrink-0">
                      {module.has_dashboard && (
                        isHealthy ? (
                          <a
                            href={`/modules/${encodeURIComponent(module.name)}`}
                            className="inline-flex items-center gap-1.5 px-3 py-1.5 text-sm font-medium text-slate-300 bg-slate-700 hover:bg-slate-600 rounded-lg transition-colors"
                          >
                            <ExternalLink className="w-4 h-4" />
                            Dashboard
                          </a>
                        ) : (
                          <span className="inline-flex items-center gap-1.5 px-3 py-1.5 text-sm font-medium text-slate-500 bg-slate-800 rounded-lg cursor-not-allowed">
                            <ExternalLink className="w-4 h-4" />
                            Dashboard
                          </span>
                        )
                      )}
                      <Button
                        size="sm"
                        variant="secondary"
                        onClick={() => openLogs(module.name)}
                      >
                        <FileText className="w-4 h-4 mr-1" />
                        Logs
                      </Button>
                      {module.enabled ? (
                        <>
                          <Button
                            size="sm"
                            variant="secondary"
                            disabled={actionLoading !== null}
                            isLoading={actionLoading === `${module.name}-restart`}
                            onClick={() => performAction(module.name, 'restart')}
                          >
                            <RefreshCw className="w-4 h-4 mr-1" />
                            Restart
                          </Button>
                          <Button
                            size="sm"
                            variant="secondary"
                            disabled={actionLoading !== null}
                            isLoading={actionLoading === `${module.name}-disable`}
                            onClick={() => performAction(module.name, 'disable')}
                          >
                            <Pause className="w-4 h-4 mr-1" />
                            Disable
                          </Button>
                        </>
                      ) : (
                        <Button
                          size="sm"
                          variant="primary"
                          disabled={actionLoading !== null}
                          isLoading={actionLoading === `${module.name}-enable`}
                          onClick={() => performAction(module.name, 'enable')}
                        >
                          <Play className="w-4 h-4 mr-1" />
                          Enable
                        </Button>
                      )}
                    </div>
                  </div>
                </CardContent>
              </Card>
            );
          })
        )}
      </div>

      {/* Help text */}
      <div className="mt-8 p-4 bg-slate-800/50 rounded-lg border border-slate-700">
        <p className="text-sm text-slate-400">
          Each module runs as a standalone service with its own database and web dashboard.
          Click <strong className="text-slate-300">Dashboard</strong> to open the module's
          built-in UI. Use <strong className="text-slate-300">Reload Modules</strong> to
          re-sync all module tools. You can also manage modules via AI chat:
          <code className="text-stark-400 bg-slate-700 px-1.5 py-0.5 rounded mx-1">
            manage_modules(action="list")
          </code>
        </p>
      </div>

      {/* Featured Modules from StarkHub */}
      {!featuredLoading && featuredModules.length > 0 && (
        <div className="mt-10">
          <div className="flex items-center gap-2 mb-4">
            <Star className="w-5 h-5 text-yellow-400" />
            <h2 className="text-xl font-bold text-white">Featured Modules</h2>
            <span className="text-xs text-slate-500 bg-slate-700 px-2 py-0.5 rounded">
              from StarkHub
            </span>
          </div>
          <div className="space-y-3">
            {featuredModules.map((mod) => (
              <Card key={mod.id} variant="elevated">
                <CardContent>
                  <div className="flex items-center justify-between gap-4 py-2">
                    <div className="flex-1 min-w-0">
                      <div className="flex items-center gap-3 mb-1">
                        <Package className="w-4 h-4 text-stark-400 flex-shrink-0" />
                        <h3 className="text-base font-semibold text-white">{mod.name}</h3>
                        <span className="text-xs text-slate-500 bg-slate-700 px-2 py-0.5 rounded">
                          v{mod.version}
                        </span>
                        {(!mod.x402_cost || mod.x402_cost === '0') && (
                          <span className="text-xs text-green-400 bg-green-500/20 px-2 py-0.5 rounded">
                            Free
                          </span>
                        )}
                        {mod.license && (
                          <span className="text-xs text-slate-400 bg-slate-700 px-2 py-0.5 rounded">
                            {mod.license}
                          </span>
                        )}
                      </div>
                      <p className="text-sm text-slate-400 mb-2">{mod.description}</p>
                      <div className="flex gap-3 text-xs text-slate-500">
                        <span>
                          {mod.author_username ? `@${mod.author_username}` : mod.author_address.slice(0, 10)}
                        </span>
                        <span>{mod.install_count} installs</span>
                        {mod.tools_provided.length > 0 && (
                          <span className="flex items-center gap-1">
                            <Wrench className="w-3 h-3" />
                            {mod.tools_provided.length} tools
                          </span>
                        )}
                      </div>
                    </div>
                    <Button
                      size="sm"
                      variant="primary"
                      disabled={fetchingRemote !== null}
                      isLoading={fetchingRemote === mod.slug}
                      onClick={() => fetchRemoteModule(mod)}
                    >
                      <Download className="w-4 h-4 mr-1" />
                      Fetch
                    </Button>
                  </div>
                </CardContent>
              </Card>
            ))}
          </div>
        </div>
      )}

      {/* StarkHub link */}
      <div className="mt-8 text-center">
        <a
          href="https://hub.starkbot.ai/modules"
          target="_blank"
          rel="noopener noreferrer"
          className="inline-flex items-center gap-2 px-4 py-2 text-sm text-slate-400 hover:text-stark-400 bg-slate-800/50 hover:bg-slate-800 border border-slate-700 hover:border-stark-500/50 rounded-lg transition-colors"
        >
          <ExternalLink className="w-4 h-4" />
          Find more modules on StarkHub
        </a>
      </div>

      {/* Logs Modal */}
      <Modal
        isOpen={logsModule !== null}
        onClose={() => setLogsModule(null)}
        title={logsModule ? `${formatModuleName(logsModule)} — Service Logs` : ''}
        size="xl"
      >
        <div className="flex justify-end mb-3">
          <Button size="sm" variant="secondary" onClick={refreshLogs} disabled={logsLoading}>
            <RefreshCw className={`w-4 h-4 mr-1 ${logsLoading ? 'animate-spin' : ''}`} />
            Refresh
          </Button>
        </div>
        <div className="bg-slate-900 rounded-lg border border-slate-700 p-4 max-h-[60vh] overflow-auto">
          {logsLoading && logLines.length === 0 ? (
            <p className="text-slate-500 text-sm">Loading...</p>
          ) : logLines.length === 0 ? (
            <p className="text-slate-500 text-sm">No log output captured yet.</p>
          ) : (
            <pre className="text-xs text-slate-300 font-mono whitespace-pre-wrap break-words">
              {logLines.join('\n')}
            </pre>
          )}
        </div>
      </Modal>
    </div>
  );
}

function formatModuleName(name: string): string {
  return name
    .split('_')
    .map((w) => w.charAt(0).toUpperCase() + w.slice(1))
    .join(' ');
}
