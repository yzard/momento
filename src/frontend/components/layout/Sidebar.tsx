import { NavLink, useLocation } from 'react-router-dom'
import { useState } from 'react'
import {
  Camera,
  ChevronLeft,
  ChevronRight,
  Folder,
  LogOut,
  UsersRound,
  Image as ImageIcon,
  Map as MapIcon,
  MapPinned,
  ScanText,
  ScanSearch,
  Trash2,
  Video,
  Wrench,
  type LucideIcon,
} from 'lucide-react'
import { cn } from '../../lib/utils'
import { MOMENTO_VERSION } from '../../lib/version'
import { useAuth } from '../../hooks/useAuth'
import AndroidAppDownloadLink from './AndroidAppDownloadLink'

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
      { to: '/timeline/screenshots', label: 'Screenshot', icon: ScanSearch },
      { to: '/timeline/documents', label: 'Document', icon: ScanText },
    ],
  },
  { to: '/albums', label: 'Albums', icon: Folder },
  { to: '/map', label: 'Map', icon: MapIcon },
  { to: '/places', label: 'Places', icon: MapPinned },
  { to: '/faces', label: 'Faces', icon: UsersRound },
  {
    to: '/utility',
    label: 'Utility',
    icon: Wrench,
    children: [{ to: '/utility/deduplicate', label: 'Deduplicate', icon: ScanSearch }],
  },
  { to: '/trash', label: 'Trash', icon: Trash2 },
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

function PrimaryNavLink({
  item,
  isCollapsed,
  search,
  onNavigate,
}: NavSectionProps & { search: string }) {
  return (
    <NavLink
      to={`${item.to}${search}`}
      onClick={onNavigate}
      className={({ isActive }) =>
        cn(
          'group flex flex-1 rounded-lg border border-transparent font-medium transition-all duration-200',
          isCollapsed
            ? 'flex-col items-center justify-center gap-1 px-1 py-3 text-[10px]'
            : 'flex-row items-center gap-4 px-4 py-3.5 text-sm',
          isActive
            ? 'border-border/50 bg-muted/50 text-foreground shadow-sm'
            : 'text-muted-foreground hover:bg-muted/30 hover:text-foreground'
        )
      }
    >
      {({ isActive }) => (
        <>
          <item.icon
            className={cn(
              'transition-colors duration-200',
              isCollapsed ? 'h-6 w-6' : 'h-5 w-5',
              isActive ? 'text-primary' : 'text-muted-foreground group-hover:text-foreground'
            )}
            strokeWidth={2}
          />
          <span
            className={cn(
              'whitespace-nowrap tracking-wide',
              isCollapsed && 'text-[10px] font-semibold'
            )}
          >
            {item.label}
          </span>
        </>
      )}
    </NavLink>
  )
}

function ChildNavLinks({
  children,
  expanded,
  isCollapsed,
  search,
  onNavigate,
}: {
  children: NavItem[] | undefined
  expanded: boolean
  isCollapsed: boolean
  search: string
  onNavigate: () => void
}) {
  if (!children || !expanded) return null

  return (
    <div className={cn('mt-1 space-y-1', !isCollapsed && 'ml-4')}>
      {children.map((child) => (
        <NavLink
          key={child.to}
          to={`${child.to}${search}`}
          onClick={onNavigate}
          title={child.label}
          className={({ isActive }) =>
            cn(
              'group flex items-center rounded-md transition-colors',
              isCollapsed ? 'justify-center p-2' : 'gap-3 px-4 py-2 text-sm',
              isActive
                ? 'bg-primary/10 text-primary'
                : 'text-muted-foreground hover:bg-muted/30 hover:text-foreground'
            )
          }
        >
          {({ isActive }) => (
            <>
              <child.icon
                className={cn('h-4 w-4', isActive ? 'text-primary' : 'group-hover:text-foreground')}
              />
              {!isCollapsed && <span>{child.label}</span>}
            </>
          )}
        </NavLink>
      ))}
    </div>
  )
}

function NavSection({ item, isCollapsed, onNavigate }: NavSectionProps) {
  const location = useLocation()
  const [expandedOverride, setExpandedOverride] = useState<boolean | null>(null)
  const isFocused = location.pathname === item.to || location.pathname.startsWith(`${item.to}/`)
  const hasChildren = Boolean(item.children?.length)
  const isExpanded = hasChildren && (expandedOverride ?? isFocused)
  const search = item.to.startsWith('/timeline') ? location.search : ''

  return (
    <div>
      <div className="flex items-center">
        <PrimaryNavLink
          item={item}
          isCollapsed={isCollapsed}
          search={search}
          onNavigate={onNavigate}
        />
        {hasChildren && (
          <button
            type="button"
            aria-label={`${isExpanded ? 'Collapse' : 'Expand'} ${item.label}`}
            aria-expanded={isExpanded}
            onClick={() => setExpandedOverride(!isExpanded)}
            className={cn(
              'inline-flex shrink-0 items-center justify-center rounded-md text-muted-foreground transition-colors duration-200 hover:bg-muted/50 hover:text-foreground',
              isCollapsed ? 'h-11 w-8' : 'mr-1 h-11 w-11'
            )}
          >
            <ChevronRight
              aria-hidden="true"
              className={cn(
                'h-4 w-4 transition-transform duration-200 motion-reduce:transition-none',
                isExpanded && 'rotate-90'
              )}
            />
          </button>
        )}
      </div>
      <ChildNavLinks
        children={item.children}
        expanded={isExpanded}
        isCollapsed={isCollapsed}
        search={search}
        onNavigate={onNavigate}
      />
    </div>
  )
}

function SidebarBrandLinks({ isCollapsed }: Pick<SidebarProps, 'isCollapsed'>) {
  if (isCollapsed) return <AndroidAppDownloadLink compact />

  return (
    <div className="animate-fade-in space-y-2">
      <div className="rounded-xl border border-primary/10 bg-primary/5 px-3 py-2">
        <p className="text-center text-xs font-medium text-primary/80">v{MOMENTO_VERSION}</p>
      </div>
      <AndroidAppDownloadLink compact={false} />
    </div>
  )
}

function SidebarCollapseButton({
  isCollapsed,
  onClick,
}: {
  isCollapsed: boolean
  onClick: () => void
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      className="rounded-lg p-2 text-muted-foreground transition-colors hover:bg-muted/50 hover:text-foreground"
      aria-label={isCollapsed ? 'Expand Sidebar' : 'Collapse Sidebar'}
    >
      {isCollapsed ? <ChevronRight className="h-5 w-5" /> : <ChevronLeft className="h-5 w-5" />}
    </button>
  )
}

function SidebarAccountControls({
  isCollapsed,
  toggleCollapse,
  onNavigate,
}: Pick<SidebarProps, 'isCollapsed' | 'toggleCollapse' | 'onNavigate'>) {
  const { user, logout } = useAuth()

  return (
    <div className={cn('flex items-center', isCollapsed ? 'flex-col gap-3' : 'gap-2')}>
      <NavLink
        to="/settings"
        onClick={onNavigate}
        aria-label="Open account settings"
        title={user?.username ?? 'Account'}
        className={cn(
          'flex items-center rounded-lg text-foreground transition-colors hover:bg-muted/50',
          isCollapsed ? 'justify-center p-1' : 'min-w-0 flex-1 gap-3 p-1.5'
        )}
      >
        <span className="flex h-9 w-9 shrink-0 items-center justify-center rounded-full bg-primary text-sm font-bold text-primary-foreground">
          {user?.username?.[0]?.toUpperCase()}
        </span>
        {!isCollapsed && <span className="truncate text-sm font-medium">{user?.username}</span>}
      </NavLink>
      {!isCollapsed && (
        <button
          type="button"
          onClick={logout}
          aria-label="Logout"
          title="Logout"
          className="rounded-lg p-2 text-muted-foreground transition-colors hover:bg-destructive/10 hover:text-destructive"
        >
          <LogOut className="h-5 w-5" strokeWidth={2} />
        </button>
      )}
      <SidebarCollapseButton isCollapsed={isCollapsed} onClick={toggleCollapse} />
    </div>
  )
}

function SidebarFooter({
  isCollapsed,
  toggleCollapse,
  onNavigate,
}: Pick<SidebarProps, 'isCollapsed' | 'toggleCollapse' | 'onNavigate'>) {
  return (
    <div
      className={cn(
        'border-t border-border/50',
        isCollapsed ? 'flex flex-col items-center gap-3 p-3' : 'space-y-3 p-6'
      )}
    >
      <SidebarBrandLinks isCollapsed={isCollapsed} />
      <SidebarAccountControls
        isCollapsed={isCollapsed}
        toggleCollapse={toggleCollapse}
        onNavigate={onNavigate}
      />
    </div>
  )
}

export default function Sidebar({
  isCollapsed,
  isMobileOpen,
  toggleCollapse,
  onNavigate,
}: SidebarProps) {
  return (
    <aside
      className={cn(
        'fixed inset-y-0 left-0 z-40 flex h-full w-72 flex-col border-r border-border bg-background transition-transform duration-300 ease-in-out md:relative md:z-20 md:translate-x-0',
        isMobileOpen ? 'translate-x-0' : '-translate-x-full',
        isCollapsed ? 'md:w-20' : 'md:w-72'
      )}
    >
      <div
        className={cn('flex items-center', isCollapsed ? 'justify-center p-4 py-6' : 'p-8 pb-10')}
      >
        {!isCollapsed && (
          <h2 className="text-2xl font-display font-bold text-foreground tracking-tight animate-fade-in">
            Momento
          </h2>
        )}
      </div>

      <nav
        className={cn('flex-1 overflow-y-auto', isCollapsed ? 'px-2 space-y-4' : 'px-6 space-y-8')}
      >
        <div className="space-y-2">
          {navItems.map((item) => (
            <NavSection
              key={item.to}
              item={item}
              isCollapsed={isCollapsed}
              onNavigate={onNavigate}
            />
          ))}
        </div>
      </nav>

      <SidebarFooter
        isCollapsed={isCollapsed}
        toggleCollapse={toggleCollapse}
        onNavigate={onNavigate}
      />
    </aside>
  )
}
