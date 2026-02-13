import { useState, useEffect, useCallback } from 'react';
import { useNavigate } from 'react-router-dom';
import {
  Home,
  MessageSquare,
  Monitor,
  Settings,
  Bot,
  Wrench,
  Zap,
  Clock,
  Calendar,
  Brain,
  Users,
  FolderOpen,
  ScrollText,
  Bug,
  LogOut,
  Key,
  DollarSign,
  Shield,
  Sparkles,
  BookOpen,
  Wallet,
  Network,
  Heart,
  Cloud,
  Columns,
  Package,
  HardDrive,
} from 'lucide-react';
import HeartbeatIcon from '@/components/HeartbeatIcon';
import NavItem from './NavItem';
import { useAuth } from '@/hooks/useAuth';
import { getHeartbeatConfig } from '@/lib/api';

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
            <h1 className="text-2xl font-bold text-stark-400">StarkBot</h1>
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
        {/* Main Section */}
        <div className="space-y-1">
          <NavItem to="/dashboard" icon={Home} label="Dashboard" />
          <NavItem to="/agent-chat" icon={MessageSquare} label="Agent Chat" />
          <NavItem to="/agent-settings" icon={Settings} label="Agent Settings" />
          <NavItem to="/bot-settings" icon={Bot} label="Bot Settings" />
          <NavItem to="/crypto-transactions" icon={Wallet} label="Crypto Transactions" />
          <NavItem to="/tools" icon={Wrench} label="Tools" />
          <NavItem to="/skills" icon={Zap} label="Skills" />
        </div>

        {/* Configuration Section */}
        <div className="pt-4 mt-4 border-t border-slate-700 space-y-1">
          <p className="px-4 py-2 text-xs font-semibold text-slate-500 uppercase tracking-wider">
            Configuration
          </p>
          <NavItem to="/channels" icon={Monitor} label="Channels" />
          <NavItem to="/heartbeat" icon={Heart} label="Heartbeat" />
          <NavItem to="/scheduling" icon={Clock} label="Scheduling" />
          <NavItem to="/modules" icon={Package} label="Modules" />
          <NavItem to="/api-keys" icon={Key} label="API Keys" />
          <NavItem to="/cloud-backup" icon={Cloud} label="Cloud Backup" />
        </div>

        {/* Data Section */}
        <div className="pt-4 mt-4 border-t border-slate-700 space-y-1">
          <p className="px-4 py-2 text-xs font-semibold text-slate-500 uppercase tracking-wider">
            Data
          </p>
          <NavItem to="/sessions" icon={Calendar} label="Chat Sessions" />
          <NavItem to="/memories" icon={Brain} label="Memory Browser" />
          <NavItem to="/mindmap" icon={Network} label="Mind Map" />
          <NavItem to="/kanban" icon={Columns} label="Kanban Board" />
          <NavItem to="/identities" icon={Users} label="Identities" />
          <NavItem to="/files" icon={FolderOpen} label="Workspace Files" />
          <NavItem to="/system-files" icon={Sparkles} label="System Files" />
          <NavItem to="/journal" icon={BookOpen} label="Journal" />
        </div>

        {/* Developer Section */}
        <div className="pt-4 mt-4 border-t border-slate-700 space-y-1">
          <p className="px-4 py-2 text-xs font-semibold text-slate-500 uppercase tracking-wider">
            Developer
          </p>
          <NavItem to="/logs" icon={ScrollText} label="Live Logs" />
          <NavItem to="/system" icon={HardDrive} label="System" />
          <NavItem to="/debug" icon={Bug} label="Debug" />
          <NavItem to="/payments" icon={DollarSign} label="Payments" />
          <NavItem to="/eip8004" icon={Shield} label="EIP-8004" />
        </div>
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
