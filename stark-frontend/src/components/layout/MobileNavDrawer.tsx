import { useEffect } from 'react';
import { Link, useLocation } from 'react-router-dom';
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
  ShieldCheck,
  Bug,
  LogOut,
  Key,
  DollarSign,
  Shield,
  Sparkles,
  BookOpen,
  X,
  Wallet,
  Network,
  Heart,
  Cloud,
  Columns,
} from 'lucide-react';
import clsx from 'clsx';
import { useAuth } from '@/hooks/useAuth';

interface MobileNavDrawerProps {
  isOpen: boolean;
  onClose: () => void;
}

interface DrawerNavItemProps {
  to: string;
  icon: React.ComponentType<{ className?: string }>;
  label: string;
  onClick: () => void;
}

function DrawerNavItem({ to, icon: Icon, label, onClick }: DrawerNavItemProps) {
  const location = useLocation();
  const isActive = location.pathname === to;

  return (
    <Link
      to={to}
      onClick={onClick}
      className={clsx(
        'flex items-center gap-3 px-4 py-3 rounded-lg font-medium transition-colors',
        isActive
          ? 'bg-stark-500/20 text-stark-400'
          : 'text-slate-400 hover:text-white hover:bg-slate-700/50'
      )}
    >
      <Icon className="w-5 h-5" />
      <span>{label}</span>
    </Link>
  );
}

export default function MobileNavDrawer({ isOpen, onClose }: MobileNavDrawerProps) {
  const { logout } = useAuth();

  // Close on escape key
  useEffect(() => {
    const handleEscape = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onClose();
    };

    if (isOpen) {
      document.addEventListener('keydown', handleEscape);
      document.body.style.overflow = 'hidden';
    }

    return () => {
      document.removeEventListener('keydown', handleEscape);
      document.body.style.overflow = '';
    };
  }, [isOpen, onClose]);

  const handleLogout = () => {
    onClose();
    logout();
  };

  const mainItems = [
    { to: '/dashboard', icon: Home, label: 'Dashboard' },
    { to: '/agent-chat', icon: MessageSquare, label: 'Agent Chat' },
    { to: '/workstream', icon: Columns, label: 'Workstream' },
    { to: '/heartbeat', icon: Heart, label: 'Heartbeat' },
    { to: '/impulse-map', icon: Network, label: 'Impulse Map' },
  ];

  const configItems = [
    { to: '/agent-settings', icon: Settings, label: 'Agent Settings' },
    { to: '/bot-settings', icon: Bot, label: 'Bot Settings' },
    { to: '/channels', icon: Monitor, label: 'Channels' },
    { to: '/scheduling', icon: Clock, label: 'Scheduling' },
    { to: '/api-keys', icon: Key, label: 'API Keys' },
    { to: '/cloud-backup', icon: Cloud, label: 'Cloud Backup' },
  ];

  const dataItems = [
    { to: '/sessions', icon: Calendar, label: 'Chat Sessions' },
    { to: '/memories', icon: Brain, label: 'Memory Browser' },
    { to: '/identities', icon: Users, label: 'Identities' },
    { to: '/files', icon: FolderOpen, label: 'Workspace Files' },
    { to: '/crypto-transactions', icon: Wallet, label: 'Crypto Transactions' },
    { to: '/system-files', icon: Sparkles, label: 'System Files' },
    { to: '/notes', icon: BookOpen, label: 'Notes' },
  ];

  const devItems = [
    { to: '/special-roles', icon: ShieldCheck, label: 'Special Roles' },
    { to: '/tools', icon: Wrench, label: 'Tools' },
    { to: '/skills', icon: Zap, label: 'Skills' },
    { to: '/logs', icon: ScrollText, label: 'Live Logs' },
    { to: '/debug', icon: Bug, label: 'Debug' },
    { to: '/payments', icon: DollarSign, label: 'Payments' },
    { to: '/eip8004', icon: Shield, label: 'EIP-8004' },
  ];

  return (
    <>
      {/* Backdrop */}
      <div
        className={clsx(
          'md:hidden fixed inset-0 bg-black/50 z-50 transition-opacity',
          isOpen ? 'opacity-100' : 'opacity-0 pointer-events-none'
        )}
        onClick={onClose}
      />

      {/* Drawer */}
      <div
        className={clsx(
          'md:hidden fixed bottom-0 left-0 right-0 bg-slate-800 rounded-t-2xl z-50 transform transition-transform duration-300 ease-out max-h-[80vh] overflow-hidden flex flex-col',
          isOpen ? 'translate-y-0' : 'translate-y-full'
        )}
      >
        {/* Handle */}
        <div className="flex justify-center pt-3 pb-2">
          <div className="w-10 h-1 bg-slate-600 rounded-full" />
        </div>

        {/* Header */}
        <div className="flex items-center justify-between px-4 pb-3 border-b border-slate-700">
          <h2 className="text-lg font-semibold text-white">Menu</h2>
          <button
            onClick={onClose}
            className="p-2 text-slate-400 hover:text-white transition-colors"
          >
            <X className="w-5 h-5" />
          </button>
        </div>

        {/* Navigation */}
        <nav className="flex-1 overflow-y-auto p-4 pb-8 space-y-1">
          {/* Main Section */}
          <div className="space-y-1">
            {mainItems.map((item) => (
              <DrawerNavItem
                key={item.to}
                to={item.to}
                icon={item.icon}
                label={item.label}
                onClick={onClose}
              />
            ))}
          </div>

          {/* Configuration Section */}
          <div className="pt-4 mt-4 border-t border-slate-700 space-y-1">
            <p className="px-4 py-2 text-xs font-semibold text-slate-500 uppercase tracking-wider">
              Configuration
            </p>
            {configItems.map((item) => (
              <DrawerNavItem
                key={item.to}
                to={item.to}
                icon={item.icon}
                label={item.label}
                onClick={onClose}
              />
            ))}
          </div>

          {/* Data Section */}
          <div className="pt-4 mt-4 border-t border-slate-700 space-y-1">
            <p className="px-4 py-2 text-xs font-semibold text-slate-500 uppercase tracking-wider">
              Data
            </p>
            {dataItems.map((item) => (
              <DrawerNavItem
                key={item.to}
                to={item.to}
                icon={item.icon}
                label={item.label}
                onClick={onClose}
              />
            ))}
          </div>

          {/* Developer Section */}
          <div className="pt-4 mt-4 border-t border-slate-700 space-y-1">
            <p className="px-4 py-2 text-xs font-semibold text-slate-500 uppercase tracking-wider">
              Developer
            </p>
            {devItems.map((item) => (
              <DrawerNavItem
                key={item.to}
                to={item.to}
                icon={item.icon}
                label={item.label}
                onClick={onClose}
              />
            ))}
          </div>

          {/* Logout */}
          <div className="pt-4 mt-4 border-t border-slate-700">
            <button
              onClick={handleLogout}
              className="w-full flex items-center gap-3 px-4 py-3 rounded-lg font-medium text-slate-400 hover:text-white hover:bg-slate-700/50 transition-colors"
            >
              <LogOut className="w-5 h-5" />
              <span>Logout</span>
            </button>
          </div>
        </nav>
      </div>
    </>
  );
}
