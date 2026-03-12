import { useState, useEffect, useCallback } from 'react';
import { Routes, Route } from 'react-router-dom';
import Layout from '@/components/layout/Layout';
import Login from '@/pages/Login';
import Setup from '@/pages/Setup';
import Dashboard from '@/pages/Dashboard';
import StarflaskAgents from '@/pages/StarflaskAgents';
import CommandCenter from '@/pages/CommandCenter';
import BotSettings from '@/pages/BotSettings';
import ApiKeys from '@/pages/ApiKeys';
import CloudBackup from '@/pages/CloudBackup';
import CryptoTransactions from '@/pages/CryptoTransactions';
import System from '@/pages/System';
import GuestDashboard from '@/pages/GuestDashboard';

type SetupState = 'loading' | 'needs_key' | 'needs_agents' | 'ready';

function App() {
  const [setupState, setSetupState] = useState<SetupState>('loading');

  const checkSetup = useCallback(() => {
    const token = localStorage.getItem('stark_token');
    if (!token) {
      setSetupState('ready');
      return;
    }

    fetch('/api/health/config')
      .then(r => r.json())
      .then(data => {
        const hasKey = data.starflask_api_key_set === true || data.starflask_configured === true;
        const hasAgents = (data.starflask_agents_provisioned ?? 0) > 0;

        if (!hasKey) {
          setSetupState('needs_key');
        } else if (!hasAgents) {
          setSetupState('needs_agents');
        } else {
          setSetupState('ready');
        }
      })
      .catch(() => setSetupState('ready'));
  }, []);

  useEffect(() => { checkSetup(); }, [checkSetup]);

  if (setupState === 'loading') {
    return (
      <div className="min-h-screen bg-slate-900 flex items-center justify-center">
        <div className="text-slate-500">Loading...</div>
      </div>
    );
  }

  if (setupState === 'needs_key' || setupState === 'needs_agents') {
    return (
      <Setup
        initialStep={setupState === 'needs_agents' ? 'deploy' : 'api_key'}
        onComplete={() => { setSetupState('ready'); window.location.href = '/dashboard'; }}
      />
    );
  }

  return (
    <Routes>
      <Route path="/" element={<Login />} />
      <Route path="/auth" element={<Login />} />
      <Route path="/guest_dashboard" element={<GuestDashboard />} />
      <Route element={<Layout />}>
        <Route path="/dashboard" element={<Dashboard />} />
        <Route path="/command-center" element={<CommandCenter />} />
        <Route path="/starflask-agents" element={<StarflaskAgents />} />
        <Route path="/bot-settings" element={<BotSettings />} />
        <Route path="/api-keys" element={<ApiKeys />} />
        <Route path="/cloud-backup" element={<CloudBackup />} />
        <Route path="/crypto-transactions" element={<CryptoTransactions />} />
        <Route path="/system" element={<System />} />
      </Route>
    </Routes>
  );
}

export default App;
