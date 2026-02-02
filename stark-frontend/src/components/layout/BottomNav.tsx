import { useState } from 'react';
import { Link, useLocation } from 'react-router-dom';
import { Home, MessageSquare, Monitor, Settings, Menu } from 'lucide-react';
import clsx from 'clsx';
import MobileNavDrawer from './MobileNavDrawer';

interface BottomNavItemProps {
  to: string;
  icon: React.ComponentType<{ className?: string }>;
  label: string;
  isActive: boolean;
}

function BottomNavItem({ to, icon: Icon, label, isActive }: BottomNavItemProps) {
  return (
    <Link
      to={to}
      className={clsx(
        'flex flex-col items-center justify-center flex-1 py-2 transition-colors',
        isActive ? 'text-stark-400' : 'text-slate-400'
      )}
    >
      <Icon className="w-5 h-5" />
      <span className="text-xs mt-1">{label}</span>
    </Link>
  );
}

interface MoreButtonProps {
  onClick: () => void;
  isOpen: boolean;
}

function MoreButton({ onClick, isOpen }: MoreButtonProps) {
  return (
    <button
      onClick={onClick}
      className={clsx(
        'flex flex-col items-center justify-center flex-1 py-2 transition-colors',
        isOpen ? 'text-stark-400' : 'text-slate-400'
      )}
    >
      <Menu className="w-5 h-5" />
      <span className="text-xs mt-1">More</span>
    </button>
  );
}

export default function BottomNav() {
  const location = useLocation();
  const [isDrawerOpen, setIsDrawerOpen] = useState(false);

  const navItems = [
    { to: '/dashboard', icon: Home, label: 'Home' },
    { to: '/agent-chat', icon: MessageSquare, label: 'Chat' },
    { to: '/channels', icon: Monitor, label: 'Channels' },
    { to: '/agent-settings', icon: Settings, label: 'Settings' },
  ];

  return (
    <>
      <nav className="md:hidden fixed bottom-0 left-0 right-0 bg-slate-800 border-t border-slate-700 flex z-50">
        {navItems.map((item) => (
          <BottomNavItem
            key={item.to}
            to={item.to}
            icon={item.icon}
            label={item.label}
            isActive={location.pathname === item.to}
          />
        ))}
        <MoreButton onClick={() => setIsDrawerOpen(true)} isOpen={isDrawerOpen} />
      </nav>

      <MobileNavDrawer
        isOpen={isDrawerOpen}
        onClose={() => setIsDrawerOpen(false)}
      />
    </>
  );
}
