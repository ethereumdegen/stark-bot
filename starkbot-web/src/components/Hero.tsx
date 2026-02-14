import { Github, ChevronDown, Monitor, BookOpen } from 'lucide-react'
import { Link } from 'react-router-dom'

export function Hero() {
  return (
    <section className="pt-32 pb-20 px-6">
      <div className="max-w-4xl mx-auto text-center">
        {/* Mascot/Logo */}
        <div className="mb-8 animate-float">
          <div className="w-32 h-32 mx-auto bg-gradient-to-br from-blue-500 to-blue-700 rounded-3xl flex items-center justify-center glow transform rotate-3 hover:rotate-0 transition-transform duration-500">
            <div className="relative">
              <Monitor className="w-16 h-16 text-white" strokeWidth={1.5} />
              <div className="absolute top-6 left-4 flex gap-4">
                <div className="w-2 h-2 bg-white rounded-full" />
                <div className="w-2 h-2 bg-white rounded-full" />
              </div>
            </div>
          </div>
        </div>

        {/* Title */}
        <h1 className="text-5xl sm:text-7xl font-black mb-6 tracking-tight text-white">
          StarkBot
        </h1>

        {/* Tagline */}
        <p className="text-blue-400 text-xl sm:text-2xl font-semibold uppercase tracking-widest mb-2">
          Web3-Native AI Agent
        </p>

        {/* Badges */}
        <div className="flex flex-wrap gap-3 justify-center mb-6">
          <span className="px-3 py-1 bg-white/5 border border-white/10 rounded-full text-white/70 text-sm font-medium">
            Sign In With Ethereum
          </span>
          <span className="px-3 py-1 bg-white/5 border border-white/10 rounded-full text-white/70 text-sm font-medium">
            x402 Payments
          </span>
          <span className="px-3 py-1 bg-white/5 border border-white/10 rounded-full text-white/70 text-sm font-medium">
            DeFi Relay
          </span>
        </div>

        {/* Description */}
        <p className="text-white/50 text-lg sm:text-xl max-w-2xl mx-auto leading-relaxed mb-12">
          A crypto-native AI assistant with wallet authentication, x402 micropayments, and autonomous agent capabilities.
          Open source, self-hostable, and built for the decentralized web.
        </p>

        {/* CTA Buttons */}
        <div className="flex flex-col sm:flex-row gap-4 justify-center">
          <a
            href="https://github.com/ethereumdegen/stark-bot"
            target="_blank"
            rel="noopener noreferrer"
            className="px-8 py-4 bg-gradient-to-r from-blue-500 to-blue-600 hover:from-blue-400 hover:to-blue-500 text-white font-semibold rounded-xl transition-all duration-300 transform hover:scale-105 shadow-lg hover:shadow-blue-500/25 flex items-center justify-center gap-3"
          >
            <Github className="w-6 h-6" />
            View on GitHub
          </a>
           
          <Link
            to="/docs"
            className="px-8 py-4 bg-white/5 hover:bg-white/10 text-white font-semibold rounded-xl transition-all duration-300 border border-white/20 hover:border-blue-500/50 flex items-center justify-center gap-3"
          >
            <BookOpen className="w-6 h-6" />
            Read the Docs
          </Link>
          <a
            href="#features"
            className="px-8 py-4 bg-white/5 hover:bg-white/10 text-white font-semibold rounded-xl transition-all duration-300 border border-white/20 hover:border-white/30 flex items-center justify-center gap-2"
          >
            Learn More
            <ChevronDown className="w-5 h-5" />
          </a>
        </div>
      </div>
    </section>
  )
}
