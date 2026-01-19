import { Outlet } from 'react-router-dom'
import { useState } from 'react'
import Header from './Header'
import Sidebar from './Sidebar'

export default function Layout() {
  const [isSidebarCollapsed, setIsSidebarCollapsed] = useState(false)

  return (
    <div className="flex h-screen bg-background text-foreground overflow-hidden font-sans selection:bg-primary/20 selection:text-primary-foreground">
      <Sidebar isCollapsed={isSidebarCollapsed} toggleCollapse={() => setIsSidebarCollapsed(!isSidebarCollapsed)} />
      <div className="flex flex-col flex-1 min-w-0 relative bg-background">
        <div className="absolute inset-0 bg-[radial-gradient(hsl(var(--foreground)/0.03)_1px,transparent_1px)] [background-size:32px_32px] pointer-events-none" />
        <Header />
        <main id="app-main" className="flex-1 overflow-hidden z-0 relative flex flex-col">
          <Outlet />
        </main>
      </div>
    </div>
  )
}

