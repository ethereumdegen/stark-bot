import { Github, BookOpen, ArrowUp } from 'lucide-react'
import { Link } from 'react-router-dom'
import { useState } from 'react'
import { DarkModeToggle } from './DarkModeToggle'

export function FloatingNav() {
  const [hoveredItem, setHoveredItem] = useState<string | null>(null)

  const scrollToTop = () => {
    window.scrollTo({ top: 0, behavior: 'smooth' })
  }

  const items = [
    {
      id: 'github',
      icon: Github,
      label: 'GitHub',
      href: 'https://github.com/ethereumdegen/stark-bot',
      external: true,
    },
    {
      id: 'docs',
      icon: BookOpen,
      label: 'Docs',
      to: '/docs',
      external: false,
    },
    {
      id: 'top',
      icon: ArrowUp,
      label: 'Top',
      onClick: scrollToTop,
    },
  ]

  return (
    <div className="fixed bottom-6 right-6 z-50">
      <div className="glass rounded-full px-2 py-2 flex items-center gap-1">
        <DarkModeToggle />
        {items.map((item) => {
          const Icon = item.icon
          const isHovered = hoveredItem === item.id

          const content = (
            <div className="relative flex items-center">
              <div
                className={`p-2.5 rounded-full transition-all duration-200 ${
                  isHovered ? 'bg-white/10' : ''
                }`}
              >
                <Icon className="w-5 h-5 text-white/70" />
              </div>
              <span
                className={`absolute right-full mr-2 px-2 py-1 text-sm text-white/90 bg-white/10 backdrop-blur-md rounded whitespace-nowrap transition-all duration-200 ${
                  isHovered ? 'opacity-100 translate-x-0' : 'opacity-0 translate-x-2 pointer-events-none'
                }`}
              >
                {item.label}
              </span>
            </div>
          )

          if (item.onClick) {
            return (
              <button
                key={item.id}
                onClick={item.onClick}
                onMouseEnter={() => setHoveredItem(item.id)}
                onMouseLeave={() => setHoveredItem(null)}
                className="focus:outline-none"
              >
                {content}
              </button>
            )
          }

          if (item.external) {
            return (
              <a
                key={item.id}
                href={item.href}
                target="_blank"
                rel="noopener noreferrer"
                onMouseEnter={() => setHoveredItem(item.id)}
                onMouseLeave={() => setHoveredItem(null)}
              >
                {content}
              </a>
            )
          }

          return (
            <Link
              key={item.id}
              to={item.to!}
              onMouseEnter={() => setHoveredItem(item.id)}
              onMouseLeave={() => setHoveredItem(null)}
            >
              {content}
            </Link>
          )
        })}
      </div>
    </div>
  )
}
