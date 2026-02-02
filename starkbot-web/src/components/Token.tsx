import { Coins, ExternalLink, Copy, Check } from 'lucide-react'
import { useState } from 'react'

const CONTRACT_ADDRESS = '0x587Cd533F418825521f3A1daa7CCd1E7339A1B07'

export function Token() {
  const [copied, setCopied] = useState(false)

  const copyAddress = () => {
    navigator.clipboard.writeText(CONTRACT_ADDRESS)
    setCopied(true)
    setTimeout(() => setCopied(false), 2000)
  }

  return (
    <section className="py-8 px-6">
      <div className="max-w-5xl mx-auto">
        <div className="bg-gradient-to-br from-blue-500/10 to-purple-500/10 rounded-2xl border border-blue-500/20 px-6 py-4">
          <div className="flex flex-wrap items-center gap-4 sm:gap-6">
            {/* Token Icon */}
            <div className="flex-shrink-0">
              <div className="w-14 h-14 bg-gradient-to-br from-blue-500 to-blue-700 rounded-xl flex items-center justify-center shadow-lg shadow-blue-500/30">
                <Coins className="w-7 h-7 text-white" />
              </div>
            </div>

            {/* Token Name */}
            <div className="flex items-center gap-3">
              <h3 className="text-xl font-bold text-white">$STARKBOT</h3>
              <span className="px-2 py-0.5 bg-blue-500/20 text-blue-400 text-xs font-medium rounded-full">
                BASE
              </span>
            </div>

            {/* Action Buttons */}
            <div className="flex items-center gap-2 order-last w-full sm:order-none sm:w-auto">
              <a
                href="https://app.uniswap.org/swap?chain=base&outputCurrency=0x587Cd533F418825521f3A1daa7CCd1E7339A1B07"
                target="_blank"
                rel="noopener noreferrer"
                className="inline-flex items-center gap-1.5 px-4 py-2 bg-gradient-to-r from-blue-500 to-blue-600 hover:from-blue-400 hover:to-blue-500 text-white text-sm font-semibold rounded-lg transition-all duration-300 transform hover:scale-105 shadow-lg hover:shadow-blue-500/25"
              >
                Buy
                <ExternalLink className="w-3.5 h-3.5" />
              </a>
              <a
                href="https://www.geckoterminal.com/base/pools/0x0d64a8e0d28626511cc23fc75b81c2f03e222b14f9b944b60eecc3f4ddabeddc"
                target="_blank"
                rel="noopener noreferrer"
                className="inline-flex items-center gap-1.5 px-4 py-2 bg-white/5 hover:bg-white/10 text-white text-sm font-medium rounded-lg transition-all duration-300 border border-white/10 hover:border-white/20"
              >
                Chart
                <ExternalLink className="w-3.5 h-3.5" />
              </a>
              <a
                href="https://clanker.world/clanker/0x587Cd533F418825521f3A1daa7CCd1E7339A1B07"
                target="_blank"
                rel="noopener noreferrer"
                className="inline-flex items-center gap-1.5 px-4 py-2 bg-white/5 hover:bg-white/10 text-white text-sm font-medium rounded-lg transition-all duration-300 border border-white/10 hover:border-white/20"
              >
                Clanker
                <ExternalLink className="w-3.5 h-3.5" />
              </a>
            </div>

            {/* Contract Address - pushed to right */}
            <div className="flex items-center gap-2 ml-auto">
              <code className="text-sm text-white/50 font-mono bg-white/5 px-3 py-1.5 rounded-lg">
                {CONTRACT_ADDRESS.slice(0, 6)}...{CONTRACT_ADDRESS.slice(-4)}
              </code>
              <button
                onClick={copyAddress}
                className="p-1.5 text-white/50 hover:text-white bg-white/5 hover:bg-white/10 rounded-lg transition-colors"
                title="Copy address"
              >
                {copied ? <Check className="w-4 h-4 text-green-400" /> : <Copy className="w-4 h-4" />}
              </button>
            </div>
          </div>
        </div>
      </div>
    </section>
  )
}
