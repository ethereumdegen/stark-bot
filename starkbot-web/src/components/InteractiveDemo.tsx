import { useState, useEffect, useRef } from 'react'
import { Play, Pause, RotateCcw, Code, CheckCircle, Clock, Zap, MessageSquare, Send, Menu } from 'lucide-react'

interface DemoStep {
  id: string
  type: 'typing' | 'user' | 'tool_call' | 'tool_result' | 'assistant'
  content?: string
  toolName?: string
  params?: Record<string, unknown>
  success?: boolean
  delay: number
}

const demoSequences: DemoStep[][] = [
  // Sequence 1: Token Balance Check
  [
    { id: '1', type: 'typing', content: 'check my STARKBOT balance', delay: 1000 },
    { id: '2', type: 'user', content: 'check my STARKBOT balance', delay: 500 },
    { id: '3', type: 'tool_call', toolName: 'token_lookup', params: { symbol: 'STARKBOT' }, delay: 800 },
    { id: '4', type: 'tool_result', toolName: 'token_lookup', content: 'STARKBOT token found: 0x1234...5678 on Base', success: true, delay: 600 },
    { id: '5', type: 'tool_call', toolName: 'web3_function_call', params: { preset: 'erc20_balance', token: 'STARKBOT' }, delay: 800 },
    { id: '6', type: 'tool_result', toolName: 'web3_function_call', content: 'Balance: 1,337.42 STARKBOT ($42.69 USD)', success: true, delay: 600 },
    { id: '7', type: 'assistant', content: 'You have 1,337.42 STARKBOT tokens worth approximately $42.69 USD.', delay: 1000 },
  ],
  // Sequence 2: Moltbook Post
  [
    { id: '1', type: 'typing', content: 'post on moltbook about crypto markets', delay: 1200 },
    { id: '2', type: 'user', content: 'post on moltbook about crypto markets', delay: 500 },
    { id: '3', type: 'tool_call', toolName: 'use_skill', params: { skill_name: 'moltbook' }, delay: 800 },
    { id: '4', type: 'tool_result', toolName: 'use_skill', content: 'Moltbook skill loaded successfully', success: true, delay: 600 },
    { id: '5', type: 'tool_call', toolName: 'moltbook_post', params: { content: 'Crypto markets are looking interesting today. What are your thoughts on the current trends?' }, delay: 1000 },
    { id: '6', type: 'tool_result', toolName: 'moltbook_post', content: 'Post created successfully! Post ID: #crypto-discuss-12345', success: true, delay: 600 },
    { id: '7', type: 'assistant', content: 'Posted to Moltbook! Your crypto discussion thread is live at #crypto-discuss-12345.', delay: 1000 },
  ],
  // Sequence 3: Token Swap
  [
    { id: '1', type: 'typing', content: 'swap 0.1 ETH to USDC', delay: 1000 },
    { id: '2', type: 'user', content: 'swap 0.1 ETH to USDC', delay: 500 },
    { id: '3', type: 'tool_call', toolName: 'use_skill', params: { skill_name: 'swap' }, delay: 800 },
    { id: '4', type: 'tool_result', toolName: 'use_skill', content: 'Swap skill loaded successfully', success: true, delay: 600 },
    { id: '5', type: 'tool_call', toolName: 'swap_tokens', params: { from: 'ETH', to: 'USDC', amount: '0.1' }, delay: 1000 },
    { id: '6', type: 'tool_result', toolName: 'swap_tokens', content: 'Swap executed! 0.1 ETH → 249.83 USDC (0.15% fee)', success: true, delay: 600 },
    { id: '7', type: 'assistant', content: 'Successfully swapped 0.1 ETH for 249.83 USDC. Transaction confirmed on Base network.', delay: 1000 },
  ],
]

const getRandomSequence = () => demoSequences[Math.floor(Math.random() * demoSequences.length)]

export function InteractiveDemo() {
  const [messages, setMessages] = useState<any[]>([])
  const [inputValue, setInputValue] = useState('')
  const [isTyping, setIsTyping] = useState(false)
  const [currentStep, setCurrentStep] = useState(0)
  const [currentSequence, setCurrentSequence] = useState(() => getRandomSequence())
  const [isPaused, setIsPaused] = useState(false)
  const [isPlaying, setIsPlaying] = useState(true)
  const messagesContainerRef = useRef<HTMLDivElement>(null)
  const timeoutRef = useRef<NodeJS.Timeout | null>(null)

  const scrollToBottom = () => {
    if (messagesContainerRef.current) {
      messagesContainerRef.current.scrollTop = messagesContainerRef.current.scrollHeight
    }
  }

  useEffect(() => {
    const timer = setTimeout(scrollToBottom, 50)
    return () => clearTimeout(timer)
  }, [messages])

  useEffect(() => {
    return () => {
      if (timeoutRef.current) {
        clearTimeout(timeoutRef.current)
      }
    }
  }, [])

  useEffect(() => {
    if (!isPlaying || isPaused) return

    if (currentStep >= currentSequence.length) {
      timeoutRef.current = setTimeout(() => {
        setMessages([])
        setInputValue('')
        setCurrentSequence(getRandomSequence())
        setCurrentStep(0)
      }, 2000)
      return
    }

    const step = currentSequence[currentStep]
    timeoutRef.current = setTimeout(() => {
      processStep(step)
    }, step.delay)

    return () => {
      if (timeoutRef.current) {
        clearTimeout(timeoutRef.current)
      }
    }
  }, [currentStep, currentSequence, isPlaying, isPaused])

  const processStep = (step: any) => {
    switch (step.type) {
      case 'typing':
        setIsTyping(true)
        typeText(step.content || '', 0)
        break
      case 'user':
        setInputValue('')
        setIsTyping(false)
        setMessages(prev => [...prev, {
          id: crypto.randomUUID(),
          type: 'user',
          content: step.content || ''
        }])
        setCurrentStep(prev => prev + 1)
        break
      case 'tool_call':
        setMessages(prev => [...prev, {
          id: crypto.randomUUID(),
          type: 'tool_call',
          toolName: step.toolName,
          params: step.params,
          content: ''
        }])
        setCurrentStep(prev => prev + 1)
        break
      case 'tool_result':
        setMessages(prev => [...prev, {
          id: crypto.randomUUID(),
          type: 'tool_result',
          toolName: step.toolName,
          success: step.success,
          content: step.content || ''
        }])
        setCurrentStep(prev => prev + 1)
        break
      case 'assistant':
        setMessages(prev => [...prev, {
          id: crypto.randomUUID(),
          type: 'assistant',
          content: step.content || ''
        }])
        setCurrentStep(prev => prev + 1)
        break
    }
  }

  const typeText = (text: string, index: number) => {
    if (index <= text.length) {
      setInputValue(text.slice(0, index))
      timeoutRef.current = setTimeout(() => {
        typeText(text, index + 1)
      }, 50)
    } else {
      setCurrentStep(prev => prev + 1)
    }
  }

  const resetDemo = () => {
    if (timeoutRef.current) {
      clearTimeout(timeoutRef.current)
    }
    setMessages([])
    setInputValue('')
    setCurrentSequence(getRandomSequence())
    setCurrentStep(0)
    setIsPaused(false)
    setIsPlaying(true)
  }

  const togglePause = () => {
    setIsPaused(prev => !prev)
  }

  const togglePlay = () => {
    setIsPlaying(prev => !prev)
  }

  return (
    <div className="w-full max-w-4xl mx-auto">
      <div className="bg-slate-900 rounded-xl border border-slate-700 overflow-hidden shadow-2xl">
        {/* Header */}
        <div className="flex items-center justify-between px-4 py-3 border-b border-slate-700 bg-slate-800/50">
          <div className="flex items-center gap-3">
            <span className="text-lg font-bold text-white">Agent Chat</span>
            <div className="flex items-center gap-2 bg-slate-700/50 px-2 py-1 rounded">
              <span className="text-xs text-slate-500">Session:</span>
              <span className="text-xs font-mono text-slate-300">00000042</span>
            </div>
            <div className="flex items-center gap-2">
              <span className="w-2 h-2 rounded-full bg-green-400" />
              <span className="text-sm text-slate-400">Connected</span>
            </div>
          </div>
          <div className="flex items-center gap-2">
            <span className="text-xs font-mono text-slate-300 bg-slate-700/50 px-2 py-1 rounded">
              0x57bf...d989
            </span>
            <span className="text-xs px-2 py-0.5 bg-blue-500/20 text-blue-400 rounded-full font-medium">
              USDC
            </span>
            <button
              onClick={togglePlay}
              className="p-2 bg-slate-700/50 border border-slate-600 rounded-lg text-slate-400 hover:text-white transition-colors"
              title={isPlaying ? "Pause demo" : "Play demo"}
            >
              {isPlaying ? <Pause className="w-4 h-4" /> : <Play className="w-4 h-4" />}
            </button>
            <button
              onClick={togglePause}
              className="p-2 bg-slate-700/50 border border-slate-600 rounded-lg text-slate-400 hover:text-white transition-colors"
              title={isPaused ? "Resume demo" : "Pause demo"}
            >
              {isPaused ? <Play className="w-4 h-4" /> : <Pause className="w-4 h-4" />}
            </button>
            <button
              onClick={resetDemo}
              className="p-2 bg-slate-700/50 border border-slate-600 rounded-lg text-slate-400 hover:text-white transition-colors"
              title="Reset demo"
            >
              <RotateCcw className="w-4 h-4" />
            </button>
          </div>
        </div>

        {/* Messages area */}
        <div ref={messagesContainerRef} className="h-80 overflow-y-auto p-4 space-y-3">
          {messages.length === 0 ? (
            <div className="h-full flex items-center justify-center">
              <div className="text-center text-slate-500">
                <p>Start a conversation...</p>
              </div>
            </div>
          ) : (
            messages.map((msg) => (
              <MessageBubble key={msg.id} message={msg} />
            ))
          )}
        </div>

        {/* Input area */}
        <div className="px-4 pb-4">
          <div className="flex gap-3">
            <div className="flex-1 relative">
              <input
                type="text"
                value={inputValue}
                readOnly
                placeholder="Type a message or /command..."
                className="w-full px-4 py-3 bg-slate-800 border border-slate-700 rounded-lg text-white placeholder-slate-500 focus:outline-none"
              />
              {isTyping && (
                <span className="absolute right-3 top-1/2 -translate-y-1/2 w-0.5 h-5 bg-white animate-pulse" />
              )}
            </div>
            <button className="p-3 bg-slate-700/50 border border-slate-600 rounded-lg text-slate-400 hover:text-white transition-colors">
              <MessageSquare className="w-5 h-5" />
            </button>
            <button className="p-3 bg-orange-500 hover:bg-orange-600 rounded-lg text-white transition-colors">
              <Zap className="w-5 h-5" />
            </button>
          </div>
        </div>
      </div>
    </div>
  )
}

function MessageBubble({ message }: { message: any }) {
  if (message.type === 'user') {
    return (
      <div className="flex justify-end animate-fade-in">
        <div className="max-w-[80%] px-4 py-3 rounded-2xl rounded-br-md bg-orange-500 text-white">
          <p className="whitespace-pre-wrap break-words text-sm">{message.content}</p>
        </div>
      </div>
    )
  }

  if (message.type === 'tool_call') {
    return (
      <div className="flex justify-start animate-fade-in">
        <div className="w-full px-4 py-3 rounded-r-xl rounded-l-sm border-l-4 border-l-amber-500 bg-slate-800/95 border border-slate-700/60">
          <div className="flex items-center gap-2 mb-2">
            <Code className="w-4 h-4 text-amber-400" />
            <span className="text-sm font-semibold text-amber-300">Tool</span>
          </div>
          <div className="bg-slate-900/80 rounded-lg p-3">
            <p className="text-sm text-slate-200 mb-2">
              <span className="text-slate-400">Tool Call:</span>{' '}
              <span className="font-mono text-amber-300">{message.toolName}</span>
            </p>
            {message.params && (
              <div className="text-xs text-slate-400 font-mono">
                <span className="text-slate-500">Params: </span>
                {JSON.stringify(message.params, null, 2)}
              </div>
            )}
          </div>
        </div>
      </div>
    )
  }

  if (message.type === 'tool_result') {
    return (
      <div className="flex justify-start animate-fade-in">
        <div className={`w-full px-4 py-3 rounded-r-xl rounded-l-sm border-l-4 ${message.success ? 'border-l-green-500 bg-green-500/10' : 'border-l-red-500 bg-red-500/10'} border border-slate-700/60`}>
          <div className="flex items-center gap-2 mb-2">
            {message.success ? (
              <CheckCircle className="w-4 h-4 text-green-400" />
            ) : (
              <Clock className="w-4 h-4 text-red-400" />
            )}
            <span className={`text-sm font-semibold ${message.success ? 'text-green-300' : 'text-red-300'}`}>
              {message.success ? 'Success' : 'Error'}
            </span>
          </div>
          <div className="bg-slate-900/80 rounded-lg p-3">
            <p className="text-sm text-slate-200">{message.content}</p>
          </div>
        </div>
      </div>
    )
  }

  return (
    <div className="flex justify-start animate-fade-in">
      <div className="max-w-[80%] px-4 py-3 rounded-2xl rounded-bl-md bg-slate-700 text-white">
        <p className="whitespace-pre-wrap break-words text-sm">{message.content}</p>
      </div>
    </div>
  )
}