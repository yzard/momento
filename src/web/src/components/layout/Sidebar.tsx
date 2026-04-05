import { NavLink } from 'react-router-dom'
import { useAuth } from '../../hooks/useAuth'
import { Camera, Folder, Map as MapIcon, Settings, User, Command, ChevronLeft, ChevronRight, Trash2 } from 'lucide-react'
import { cn } from '../../lib/utils'

const navItems = [
  { to: '/timeline', label: 'Timeline', icon: Camera },
  { to: '/albums', label: 'Albums', icon: Folder },
  { to: '/map', label: 'Map', icon: MapIcon },
  { to: '/trash', label: 'Trash', icon: Trash2 },
  { to: '/settings', label: 'Settings', icon: Settings },
]

interface SidebarProps {
  isCollapsed: boolean
  toggleCollapse: () => void
}

export default function Sidebar({ isCollapsed, toggleCollapse }: SidebarProps) {
  const { user } = useAuth()

  return (
    <aside 
      className={cn(
        "bg-background border-r border-border flex flex-col h-full transition-all duration-300 ease-in-out z-20",
        isCollapsed ? "w-20" : "w-72"
      )}
    >
      <div className={cn("flex items-center", isCollapsed ? "justify-center p-4 py-6" : "p-8 pb-10")}>
        <div className="flex items-center gap-3 text-primary">
          <div className="p-2 bg-primary text-primary-foreground rounded-lg">
             <Command className="w-5 h-5" />
          </div>
          {!isCollapsed && (
            <div className="animate-fade-in">
              <h2 className="text-2xl font-display font-bold text-foreground tracking-tight">Momento</h2>
              <p className="text-xs text-muted-foreground font-medium tracking-widest uppercase pl-1 mt-0.5">Personal Gallery</p>
            </div>
          )}
        </div>
      </div>
      
      <nav className={cn("flex-1 overflow-y-auto", isCollapsed ? "px-2 space-y-4" : "px-6 space-y-8")}>
        <div className="space-y-2">
          {!isCollapsed && (
            <p className="px-4 text-xs font-semibold text-muted-foreground/70 uppercase tracking-widest mb-4 animate-fade-in">
              Menu
            </p>
          )}
          {navItems.map((item) => (
            <NavLink
              key={item.to}
              to={item.to}
              className={({ isActive }) =>
                cn(
                  "flex rounded-lg transition-all duration-200 group font-medium border border-transparent",
                  isCollapsed 
                    ? "flex-col items-center justify-center gap-1 py-3 px-1 text-[10px]" 
                    : "flex-row items-center gap-4 px-4 py-3.5 text-sm",
                  isActive
                    ? "bg-muted/50 text-foreground shadow-sm border-border/50"
                    : "text-muted-foreground hover:bg-muted/30 hover:text-foreground"
                )
              }
            >
              {({ isActive }) => (
                <>
                  <item.icon
                    className={cn(
                      "transition-colors duration-200",
                      isCollapsed ? "w-6 h-6" : "w-5 h-5",
                      isActive ? "text-primary" : "text-muted-foreground group-hover:text-foreground"
                    )}
                    strokeWidth={2}
                  />
                  <span className={cn("tracking-wide whitespace-nowrap", isCollapsed ? "text-[10px] font-semibold" : "")}>
                    {item.label}
                  </span>
                </>
              )}
            </NavLink>
          ))}
        </div>

        {user?.role === 'admin' && (
          <div className="space-y-2">
            {!isCollapsed && (
              <p className="px-4 text-xs font-semibold text-muted-foreground/70 uppercase tracking-widest mb-4 animate-fade-in">
                System
              </p>
            )}
            <NavLink
              to="/admin"
              className={({ isActive }) =>
                cn(
                  "flex rounded-lg transition-all duration-200 font-medium border border-transparent",
                  isCollapsed 
                    ? "flex-col items-center justify-center gap-1 py-3 px-1 text-[10px]" 
                    : "flex-row items-center gap-4 px-4 py-3.5 text-sm",
                  isActive
                    ? "bg-muted/50 text-foreground shadow-sm border-border/50"
                    : "text-muted-foreground hover:bg-muted/30 hover:text-foreground"
                )
              }
            >
              {({ isActive }) => (
                <>
                  <User
                    className={cn(
                      "transition-colors duration-200",
                      isCollapsed ? "w-6 h-6" : "w-5 h-5",
                      isActive ? "text-secondary" : "text-muted-foreground"
                    )}
                    strokeWidth={2}
                  />
                  <span className={cn("tracking-wide whitespace-nowrap", isCollapsed ? "text-[10px] font-semibold" : "")}>
                    Admin
                  </span>
                </>
              )}
            </NavLink>
          </div>
        )}
      </nav>

      <div className={cn("border-t border-border/50", isCollapsed ? "p-4 flex justify-center" : "p-6 flex items-center justify-between")}>
        {!isCollapsed ? (
          <div className="px-4 py-2 bg-primary/5 rounded-xl border border-primary/10 animate-fade-in">
            <p className="text-xs text-primary/80 font-medium text-center">
              Momento v0.1.0
            </p>
          </div>
        ) : null}
        
        <button
          onClick={toggleCollapse}
          className={cn(
            "p-2 rounded-lg text-muted-foreground hover:bg-muted/50 hover:text-foreground transition-colors",
            !isCollapsed ? "ml-auto" : ""
          )}
          aria-label={isCollapsed ? "Expand Sidebar" : "Collapse Sidebar"}
        >
          {isCollapsed ? <ChevronRight className="w-5 h-5" /> : <ChevronLeft className="w-5 h-5" />}
        </button>
      </div>
    </aside>
  )
}

