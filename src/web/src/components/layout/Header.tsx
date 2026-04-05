import { useAuth } from '../../hooks/useAuth'
import { LogOut, Bell } from 'lucide-react'

export default function Header() {
  const { user, logout } = useAuth()

  return (
    <header className="sticky top-0 z-10 px-10 py-6 flex items-center justify-between bg-background/95 backdrop-blur-sm border-b border-transparent transition-all duration-200">
      <div className="flex flex-col gap-0.5">
        <h1 className="text-xl font-display font-semibold text-foreground tracking-tight">
          Welcome back, {user?.username}
        </h1>
        <p className="text-sm text-muted-foreground font-medium">
          {new Date().toLocaleDateString('en-US', { weekday: 'long', month: 'long', day: 'numeric' })}
        </p>
      </div>
      
      <div className="flex items-center gap-4">
        <button className="p-2 text-muted-foreground hover:text-foreground hover:bg-muted/50 rounded-full transition-colors">
            <Bell className="w-5 h-5" />
        </button>

        <div className="h-8 w-px bg-border/50 mx-2" />

        <div className="flex items-center gap-3 pl-2 pr-2 py-1.5 bg-white border border-border rounded-full shadow-sm hover:shadow-md transition-shadow cursor-pointer group">
          <div className="w-8 h-8 bg-primary text-primary-foreground rounded-full flex items-center justify-center font-bold text-sm">
            {user?.username?.[0]?.toUpperCase()}
          </div>
          <span className="text-sm font-medium text-foreground tracking-tight pr-2 group-hover:text-primary transition-colors">{user?.username}</span>
        </div>
        
        <button
          onClick={logout}
          className="p-2 text-muted-foreground hover:text-destructive hover:bg-destructive/10 rounded-full transition-all duration-200"
          title="Logout"
        >
          <LogOut className="w-5 h-5" strokeWidth={2} />
        </button>
      </div>
    </header>
  )
}

