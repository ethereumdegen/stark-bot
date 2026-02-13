import { Routes, Route } from 'react-router-dom';
import Layout from '@/components/layout/Layout';
import Login from '@/pages/Login';
import Dashboard from '@/pages/Dashboard';
import AgentChat from '@/pages/AgentChat';
import AgentSettings from '@/pages/AgentSettings';
import BotSettings from '@/pages/BotSettings';
import Channels from '@/pages/Channels';
import Tools from '@/pages/Tools';
import Skills from '@/pages/Skills';
import Scheduling from '@/pages/Scheduling';
import Heartbeat from '@/pages/Heartbeat';
import Sessions from '@/pages/Sessions';
import MemoryBrowser from '@/pages/MemoryBrowser';
import Identities from '@/pages/Identities';
import IdentityDetail from '@/pages/IdentityDetail';
import FileBrowser from '@/pages/FileBrowser';
import SystemFiles from '@/pages/SystemFiles';
import Journal from '@/pages/Journal';
import Logs from '@/pages/Logs';
import Debug from '@/pages/Debug';
import System from '@/pages/System';
import ApiKeys from '@/pages/ApiKeys';
import CloudBackup from '@/pages/CloudBackup';
import Payments from '@/pages/Payments';
import EIP8004 from '@/pages/EIP8004';
import CryptoTransactions from '@/pages/CryptoTransactions';
import MindMap from '@/pages/MindMap';
import KanbanBoard from '@/pages/KanbanBoard';
import Modules from '@/pages/Modules';
import ModuleDashboard from '@/pages/ModuleDashboard';
import GuestDashboard from '@/pages/GuestDashboard';

function App() {
  return (
    <Routes>
      <Route path="/" element={<Login />} />
      <Route path="/auth" element={<Login />} />
      <Route path="/guest_dashboard" element={<GuestDashboard />} />
      <Route element={<Layout />}>
        <Route path="/dashboard" element={<Dashboard />} />
        <Route path="/agent-chat" element={<AgentChat />} />
        <Route path="/agent-settings" element={<AgentSettings />} />
        <Route path="/bot-settings" element={<BotSettings />} />
        <Route path="/channels" element={<Channels />} />
        <Route path="/tools" element={<Tools />} />
        <Route path="/skills" element={<Skills />} />
        <Route path="/heartbeat" element={<Heartbeat />} />
        <Route path="/scheduling" element={<Scheduling />} />
        <Route path="/api-keys" element={<ApiKeys />} />
        <Route path="/cloud-backup" element={<CloudBackup />} />
        <Route path="/sessions" element={<Sessions />} />
        <Route path="/sessions/:sessionId" element={<Sessions />} />
        <Route path="/memories" element={<MemoryBrowser />} />
        <Route path="/mindmap" element={<MindMap />} />
        <Route path="/kanban" element={<KanbanBoard />} />
        <Route path="/identities" element={<Identities />} />
        <Route path="/identities/:identityId" element={<IdentityDetail />} />
        <Route path="/files" element={<FileBrowser />} />
        <Route path="/system-files" element={<SystemFiles />} />
        <Route path="/journal" element={<Journal />} />
        <Route path="/logs" element={<Logs />} />
        <Route path="/system" element={<System />} />
        <Route path="/debug" element={<Debug />} />
        <Route path="/payments" element={<Payments />} />
        <Route path="/eip8004" element={<EIP8004 />} />
        <Route path="/crypto-transactions" element={<CryptoTransactions />} />
        <Route path="/modules" element={<Modules />} />
        <Route path="/modules/:name" element={<ModuleDashboard />} />
      </Route>
    </Routes>
  );
}

export default App;
