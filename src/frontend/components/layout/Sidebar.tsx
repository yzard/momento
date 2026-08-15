import { NavLink, useLocation } from 'react-router-dom'
import { useState } from 'react'
import { useAuth } from '../../hooks/useAuth'
import {
  Camera,
  ChevronDown,
  ChevronLeft,
  ChevronRight,
  Folder,
  UsersRound,
  Image as ImageIcon,
  Map as MapIcon,
  ScanSearch,
  Settings,
  Trash2,
  User,
  Video,
  Wrench,
  type LucideIcon,
} from 'lucide-react'
import { cn } from '../../lib/utils'

interface NavItem {
  to: string
  label: string
  icon: LucideIcon
  children?: NavItem[]
}

const navItems: NavItem[] = [
  {
    to: '/timeline',
    label: 'Timeline',
    icon: Camera,
    children: [
      { to: '/timeline/photos', label: 'Photos', icon: ImageIcon },
      { to: '/timeline/videos', label: 'Videos', icon: Video },
    ],
  },
  { to: '/albums', label: 'Albums', icon: Folder },
  { to: '/map', label: 'Map', icon: MapIcon },
  { to: '/faces', label: 'Faces', icon: UsersRound },
  {
    to: '/utility',
    label: 'Utility',
    icon: Wrench,
    children: [
      { to: '/utility/deduplicate', label: 'Deduplicate', icon: ScanSearch },
    ],
  },
  { to: '/trash', label: 'Trash', icon: Trash2 },
  { to: '/settings', label: 'Settings', icon: Settings },
]

interface SidebarProps {
  isCollapsed: boolean
  isMobileOpen: boolean
  toggleCollapse: () => void
  onNavigate: () => void
}

interface NavSectionProps {
  item: NavItem
  isCollapsed: boolean
  onNavigate: () => void
}

function NavSection({ item, isCollapsed, onNavigate }: NavSectionProps) {
  const location = useLocation()
  const [manuallyCollapsed, setManuallyCollapsed] = useState(false)
  const isFocused = location.pathname === item.to || location.pathname.startsWith(`${item.to}/`)
  const isExpanded = Boolean(item.children?.length) && isFocused && !manuallyCollapsed

  return (
    <div>
      <div className="flex items-center">
        <NavLink
          to={item.to}
          onClick={onNavigate}
          className={({ isActive }) =>
            cn(
              "flex flex-1 rounded-lg transition-all duration-200 group font-medium border border-transparent",
              isCollapsed
                ? "flex-col items-center justify-center gap-1 py-3 px-1 text-[10px]"
                : "flex-row items-center gap-4 px-4 py-3.5 text-sm",
              isActive
                ? "bg-muted/50 text-foreground shadow-sm border-border/50"
                : "text-muted-foreground hover:bg-muted/30 hover:text-foreground",
            )
          }
        >
          {({ isActive }) => (
            <>
              <item.icon
                className={cn(
                  "transition-colors duration-200",
                  isCollapsed ? "w-6 h-6" : "w-5 h-5",
                  isActive ? "text-primary" : "text-muted-foreground group-hover:text-foreground",
                )}
                strokeWidth={2}
              />
              <span className={cn("tracking-wide whitespace-nowrap", isCollapsed ? "text-[10px] font-semibold" : "")}>
                {item.label}
              </span>
            </>
          )}
        </NavLink>
        {item.children && (
          <button
            type="button"
            aria-label={`${isExpanded ? 'Collapse' : 'Expand'} ${item.label}`}
            aria-expanded={isExpanded}
            onClick={() => setManuallyCollapsed(isExpanded)}
            className={cn(
              "rounded-md text-muted-foreground hover:bg-muted/50 hover:text-foreground transition-colors",
              isCollapsed ? "p-1" : "p-2 mr-2",
            )}
          >
            {isExpanded ? <ChevronDown className="w-4 h-4" /> : <ChevronRight className="w-4 h-4" />}
          </button>
        )}
      </div>
      {item.children && isExpanded && (
        <div className={cn("mt-1 space-y-1", isCollapsed ? "" : "ml-4")}>
          {item.children.map((child) => (
            <NavLink
              key={child.to}
              to={child.to}
              onClick={onNavigate}
              title={child.label}
              className={({ isActive }) =>
                cn(
                  "flex items-center rounded-md transition-colors group",
                  isCollapsed ? "justify-center p-2" : "gap-3 px-4 py-2 text-sm",
                  isActive
                    ? "bg-primary/10 text-primary"
                    : "text-muted-foreground hover:bg-muted/30 hover:text-foreground",
                )
              }
            >
              {({ isActive }) => (
                <>
                  <child.icon className={cn("w-4 h-4", isActive ? "text-primary" : "group-hover:text-foreground")} />
                  {!isCollapsed && <span>{child.label}</span>}
                </>
              )}
            </NavLink>
          ))}
        </div>
      )}
    </div>
  )
}

export default function Sidebar({ isCollapsed, isMobileOpen, toggleCollapse, onNavigate }: SidebarProps) {
  const { user } = useAuth()

  return (
    <aside
      className={cn(
        "fixed inset-y-0 left-0 z-40 flex h-full w-72 flex-col border-r border-border bg-background transition-transform duration-300 ease-in-out md:relative md:z-20 md:translate-x-0",
        isMobileOpen ? "translate-x-0" : "-translate-x-full",
        isCollapsed ? "md:w-20" : "md:w-72"
      )}
    >
      <div className={cn("flex items-center", isCollapsed ? "justify-center p-4 py-6" : "p-8 pb-10")}>
        {!isCollapsed && (
          <h2 className="text-2xl font-display font-bold text-foreground tracking-tight animate-fade-in">
            Momento
          </h2>
        )}
      </div>

      <nav className={cn("flex-1 overflow-y-auto", isCollapsed ? "px-2 space-y-4" : "px-6 space-y-8")}>
        <div className="space-y-2">
          {navItems.map((item) => (
            <NavSection key={item.to} item={item} isCollapsed={isCollapsed} onNavigate={onNavigate} />
          ))}
        </div>

        {user?.role === 'admin' && (
          <div className="space-y-2">
            <div className="border-t border-border/50 pt-2" aria-hidden="true" />
            <NavLink
              to="/admin"
              onClick={onNavigate}
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
