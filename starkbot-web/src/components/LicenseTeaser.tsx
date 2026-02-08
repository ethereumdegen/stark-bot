import { Shield, ArrowRight } from 'lucide-react'
import { Link } from 'react-router-dom'

export function LicenseTeaser() {
  return (
    <section className="py-20 px-6 border-t border-white/10">
      <div className="max-w-4xl mx-auto">
        <div className="relative rounded-2xl border border-white/10 bg-white/[0.02] p-8 sm:p-12 overflow-hidden">
          {/* Subtle gradient accent */}
          <div className="absolute top-0 left-0 right-0 h-px bg-gradient-to-r from-transparent via-blue-500/50 to-transparent" />

          <div className="flex flex-col sm:flex-row items-start gap-6">
            <div className="flex-shrink-0 w-12 h-12 bg-gradient-to-br from-blue-500/20 to-purple-500/20 border border-blue-500/30 rounded-xl flex items-center justify-center">
              <Shield className="w-6 h-6 text-blue-400" />
            </div>

            <div className="flex-1">
              <h2 className="text-2xl sm:text-3xl font-bold text-white mb-3">
                The Stark License
              </h2>
              <p className="text-white/60 text-lg leading-relaxed mb-4">
                Every StarkBot agent is backed by an on-chain identity registered through{' '}
                <span className="text-blue-400">EIP-8004</span> on Base. The Stark License is a
                verifiable, permissionless credential that ties your agent to a wallet address,
                enabling trust, micropayments, and interoperability across the open web.
              </p>
              <p className="text-white/40 text-sm mb-6">
                Register your agent's identity on-chain and unlock the full capabilities of
                crypto-native AI.
              </p>

              <Link
                to="/starklicense"
                className="inline-flex items-center gap-2 px-6 py-3 bg-white/5 hover:bg-white/10 text-white font-medium rounded-xl transition-all duration-300 border border-white/20 hover:border-blue-500/50 group"
              >
                Learn about the Stark License
                <ArrowRight className="w-4 h-4 transition-transform group-hover:translate-x-1" />
              </Link>
            </div>
          </div>
        </div>
      </div>
    </section>
  )
}
