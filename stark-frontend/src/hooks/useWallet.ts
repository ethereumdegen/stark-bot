import { useState, useEffect, useCallback } from 'react';
import { JsonRpcProvider, Contract, formatUnits } from 'ethers';
import { getConfigStatus } from '@/lib/api';

// Supported networks configuration
type SupportedNetwork = 'mainnet' | 'base';

interface NetworkConfig {
  chainIdDecimal: number;
  name: string;
  displayName: string;
  usdcAddress: string;
  rpcUrl: string;
}

const SUPPORTED_NETWORKS: Record<SupportedNetwork, NetworkConfig> = {
  mainnet: {
    chainIdDecimal: 1,
    name: 'mainnet',
    displayName: 'Mainnet',
    usdcAddress: '0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48',
    rpcUrl: 'https://eth.llamarpc.com',
  },
  base: {
    chainIdDecimal: 8453,
    name: 'base',
    displayName: 'Base',
    usdcAddress: '0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913',
    rpcUrl: 'https://mainnet.base.org',
  },
};

// Fallback RPCs in case primary is down/rate-limited
const FALLBACK_RPCS: Record<SupportedNetwork, string[]> = {
  mainnet: ['https://rpc.ankr.com/eth', 'https://1rpc.io/eth'],
  base: ['https://base.llamarpc.com', 'https://rpc.ankr.com/base', 'https://1rpc.io/base'],
};

const USDC_DECIMALS = 6;

// Minimal ERC20 ABI for balance check
const ERC20_ABI = [
  'function balanceOf(address owner) view returns (uint256)',
];

interface WalletState {
  address: string | null;
  usdcBalance: string | null;
  isConnected: boolean;
  isLoading: boolean;
  error: string | null;
  currentNetwork: NetworkConfig;
  walletMode: string | null;
}

export function useWallet() {
  const [state, setState] = useState<WalletState>({
    address: null,
    usdcBalance: null,
    isConnected: false,
    isLoading: true,
    error: null,
    currentNetwork: SUPPORTED_NETWORKS.base,
    walletMode: null,
  });

  const [selectedNetwork, setSelectedNetwork] = useState<SupportedNetwork>('base');

  // Fetch USDC balance using direct RPC call with fallback RPCs
  const fetchUsdcBalance = useCallback(async (address: string, network: NetworkConfig): Promise<string | null> => {
    const rpcsToTry = [network.rpcUrl, ...(FALLBACK_RPCS[network.name as SupportedNetwork] || [])];

    for (const rpcUrl of rpcsToTry) {
      try {
        const provider = new JsonRpcProvider(rpcUrl);
        const usdcContract = new Contract(network.usdcAddress, ERC20_ABI, provider);
        const balance = await usdcContract.balanceOf(address);
        return formatUnits(balance, USDC_DECIMALS);
      } catch (err) {
        console.warn(`RPC failed (${rpcUrl}):`, err);
      }
    }

    console.error('All RPCs failed for USDC balance fetch');
    return null;
  }, []);

  // Fetch wallet address from backend
  const fetchWalletInfo = useCallback(async () => {
    setState(prev => ({ ...prev, isLoading: true, error: null }));

    try {
      const config = await getConfigStatus();

      if (!config.wallet_address) {
        setState(prev => ({
          ...prev,
          isLoading: false,
          isConnected: false,
          error: 'No wallet configured',
        }));
        return;
      }

      const network = SUPPORTED_NETWORKS[selectedNetwork];
      const usdcBalance = await fetchUsdcBalance(config.wallet_address, network);

      setState(prev => ({
        address: config.wallet_address,
        usdcBalance: usdcBalance ?? prev.usdcBalance,
        isConnected: true,
        isLoading: false,
        error: null,
        currentNetwork: network,
        walletMode: config.wallet_mode,
      }));
    } catch (err) {
      console.error('Failed to fetch wallet info:', err);
      setState(prev => ({
        ...prev,
        isLoading: false,
        error: err instanceof Error ? err.message : 'Failed to load wallet',
      }));
    }
  }, [selectedNetwork, fetchUsdcBalance]);

  // Switch network (just changes which RPC we query for balance)
  const switchNetwork = useCallback(async (networkName: SupportedNetwork) => {
    setSelectedNetwork(networkName);
    const network = SUPPORTED_NETWORKS[networkName];

    if (state.address) {
      const usdcBalance = await fetchUsdcBalance(state.address, network);
      setState(prev => ({
        ...prev,
        currentNetwork: network,
        usdcBalance,
      }));
    }

    return true;
  }, [state.address, fetchUsdcBalance]);

  const switchToBase = useCallback(async () => {
    return switchNetwork('base');
  }, [switchNetwork]);

  const refreshBalance = useCallback(async () => {
    if (!state.address) return;
    const usdcBalance = await fetchUsdcBalance(state.address, state.currentNetwork);
    setState(prev => ({ ...prev, usdcBalance }));
  }, [state.address, state.currentNetwork, fetchUsdcBalance]);

  // Fetch wallet info on mount
  useEffect(() => {
    fetchWalletInfo();
  }, [fetchWalletInfo]);

  // Auto-refresh USDC balance every 30 seconds
  useEffect(() => {
    if (!state.address || !state.isConnected) return;

    const intervalId = setInterval(() => {
      fetchUsdcBalance(state.address!, state.currentNetwork).then(usdcBalance => {
        if (usdcBalance !== null) {
          setState(prev => ({ ...prev, usdcBalance }));
        }
      });
    }, 30000); // 30 seconds

    return () => clearInterval(intervalId);
  }, [state.address, state.isConnected, state.currentNetwork, fetchUsdcBalance]);

  return {
    ...state,
    isCorrectNetwork: true, // Always correct since we use direct RPC
    connect: fetchWalletInfo, // Re-fetch wallet info
    refreshBalance,
    switchToBase,
    switchNetwork,
  };
}

// Export for use in UI components
export { SUPPORTED_NETWORKS };
export type { SupportedNetwork, NetworkConfig };
