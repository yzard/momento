import { useAuth } from '../../hooks/useAuth'
import { LogOut, Bell, Menu } from 'lucide-react'
import { useNavigate } from 'react-router-dom'

export default function Header({ onMenuClick }: { onMenuClick: () => void }) {
  const { user, logout } = useAuth()
  const navigate = useNavigate()

  return (
    <header className="sticky top-0 z-10 px-4 py-4 sm:px-6 md:px-10 md:py-6 flex items-center justify-between bg-background/95 backdrop-blur-sm border-b border-transparent transition-all duration-200">
      <div className="flex items-center gap-3">
        <button
          type="button"
          aria-label="Open navigation"
          onClick={onMenuClick}
          className="flex h-11 w-11 items-center justify-center rounded-lg text-muted-foreground transition-colors hover:bg-muted/50 hover:text-foreground md:hidden"
        >
          <Menu className="h-5 w-5" />
        </button>
        <div className="flex flex-col gap-0.5">
        <h1 className="text-xl font-display font-semibold text-foreground tracking-tight">
          Welcome back, {user?.username}
        </h1>
        <p className="text-sm text-muted-foreground font-medium">
          {new Date().toLocaleDateString('en-US', { weekday: 'long', month: 'long', day: 'numeric' })}
        </p>
        </div>
      </div>
      
      <div className="flex items-center gap-4">
        <button aria-label="Notifications" className="hidden p-2 text-muted-foreground hover:text-foreground hover:bg-muted/50 rounded-full transition-colors sm:block">
            <Bell className="w-5 h-5" />
        </button>

        <div className="hidden h-8 w-px bg-border/50 mx-2 sm:block" />

        <button
          type="button"
          aria-label="Open account settings"
          onClick={() => navigate('/settings')}
          className="flex items-center gap-3 p-1.5 bg-white border border-border rounded-full shadow-sm hover:shadow-md transition-shadow group"
        >
          <div className="w-8 h-8 bg-primary text-primary-foreground rounded-full flex items-center justify-center font-bold text-sm">
            {user?.username?.[0]?.toUpperCase()}
          </div>
          <span className="hidden text-sm font-medium text-foreground tracking-tight pr-2 group-hover:text-primary transition-colors sm:block">{user?.username}</span>
        </button>
        
        <button
          onClick={logout}
          aria-label="Logout"
          className="p-2 text-muted-foreground hover:text-destructive hover:bg-destructive/10 rounded-full transition-all duration-200"
          title="Logout"
        >
          <LogOut className="w-5 h-5" strokeWidth={2} />
        </button>
      </div>
    </header>
  )
}
