import { Zap, Code2, Puzzle, Lock, MessageCircle, BarChart3, AlertTriangle } from 'lucide-react'

const features = [
  {
    icon: Zap,
    title: 'Lightning Fast',
    description: 'Powered by cutting-edge AI models, StarkBot responds instantly and handles complex tasks with ease.',
  },
  {
    icon: Code2,
    title: 'Open Source',
    description: "Fully open source and self-hostable. Own your data and customize to your heart's content.",
  },
  {
    icon: Puzzle,
    title: 'Extensible',
    description: 'Easy to integrate with your existing tools and workflows. Build custom plugins and automations.',
  },
  {
    icon: Lock,
    title: 'Privacy First',
    description: 'Your data stays yours. Self-host on your own infrastructure with full control over your information.',
  },
  {
    icon: MessageCircle,
    title: 'Natural Conversations',
    description: 'Talk naturally with StarkBot. It understands context and remembers your preferences.',
  },
  {
    icon: BarChart3,
    title: 'Analytics & Insights',
    description: 'Track your productivity and get insights into how StarkBot is helping you work smarter.',
  },
]

export function Features() {
  return (
    <section id="features" className="py-20 px-6">
      <div className="max-w-6xl mx-auto">
        {/* Warning Banner */}
        <div className="mb-12 p-6 bg-yellow-500/10 border border-yellow-500/50 rounded-xl">
          <div className="flex items-start gap-4">
            <div className="flex-shrink-0">
              <AlertTriangle className="w-8 h-8 text-yellow-500" />
            </div>
            <div>
              <h3 className="text-xl font-bold text-yellow-500 mb-2">WARNING</h3>
              <p className="text-slate-300 leading-relaxed">
                Starkbot is in active development and not production-ready software.
                Starkbot is not responsible for data loss or security intrusions.
                Always run Starkbot in a sandboxed VPS container.
                Feel free to contribute to development with a{' '}
                <a
                  href="https://github.com/ethereumdegen/stark-bot"
                  target="_blank"
                  rel="noopener noreferrer"
                  className="text-yellow-500 hover:text-yellow-400 underline"
                >
                  pull request
                </a>.
              </p>
            </div>
          </div>
        </div>

        <h2 className="text-3xl sm:text-4xl font-bold text-center mb-4">
          <span className="gradient-text">Powerful Features</span>
        </h2>
        <p className="text-slate-400 text-center mb-16 max-w-2xl mx-auto">
          Everything you need to supercharge your productivity with AI
        </p>

        <div className="grid grid-cols-1 md:grid-cols-3 gap-8">
          {features.map((feature) => (
            <div
              key={feature.title}
              className="p-8 bg-slate-900/50 backdrop-blur-sm rounded-2xl border border-slate-800 hover:border-stark-500/50 transition-all duration-300 card-glow"
            >
              <div className="w-14 h-14 bg-gradient-to-br from-stark-400 to-stark-600 rounded-xl flex items-center justify-center mb-6">
                <feature.icon className="w-7 h-7 text-white" />
              </div>
              <h3 className="text-xl font-bold mb-3">{feature.title}</h3>
              <p className="text-slate-400 leading-relaxed">{feature.description}</p>
            </div>
          ))}
        </div>
      </div>
    </section>
  )
}
