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
import Sessions from '@/pages/Sessions';
import MemoryBrowser from '@/pages/MemoryBrowser';
import Identities from '@/pages/Identities';
import FileBrowser from '@/pages/FileBrowser';
import SystemFiles from '@/pages/SystemFiles';
import Journal from '@/pages/Journal';
import Logs from '@/pages/Logs';
import Debug from '@/pages/Debug';
import ApiKeys from '@/pages/ApiKeys';
import Payments from '@/pages/Payments';
import EIP8004 from '@/pages/EIP8004';
import CryptoTransactions from '@/pages/CryptoTransactions';

function App() {
  return (
    <Routes>
      <Route path="/" element={<Login />} />
      <Route element={<Layout />}>
        <Route path="/dashboard" element={<Dashboard />} />
        <Route path="/agent-chat" element={<AgentChat />} />
        <Route path="/agent-settings" element={<AgentSettings />} />
        <Route path="/bot-settings" element={<BotSettings />} />
        <Route path="/channels" element={<Channels />} />
        <Route path="/tools" element={<Tools />} />
        <Route path="/skills" element={<Skills />} />
        <Route path="/scheduling" element={<Scheduling />} />
        <Route path="/api-keys" element={<ApiKeys />} />
        <Route path="/sessions" element={<Sessions />} />
        <Route path="/memories" element={<MemoryBrowser />} />
        <Route path="/identities" element={<Identities />} />
        <Route path="/files" element={<FileBrowser />} />
        <Route path="/system-files" element={<SystemFiles />} />
        <Route path="/journal" element={<Journal />} />
        <Route path="/logs" element={<Logs />} />
        <Route path="/debug" element={<Debug />} />
        <Route path="/payments" element={<Payments />} />
        <Route path="/eip8004" element={<EIP8004 />} />
        <Route path="/crypto-transactions" element={<CryptoTransactions />} />
      </Route>
    </Routes>
  );
}

export default App;
