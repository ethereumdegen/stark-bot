import { useEffect } from 'react'
import { Navbar } from '../components/Navbar'
import { Hero } from '../components/Hero'
import { Features } from '../components/Features'
import { GetStarted } from '../components/GetStarted'
import { InteractiveDemo } from '../components/InteractiveDemo'
import { CTA } from '../components/CTA'
import { Footer } from '../components/Footer'
import { GridBackground } from '../components/GridBackground'
import { FloatingNav } from '../components/FloatingNav'
import { Stars } from '../components/Stars'
import { Token } from '../components/Token'

export default function Home() {
  useEffect(() => {
    // Smooth scroll for anchor links
    const handleClick = (e: MouseEvent) => {
      const target = e.target as HTMLElement
      const anchor = target.closest('a[href^="#"]')
      if (anchor) {
        e.preventDefault()
        const id = anchor.getAttribute('href')?.slice(1)
        const element = document.getElementById(id || '')
        if (element) {
          element.scrollIntoView({ behavior: 'smooth', block: 'start' })
        }
      }
    }
    document.addEventListener('click', handleClick)
    return () => document.removeEventListener('click', handleClick)
  }, [])

  return (
    <div className="min-h-screen overflow-x-hidden">
      <Stars />
      <GridBackground />
      <div className="relative z-10">
        <Navbar />
        <Hero />
        <Features />
        <Token />
        <InteractiveDemo />
        <GetStarted />
        <CTA />
        <Footer />
        <FloatingNav />
      </div>
    </div>
  )
}
