import { useState, useEffect } from 'react';
import { Wallet, Clock, CheckCircle, XCircle, ExternalLink, AlertCircle, Loader2 } from 'lucide-react';
import Card, { CardContent } from '@/components/ui/Card';
import Button from '@/components/ui/Button';
import { useApi } from '@/hooks/useApi';
import type { QueuedTransactionsResponse, QueuedTransactionInfo } from '@/lib/api';

type StatusFilter = 'all' | 'pending' | 'broadcast' | 'confirmed' | 'failed';

export default function CryptoTransactions() {
  const [filter, setFilter] = useState<StatusFilter>('all');
  const statusParam = filter === 'all' ? undefined : filter;

  const { data, isLoading, refetch } = useApi<QueuedTransactionsResponse>(
    `/tx-queue${statusParam ? `?status=${statusParam}` : ''}`
  );

  // Poll every 5 seconds
  useEffect(() => {
    const interval = setInterval(() => {
      refetch();
    }, 5000);
    return () => clearInterval(interval);
  }, [refetch]);

  const transactions = data?.transactions ?? [];
  const pendingCount = data?.pending_count ?? 0;
  const confirmedCount = data?.confirmed_count ?? 0;
  const failedCount = data?.failed_count ?? 0;

  const formatDate = (dateStr: string) => {
    try {
      return new Date(dateStr).toLocaleString();
    } catch {
      return dateStr;
    }
  };

  const shortenAddress = (addr: string) => {
    if (addr.length <= 12) return addr;
    return `${addr.slice(0, 6)}...${addr.slice(-4)}`;
  };

  const shortenUuid = (uuid: string) => {
    if (uuid.length <= 12) return uuid;
    return `${uuid.slice(0, 8)}...`;
  };

  const getStatusBadge = (status: QueuedTransactionInfo['status']) => {
    switch (status) {
      case 'pending':
        return (
          <span className="flex items-center gap-1 px-2 py-1 bg-amber-500/20 text-amber-400 rounded text-xs">
            <Clock className="w-3 h-3" /> Pending
          </span>
        );
      case 'broadcasting':
        return (
          <span className="flex items-center gap-1 px-2 py-1 bg-blue-500/20 text-blue-400 rounded text-xs">
            <Loader2 className="w-3 h-3 animate-spin" /> Broadcasting
          </span>
        );
      case 'broadcast':
        return (
          <span className="flex items-center gap-1 px-2 py-1 bg-blue-500/20 text-blue-400 rounded text-xs">
            <AlertCircle className="w-3 h-3" /> Broadcast
          </span>
        );
      case 'confirmed':
        return (
          <span className="flex items-center gap-1 px-2 py-1 bg-green-500/20 text-green-400 rounded text-xs">
            <CheckCircle className="w-3 h-3" /> Confirmed
          </span>
        );
      case 'failed':
        return (
          <span className="flex items-center gap-1 px-2 py-1 bg-red-500/20 text-red-400 rounded text-xs">
            <XCircle className="w-3 h-3" /> Failed
          </span>
        );
      case 'expired':
        return (
          <span className="flex items-center gap-1 px-2 py-1 bg-slate-500/20 text-slate-400 rounded text-xs">
            <Clock className="w-3 h-3" /> Expired
          </span>
        );
      default:
        return (
          <span className="px-2 py-1 bg-slate-500/20 text-slate-400 rounded text-xs">
            {status}
          </span>
        );
    }
  };

  const getNetworkBadge = (network: string) => {
    if (network === 'mainnet') {
      return (
        <span className="px-2 py-0.5 bg-purple-500/20 text-purple-400 rounded text-xs">
          Mainnet
        </span>
      );
    }
    return (
      <span className="px-2 py-0.5 bg-blue-500/20 text-blue-400 rounded text-xs">
        Base
      </span>
    );
  };

  const getExplorerUrl = (tx: QueuedTransactionInfo): string | undefined => {
    if (tx.explorer_url) return tx.explorer_url;
    if (tx.tx_hash) {
      const base = tx.network === 'mainnet' ? 'https://etherscan.io/tx' : 'https://basescan.org/tx';
      return `${base}/${tx.tx_hash}`;
    }
    return undefined;
  };

  return (
    <div className="p-8">
      <div className="mb-8">
        <h1 className="text-2xl font-bold text-white mb-2">Crypto Transactions</h1>
        <p className="text-slate-400">Track queued and broadcast blockchain transactions</p>
      </div>

      {/* Summary Cards */}
      <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-4 gap-6 mb-8">
        <Card>
          <CardContent>
            <div className="flex items-center gap-4">
              <div className="p-3 rounded-lg bg-amber-500/20">
                <Clock className="w-6 h-6 text-amber-400" />
              </div>
              <div>
                <p className="text-2xl font-bold text-white">{pendingCount}</p>
                <p className="text-sm text-slate-400">Pending</p>
              </div>
            </div>
          </CardContent>
        </Card>

        <Card>
          <CardContent>
            <div className="flex items-center gap-4">
              <div className="p-3 rounded-lg bg-green-500/20">
                <CheckCircle className="w-6 h-6 text-green-400" />
              </div>
              <div>
                <p className="text-2xl font-bold text-white">{confirmedCount}</p>
                <p className="text-sm text-slate-400">Confirmed</p>
              </div>
            </div>
          </CardContent>
        </Card>

        <Card>
          <CardContent>
            <div className="flex items-center gap-4">
              <div className="p-3 rounded-lg bg-red-500/20">
                <XCircle className="w-6 h-6 text-red-400" />
              </div>
              <div>
                <p className="text-2xl font-bold text-white">{failedCount}</p>
                <p className="text-sm text-slate-400">Failed</p>
              </div>
            </div>
          </CardContent>
        </Card>

        <Card>
          <CardContent>
            <div className="flex items-center gap-4">
              <div className="p-3 rounded-lg bg-stark-500/20">
                <Wallet className="w-6 h-6 text-stark-400" />
              </div>
              <div>
                <p className="text-2xl font-bold text-white">{transactions.length}</p>
                <p className="text-sm text-slate-400">Total Shown</p>
              </div>
            </div>
          </CardContent>
        </Card>
      </div>

      {/* Filter Tabs */}
      <div className="flex gap-2 mb-6">
        <Button
          variant={filter === 'all' ? 'primary' : 'secondary'}
          size="sm"
          onClick={() => setFilter('all')}
        >
          All
        </Button>
        <Button
          variant={filter === 'pending' ? 'primary' : 'secondary'}
          size="sm"
          onClick={() => setFilter('pending')}
        >
          Pending
        </Button>
        <Button
          variant={filter === 'broadcast' ? 'primary' : 'secondary'}
          size="sm"
          onClick={() => setFilter('broadcast')}
        >
          Broadcast
        </Button>
        <Button
          variant={filter === 'confirmed' ? 'primary' : 'secondary'}
          size="sm"
          onClick={() => setFilter('confirmed')}
        >
          Confirmed
        </Button>
        <Button
          variant={filter === 'failed' ? 'primary' : 'secondary'}
          size="sm"
          onClick={() => setFilter('failed')}
        >
          Failed
        </Button>
      </div>

      {/* Transactions Table */}
      <Card>
        <CardContent>
          {isLoading ? (
            <div className="text-center py-8 text-slate-400">Loading transactions...</div>
          ) : transactions.length === 0 ? (
            <div className="text-center py-8 text-slate-400">
              No transactions found. Transactions will appear here when you use web3_tx.
            </div>
          ) : (
            <div className="overflow-x-auto">
              <table className="w-full">
                <thead>
                  <tr className="border-b border-slate-700">
                    <th className="text-left py-3 px-4 text-slate-400 font-medium">Date</th>
                    <th className="text-left py-3 px-4 text-slate-400 font-medium">UUID</th>
                    <th className="text-left py-3 px-4 text-slate-400 font-medium">Network</th>
                    <th className="text-left py-3 px-4 text-slate-400 font-medium">To</th>
                    <th className="text-left py-3 px-4 text-slate-400 font-medium">Value</th>
                    <th className="text-left py-3 px-4 text-slate-400 font-medium">Status</th>
                    <th className="text-left py-3 px-4 text-slate-400 font-medium">TX Hash</th>
                  </tr>
                </thead>
                <tbody>
                  {transactions.map((tx) => (
                    <tr key={tx.uuid} className="border-b border-slate-700/50 hover:bg-slate-700/30">
                      <td className="py-3 px-4 text-slate-300 text-sm">
                        {formatDate(tx.created_at)}
                      </td>
                      <td className="py-3 px-4">
                        <span className="text-white font-mono text-sm" title={tx.uuid}>
                          {shortenUuid(tx.uuid)}
                        </span>
                      </td>
                      <td className="py-3 px-4">
                        {getNetworkBadge(tx.network)}
                      </td>
                      <td className="py-3 px-4">
                        <span className="text-slate-300 font-mono text-sm" title={tx.to}>
                          {shortenAddress(tx.to)}
                        </span>
                      </td>
                      <td className="py-3 px-4">
                        <span className="text-green-400 font-mono text-sm">
                          {tx.value_formatted}
                        </span>
                      </td>
                      <td className="py-3 px-4">
                        {getStatusBadge(tx.status)}
                        {tx.error && (
                          <span className="block text-xs text-red-400 mt-1 max-w-[150px] truncate" title={tx.error}>
                            {tx.error}
                          </span>
                        )}
                      </td>
                      <td className="py-3 px-4">
                        {tx.tx_hash ? (
                          <a
                            href={getExplorerUrl(tx)}
                            target="_blank"
                            rel="noopener noreferrer"
                            className="flex items-center gap-1 text-stark-400 hover:text-stark-300 font-mono text-sm"
                          >
                            {tx.tx_hash.slice(0, 10)}...{tx.tx_hash.slice(-6)}
                            <ExternalLink className="w-3 h-3" />
                          </a>
                        ) : (
                          <span className="text-slate-500">-</span>
                        )}
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          )}
        </CardContent>
      </Card>

      {/* Help Text */}
      <div className="mt-6 text-sm text-slate-500">
        <p>Transactions are queued when using <code className="bg-slate-700 px-1 rounded">web3_tx</code>.</p>
        <p>Use <code className="bg-slate-700 px-1 rounded">broadcast_web3_tx</code> to broadcast pending transactions.</p>
        <p>Use <code className="bg-slate-700 px-1 rounded">list_queued_web3_tx</code> to view transactions in chat.</p>
      </div>
    </div>
  );
}
