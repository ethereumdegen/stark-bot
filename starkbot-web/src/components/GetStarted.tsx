import { Cloud } from 'lucide-react'

const steps = [
  {
    number: 1,
    title: 'Clone the Repo',
    description: 'Get the source code from GitHub',
  },
  {
    number: 2,
    title: 'Configure',
    description: 'Set up your env vars and x402 facilitator',
  },
  {
    number: 3,
    title: 'Run with Docker',
    description: 'Deploy and connect your wallet',
  },
]

export function GetStarted() {
  return (
    <section className="py-20 px-6 bg-white/[0.02]">
      <div className="max-w-5xl mx-auto text-center">
        <h2 className="text-3xl sm:text-4xl font-bold mb-4 text-white">
          Get Started in Minutes
        </h2>
        <p className="text-white/50 mb-16 max-w-2xl mx-auto">
          Self-host your Web3-native AI with Docker or deploy to the cloud
        </p>

        <div className="grid grid-cols-1 md:grid-cols-3 gap-8 mb-16">
          {steps.map((step) => (
            <div key={step.number}>
              <div className="w-12 h-12 border-2 border-white/20 rounded-full flex items-center justify-center text-xl font-bold mx-auto mb-4 text-white">
                {step.number}
              </div>
              <h3 className="text-xl font-bold mb-2 text-white">{step.title}</h3>
              <p className="text-white/50">{step.description}</p>
            </div>
          ))}
        </div>

        {/* Docker Code Block */}
        <div className="bg-black/50 rounded-xl border border-white/10 p-6 text-left max-w-2xl mx-auto">
          <div className="flex items-center gap-2 mb-4">
            <div className="w-3 h-3 bg-white/20 rounded-full" />
            <div className="w-3 h-3 bg-white/20 rounded-full" />
            <div className="w-3 h-3 bg-white/20 rounded-full" />
            <span className="ml-2 text-white/30 text-sm">terminal</span>
          </div>
          <code className="text-sm text-white font-mono block space-y-1">
            <div><span className="text-white/40">$</span> git clone https://github.com/ethereumdegen/stark-bot</div>
            <div><span className="text-white/40">$</span> cd stark-bot</div>
            <div><span className="text-white/40">$</span> cp .env.example .env</div>
            <div><span className="text-white/40">$</span> docker compose up -d</div>
          </code>
        </div>

        {/* Cloud Deploy Section */}
        <div className="mt-20">
          <h3 className="text-2xl sm:text-3xl font-bold mb-4 text-white">
            Deploy to the Cloud
          </h3>
          <p className="text-white/50 mb-8 max-w-2xl mx-auto">
            Deploy StarkBot to your favorite cloud platform for a fully managed experience
          </p>

          <div className="bg-white/5 backdrop-blur-sm rounded-2xl border border-white/10 p-8 max-w-2xl mx-auto">
            <div className="flex items-center justify-center gap-3 mb-6">
              <Cloud className="w-8 h-8 text-white/70" />
              <span className="text-xl font-bold text-white">One-Click Deploy</span>
            </div>

            <ol className="text-left text-white/60 space-y-4 mb-8">
              <li className="flex gap-3">
                <span className="text-white/40 font-bold">1.</span>
                <span>Click a deploy button below to start</span>
              </li>
              <li className="flex gap-3">
                <span className="text-white/40 font-bold">2.</span>
                <span>Connect your GitHub account when prompted</span>
              </li>
              <li className="flex gap-3">
                <span className="text-white/40 font-bold">3.</span>
                <span>Configure environment variables (API keys, DeFi Relay x402 facilitator)</span>
              </li>
              <li className="flex gap-3">
                <span className="text-white/40 font-bold">4.</span>
                <span>Deploy! The platform auto-detects the Dockerfile</span>
              </li>
            </ol>

            <div className="flex flex-col sm:flex-row items-center justify-center gap-4">
              <a
                href="https://starkbot.cloud"
                target="_blank"
                rel="noopener noreferrer"
                className="inline-flex items-center gap-2 px-6 py-3 bg-[#0080FF] hover:bg-[#0069d9] text-white font-semibold rounded-lg transition-all duration-300"
              >
                <Cloud className="w-5 h-5" />
                Starkbot Cloud
              </a>
              <a
                href="https://railway.com/deploy/tQTOx4?referralCode=CnqMxN&utm_medium=integration&utm_source=template&utm_campaign=generic"
                target="_blank"
                rel="noopener noreferrer"
                className="inline-flex items-center gap-2 px-6 py-3 bg-[#0B0D0E] hover:bg-[#1a1d1f] border border-white/20 text-white font-semibold rounded-lg transition-all duration-300"
              >
                <svg className="w-5 h-5" viewBox="0 0 24 24" fill="currentColor">
                  <path d="M.113 10.27A13.3 13.3 0 000 11.2h4.94L.114 10.27zm.12-.347l5.727.937.59-.098L.114 4.704a13.27 13.27 0 00.118 5.219zm1.04-6.85l7.138 7.139.589-.098-3.91-9.437a13.27 13.27 0 00-3.816 2.397zm5.2-3.266l4.628 11.18.59-.098L8.71.133a13.4 13.4 0 00-2.237-.326zM11.14.01l3.394 12.247h.589L14.63 0c-1.16-.04-2.34.003-3.49.01zm4.728-.01l-.358 12.247.59.098L19.89.49a13.27 13.27 0 00-4.02-.49zm5.21 1.194l-4.076 11.592.59.098L23.29 3.27a13.27 13.27 0 00-2.21-2.076zm2.89 3.24l-7.165 8.804.59.098 7.55-5.526a13.27 13.27 0 00-.976-3.375zm1.304 4.737l-8.932 5.15.59.099 8.71-2.11a13.27 13.27 0 00-.368-3.139zm.456 4.383l-9.765 1.54.116.59 9.768 1.545c.12-.61.2-1.23.24-1.86l-.36-1.815zm-.288 3.12l-9.608 2.116.116.59 9.094 4.57c.33-.7.6-1.43.81-2.18l-.412-5.096zm-1.293 3.886l-8.546 5.46.117.59 7.21 7.128c.6-.56 1.15-1.16 1.65-1.8l-.43-11.378zm-2.483 3.467l-6.578 8.82.117.59 4.243 9.62c.8-.38 1.56-.83 2.27-1.34l-.052-17.69zm-3.313 3.13l-3.95 11.526.117.59.58 11.58c.9-.12 1.77-.32 2.62-.58l.633-23.116zm-3.626 3.025l-.995 12.25.59.099.583-11.682-.178-.667zm-.93 3.067l-.347 1.765c.11.04.22.07.33.11l.017-1.875z"/>
                </svg>
                Deploy to Railway
              </a>
            </div>
          </div>
        </div>
      </div>
    </section>
  )
}
