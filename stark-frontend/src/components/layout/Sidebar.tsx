import { useState, useEffect, useCallback } from 'react';
import { useNavigate } from 'react-router-dom';
import { LogOut } from 'lucide-react';
import HeartbeatIcon from '@/components/HeartbeatIcon';
import NavItem from './NavItem';
import { useAuth } from '@/hooks/useAuth';
import { getHeartbeatConfig } from '@/lib/api';
import navigation from '@/config/navigation.json';
import iconMap from '@/config/iconMap';

export default function Sidebar() {
  const { logout } = useAuth();
  const navigate = useNavigate();
  const [version, setVersion] = useState<string | null>(null);
  const [heartbeatEnabled, setHeartbeatEnabled] = useState(false);

  const loadHeartbeatConfig = useCallback(async () => {
    try {
      const config = await getHeartbeatConfig();
      if (config) {
        setHeartbeatEnabled(config.enabled);
      }
    } catch (e) {
      console.error('Failed to load heartbeat config:', e);
    }
  }, []);

  useEffect(() => {
    fetch('/api/version')
      .then(res => {
        if (!res.ok) throw new Error(`HTTP ${res.status}`);
        return res.json();
      })
      .then(data => setVersion(data.version))
      .catch(err => {
        console.warn('Failed to fetch version:', err);
        setVersion(null);
      });

    loadHeartbeatConfig();
  }, [loadHeartbeatConfig]);

  return (
    <aside className="hidden md:flex w-64 h-screen sticky top-0 bg-slate-800 flex-col border-r border-slate-700">
      {/* Header */}
      <div className="p-6 border-b border-slate-700">
        <div className="flex items-center justify-between">
          <div>
            <h1 className="text-2xl text-stark-400" style={{ fontFamily: "'Orbitron', sans-serif" }}>StarkBot</h1>
            {version && (
              <span className="text-xs text-slate-500">v{version}</span>
            )}
          </div>
          <button
            onClick={() => navigate('/heartbeat')}
            className="group cursor-pointer"
            title="Configure heartbeat"
          >
            <HeartbeatIcon enabled={heartbeatEnabled} size={16} />
          </button>
        </div>
      </div>

      {/* Navigation */}
      <nav className="flex-1 p-4 space-y-1 overflow-y-auto">
        {navigation.sections.map((section, idx) => (
          <div
            key={section.label ?? '__main'}
            className={idx > 0 ? 'pt-4 mt-4 border-t border-slate-700 space-y-1' : 'space-y-1'}
          >
            {section.label && (
              <p className="px-4 py-2 text-xs font-semibold text-slate-500 uppercase tracking-wider">
                {section.label}
              </p>
            )}
            {section.items.map((item) => {
              const Icon = iconMap[item.icon];
              return Icon ? (
                <NavItem key={item.to} to={item.to} icon={Icon} label={item.label} />
              ) : null;
            })}
          </div>
        ))}
      </nav>

      {/* Footer */}
      <div className="p-4 border-t border-slate-700">
        <button
          onClick={logout}
          className="w-full flex items-center gap-3 px-4 py-3 rounded-lg font-medium text-slate-400 hover:text-white hover:bg-slate-700/50 transition-colors"
        >
          <LogOut className="w-5 h-5" />
          <span>Logout</span>
        </button>
      </div>
    </aside>
  );
}
