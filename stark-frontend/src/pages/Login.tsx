import { useState, useMemo, useEffect } from 'react';
import { useNavigate, useLocation } from 'react-router-dom';
import { BrowserProvider } from 'ethers';
import { generateChallenge, validateAuth, getConfigStatus, ConfigStatus } from '@/lib/api';
import Button from '@/components/ui/Button';
import Card, { CardContent } from '@/components/ui/Card';

type LoginState = 'idle' | 'connecting' | 'signing' | 'verifying' | 'flash';

// Detect if we're on mobile
function isMobile(): boolean {
  return /Android|webOS|iPhone|iPad|iPod|BlackBerry|IEMobile|Opera Mini/i.test(
    navigator.userAgent
  );
}

// Check if ethereum provider is available
function hasWalletProvider(): boolean {
  return typeof window.ethereum !== 'undefined';
}

// Generate deep links for wallet apps
function getWalletDeepLinks() {
  const currentUrl = window.location.href;
  const urlWithoutProtocol = currentUrl.replace(/^https?:\/\//, '');

  return {
    // Rainbow uses rnbwapp.com universal links to open the app
    rainbow: `https://rnbwapp.com/dapp?url=${encodeURIComponent(currentUrl)}`,
    metamask: `https://metamask.app.link/dapp/${urlWithoutProtocol}`,
    trust: `https://link.trustwallet.com/open_url?url=${encodeURIComponent(currentUrl)}`,
    coinbase: `https://go.cb-w.com/dapp?cb_url=${encodeURIComponent(currentUrl)}`,
  };
}

export default function Login() {
  const [error, setError] = useState('');
  const [state, setState] = useState<LoginState>('idle');
  const [connectedAddress, setConnectedAddress] = useState<string | null>(null);
  const [configStatus, setConfigStatus] = useState<ConfigStatus | null>(null);
  const navigate = useNavigate();
  const location = useLocation();

  // Handle flash login token from URL (e.g., /#/auth?token=xxx&flash=true)
  useEffect(() => {
    const params = new URLSearchParams(location.search);
    const token = params.get('token');
    const isFlash = params.get('flash') === 'true';

    if (token && isFlash) {
      setState('flash');
      // Store the token and redirect to dashboard
      localStorage.setItem('stark_token', token);
      navigate('/dashboard');
    }
  }, [location, navigate]);

  // Fetch config status on mount to show appropriate warnings
  useEffect(() => {
    getConfigStatus()
      .then(setConfigStatus)
      .catch((err) => console.error('Failed to fetch config status:', err));
  }, []);

  const showMobileWalletOptions = useMemo(() => {
    return isMobile() && !hasWalletProvider();
  }, []);

  const walletLinks = useMemo(() => getWalletDeepLinks(), []);

  const openInWallet = (wallet: keyof ReturnType<typeof getWalletDeepLinks>) => {
    window.location.href = walletLinks[wallet];
  };

  const getStateMessage = () => {
    switch (state) {
      case 'connecting':
        return 'Connecting to wallet...';
      case 'signing':
        return 'Please sign the message in your wallet...';
      case 'verifying':
        return 'Verifying signature...';
      case 'flash':
        return 'Logging in via Starkbot Cloud...';
      default:
        return '';
    }
  };

  const handleConnect = async () => {
    setError('');
    setState('connecting');

    try {
      // Check if MetaMask or compatible wallet is available
      if (!hasWalletProvider()) {
        throw new Error('Please install MetaMask or a compatible Ethereum wallet');
      }

      // Request account access
      const provider = new BrowserProvider(window.ethereum!);
      const accounts = await provider.send('eth_requestAccounts', []);

      if (!accounts || accounts.length === 0) {
        throw new Error('No accounts found. Please connect your wallet.');
      }

      const address = accounts[0].toLowerCase();
      setConnectedAddress(address);

      // Generate challenge from server
      const { challenge } = await generateChallenge(address);

      // Request signature
      setState('signing');
      const signer = await provider.getSigner();
      const signature = await signer.signMessage(challenge);

      // Verify with server
      setState('verifying');
      const result = await validateAuth(address, challenge, signature);

      // Store token and navigate
      localStorage.setItem('stark_token', result.token);
      navigate('/dashboard');
    } catch (err) {
      console.error('Login error:', err);
      if (err instanceof Error) {
        // Handle user rejection
        if (err.message.includes('user rejected') || err.message.includes('User denied')) {
          setError('Signature request was rejected');
        } else {
          setError(err.message);
        }
      } else {
        setError('Login failed');
      }
      setState('idle');
    }
  };

  const handleDisconnect = () => {
    setConnectedAddress(null);
    setError('');
    setState('idle');
  };

  const isLoading = state !== 'idle';

  return (
    <div className="min-h-screen flex items-center justify-center p-4">
      <div className="w-full max-w-md">
        <Card variant="elevated">
          <CardContent className="p-8">
            <div className="text-center mb-8">
              <h1 className="text-3xl font-bold text-stark-400 mb-2">StarkBot</h1>
              <p className="text-slate-400">Connect your wallet to continue</p>
            </div>

            <div className="space-y-6">
              {connectedAddress && !isLoading && (
                <div className="bg-slate-800/50 border border-slate-700 rounded-lg p-4">
                  <div className="text-sm text-slate-400 mb-1">Connected wallet</div>
                  <div className="font-mono text-sm text-slate-200 truncate">
                    {connectedAddress}
                  </div>
                  <button
                    onClick={handleDisconnect}
                    className="text-xs text-slate-500 hover:text-slate-300 mt-2"
                  >
                    Disconnect
                  </button>
                </div>
              )}

              {isLoading && (
                <div className="bg-stark-500/10 border border-stark-500/30 rounded-lg p-4 text-center">
                  <div className="flex items-center justify-center gap-2 text-stark-400">
                    <svg
                      className="animate-spin h-5 w-5"
                      xmlns="http://www.w3.org/2000/svg"
                      fill="none"
                      viewBox="0 0 24 24"
                    >
                      <circle
                        className="opacity-25"
                        cx="12"
                        cy="12"
                        r="10"
                        stroke="currentColor"
                        strokeWidth="4"
                      />
                      <path
                        className="opacity-75"
                        fill="currentColor"
                        d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z"
                      />
                    </svg>
                    <span>{getStateMessage()}</span>
                  </div>
                </div>
              )}

              {/* Configuration warnings */}
              {configStatus && !configStatus.login_configured && (
                <div className="bg-red-500/20 border border-red-500/50 text-red-400 px-4 py-3 rounded-lg text-sm">
                  {configStatus.wallet_mode === 'flash' ? (
                    <>
                      Login is not configured. Launch StarkBot via the provisioning plane at{' '}
                      <a href="https://starkbot.cloud" className="underline text-red-300 hover:text-red-200" target="_blank" rel="noopener noreferrer">starkbot.cloud</a>.
                    </>
                  ) : (
                    <>Login is not configured. Set <code className="bg-red-500/30 px-1 rounded">BURNER_WALLET_BOT_PRIVATE_KEY</code> environment variable and rebuild the instance.</>
                  )}
                </div>
              )}

              {configStatus && configStatus.login_configured && !configStatus.burner_wallet_configured && (
                <div className="bg-amber-500/20 border border-amber-500/50 text-amber-400 px-4 py-3 rounded-lg text-sm">
                  {configStatus.wallet_mode === 'flash' ? (
                    <>
                      Wallet not configured. Launch StarkBot via the provisioning plane at{' '}
                      <a href="https://starkbot.cloud" className="underline text-amber-300 hover:text-amber-200" target="_blank" rel="noopener noreferrer">starkbot.cloud</a>{' '}
                      to enable Web3 transaction tools.
                    </>
                  ) : (
                    <><code className="bg-amber-500/30 px-1 rounded">BURNER_WALLET_BOT_PRIVATE_KEY</code> is not configured. Web3 transaction tools will not work.</>
                  )}
                </div>
              )}

              {error && (
                <div className="bg-red-500/20 border border-red-500/50 text-red-400 px-4 py-3 rounded-lg text-sm">
                  {error}
                </div>
              )}

              {showMobileWalletOptions ? (
                <>
                  <p className="text-sm text-slate-400 text-center">
                    Open in your wallet app
                  </p>
                  <div className="space-y-3">
                    <button
                      onClick={() => openInWallet('rainbow')}
                      className="w-full flex items-center justify-center gap-3 px-4 py-3 bg-gradient-to-r from-blue-500 to-purple-500 hover:from-blue-600 hover:to-purple-600 text-white font-medium rounded-lg transition-all"
                    >
                      <span className="text-xl">🌈</span>
                      Rainbow
                    </button>
                    <button
                      onClick={() => openInWallet('metamask')}
                      className="w-full flex items-center justify-center gap-3 px-4 py-3 bg-orange-500 hover:bg-orange-600 text-white font-medium rounded-lg transition-colors"
                    >
                      <span className="text-xl">🦊</span>
                      MetaMask
                    </button>
                    <button
                      onClick={() => openInWallet('coinbase')}
                      className="w-full flex items-center justify-center gap-3 px-4 py-3 bg-blue-600 hover:bg-blue-700 text-white font-medium rounded-lg transition-colors"
                    >
                      <span className="text-xl">💰</span>
                      Coinbase Wallet
                    </button>
                    <button
                      onClick={() => openInWallet('trust')}
                      className="w-full flex items-center justify-center gap-3 px-4 py-3 bg-slate-700 hover:bg-slate-600 text-white font-medium rounded-lg transition-colors"
                    >
                      <span className="text-xl">🛡️</span>
                      Trust Wallet
                    </button>
                  </div>
                  <p className="text-xs text-slate-500 text-center">
                    This will open the app in your wallet's browser
                  </p>
                </>
              ) : (
                <>
                  <Button
                    onClick={handleConnect}
                    className="w-full"
                    size="lg"
                    disabled={isLoading}
                  >
                    {isLoading ? 'Connecting...' : 'Connect Wallet'}
                  </Button>

                  <p className="text-xs text-slate-500 text-center">
                    Sign in with your Ethereum wallet using SIWE (Sign In With Ethereum)
                  </p>

                  {configStatus?.guest_dashboard_enabled && (
                    <button
                      onClick={() => navigate('/guest_dashboard')}
                      className="w-full text-sm text-slate-400 hover:text-slate-200 py-2 transition-colors"
                    >
                      View Guest Dashboard
                    </button>
                  )}
                </>
              )}
            </div>
          </CardContent>
        </Card>
      </div>
    </div>
  );
}

// Extend Window interface for ethereum provider
declare global {
  interface Window {
    ethereum?: {
      request: (args: { method: string; params?: unknown[] }) => Promise<unknown>;
      isMetaMask?: boolean;
      on?: (event: string, handler: (...args: unknown[]) => void) => void;
      removeListener?: (event: string, handler: (...args: unknown[]) => void) => void;
    };
  }
}
