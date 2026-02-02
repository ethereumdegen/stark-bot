import { Code, CreditCard, Globe, AlertTriangle, Zap, MessageSquare, TrendingUp } from 'lucide-react'
import InteractiveDemo from './InteractiveDemo'

const features = [
  {
    icon: Globe,
    title: 'Web3 First',
    description: 'Built for the decentralized web. Native blockchain integrations, on-chain identity, and permissionless access.',
  },
  {
    icon: CreditCard,
    title: 'x402 Micropayments',
    description: 'Pay-per-use AI with the x402 protocol. Seamless crypto payments powered by DeFi Relay as the facilitator.',
  },
  {
    icon: Zap,
    title: 'Social Integration',
    description: 'Post to Twitter, Discord, Moltbook and other social platforms automatically',
  },
  {
    icon: MessageSquare,
    title: 'Multi-Platform',
    description: 'Deploy across multiple platforms with unified API and consistent experience',
  },
  {
    icon: TrendingUp,
    title: 'Market Intelligence',
    description: 'Real-time crypto prices, market analysis, and DeFi opportunities',
  },
  {
    icon: Code,
    title: 'Open Source',
    description: "Fully open source and self-hostable. Own your data and customize to your heart's content.",
  },
]

export function Features() {
  return (
    <section id="features" className="py-20 px-6">
      <div className="max-w-6xl mx-auto">
        {/* Chat Demo Section */}
        <div className="mb-16">
          <h2 className="text-3xl sm:text-4xl font-bold text-center mb-4 text-white">
            See It In Action
          </h2>
          <p className="text-white/50 text-center mb-8 max-w-2xl mx-auto">
            This is what StarkBot looks like when you run your own instance
          </p>
          <div className="chat-demo-glow rounded-xl">
            <ChatDemo />
          </div>
        </div>

        {/* Warning Banner */}
        <div className="mb-12 p-6 bg-white/5 border border-white/20 rounded-xl">
          <div className="flex items-start gap-4">
            <div className="flex-shrink-0">
              <AlertTriangle className="w-8 h-8 text-white/70" />
            </div>
            <div>
              <h3 className="text-xl font-bold text-white/90 mb-2">WARNING</h3>
              <p className="text-white/60 leading-relaxed">
                Starkbot is in active development and not production-ready software.
                Starkbot is not responsible for data loss or security intrusions.
                Always run Starkbot in a sandboxed VPS container.
                Feel free to contribute to development with a{' '}
                <a
                  href="https://github.com/ethereumdegen/stark-bot"
                  target="_blank"
                  rel="noopener noreferrer"
                  className="text-white hover:text-white/80 underline"
                >
                  pull request
                </a>.
              </p>
            </div>
          </div>
        </div>

        <h2 className="text-3xl sm:text-4xl font-bold text-center mb-4 text-white">
          Web3-Native Features
        </h2>
        <p className="text-white/50 text-center mb-16 max-w-2xl mx-auto">
          Crypto-first AI infrastructure with wallet auth, micropayments, and autonomous agents
        </p>

        <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-8">
          {features.map((feature) => (
            <div
              key={feature.title}
              className="p-8 bg-white/5 backdrop-blur-sm rounded-2xl border border-white/10 hover:border-white/20 transition-all duration-300 card-glow"
            >
              <div className="w-14 h-14 bg-white/10 rounded-xl flex items-center justify-center mb-6">
                <feature.icon className="w-7 h-7 text-white" />
              </div>
              <h3 className="text-xl font-bold mb-3 text-white">{feature.title}</h3>
              <p className="text-white/50 leading-relaxed">{feature.description}</p>
            </div>
          ))}
        </div>
      </div>
    </section>
  )
}
