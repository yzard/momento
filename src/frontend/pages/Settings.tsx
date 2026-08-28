import {
  AlertTriangle,
  Database,
  FileText,
  Monitor,
  Moon,
  Palette,
  ShieldCheck,
  Sun,
  Users,
} from 'lucide-react'

import AiPanel from '../components/admin/AiPanel'
import ImportPanel from '../components/admin/ImportPanel'
import MetadataPanel from '../components/admin/MetadataPanel'
import UserManagement from '../components/admin/UserManagement'
import PasswordChangeForm from '../components/auth/PasswordChangeForm'
import AndroidAppDownloadLink from '../components/layout/AndroidAppDownloadLink'
import { useAuth } from '../hooks/useAuth'
import { useTheme } from '../hooks/useTheme'
import type { ThemePreference } from '../lib/theme'
import { cn } from '../lib/utils'

const THEME_OPTIONS: Array<{ value: ThemePreference; label: string; icon: typeof Sun }> = [
  { value: 'light', label: 'Light', icon: Sun },
  { value: 'dark', label: 'Dark', icon: Moon },
  { value: 'system', label: 'System', icon: Monitor },
]

function AppearanceSettings() {
  const { preference, setPreference } = useTheme()
  return (
    <section
      className="mb-8 overflow-hidden rounded-xl border border-border bg-card shadow-sm"
      aria-labelledby="appearance-title"
    >
      <div className="flex items-center gap-4 border-b border-border bg-muted/30 px-8 py-6">
        <div className="flex h-10 w-10 items-center justify-center rounded-lg border border-border bg-card text-primary shadow-sm">
          <Palette className="h-5 w-5" />
        </div>
        <div>
          <h2 id="appearance-title" className="font-display text-xl font-semibold text-foreground">
            Appearance
          </h2>
          <p className="text-sm text-muted-foreground">
            Choose a light, dark, or system-matched theme.
          </p>
        </div>
      </div>
      <div className="grid gap-3 p-6 sm:grid-cols-3 sm:p-8">
        {THEME_OPTIONS.map((option) => {
          const Icon = option.icon
          const selected = preference === option.value
          return (
            <button
              key={option.value}
              type="button"
              aria-pressed={selected}
              onClick={() => setPreference(option.value)}
              className={cn(
                'flex items-center justify-center gap-3 rounded-lg border px-4 py-3 text-sm font-semibold transition-colors',
                selected
                  ? 'border-primary bg-primary/10 text-primary'
                  : 'border-border bg-background text-muted-foreground hover:bg-muted/50 hover:text-foreground'
              )}
            >
              <Icon className="h-4 w-4" />
              {option.label}
            </button>
          )
        })}
      </div>
    </section>
  )
}

function SecuritySettings({ mustChangePassword }: { mustChangePassword: boolean }) {
  return (
    <section
      className="overflow-hidden rounded-xl border border-border bg-card shadow-sm"
      aria-labelledby="security-title"
    >
      <div className="flex items-center gap-4 border-b border-border bg-muted/30 px-8 py-6">
        <div className="flex h-10 w-10 items-center justify-center rounded-lg border border-border bg-card text-primary shadow-sm">
          <ShieldCheck className="h-5 w-5" />
        </div>
        <div>
          <h2 id="security-title" className="font-display text-xl font-semibold text-foreground">
            Security
          </h2>
          <p className="text-sm text-muted-foreground">
            Update your password and security settings.
          </p>
        </div>
      </div>
      <div className="p-8 sm:p-10">
        {mustChangePassword && (
          <div className="mb-8 flex items-start gap-4 rounded-lg border border-amber-500/20 bg-amber-500/10 p-4">
            <AlertTriangle className="mt-0.5 h-5 w-5 shrink-0 text-amber-600" />
            <div>
              <h3 className="text-sm font-bold uppercase tracking-wide text-amber-700">
                Action Required
              </h3>
              <p className="mt-1 text-sm font-medium text-amber-600/90">
                Your account requires a password update. Please set a new password to continue using
                all features.
              </p>
            </div>
          </div>
        )}
        <PasswordChangeForm layout="settings" onComplete={() => undefined} />
      </div>
    </section>
  )
}

function AdminSettings() {
  const webdavUrl = new URL('/webdav/', window.location.origin).toString()
  return (
    <section className="mt-16 border-t border-border pt-12" aria-labelledby="admin-settings-title">
      <div className="mb-12 flex flex-col gap-4 sm:flex-row sm:items-end sm:justify-between">
        <div>
          <h2
            id="admin-settings-title"
            className="font-display text-3xl font-bold tracking-tight text-foreground"
          >
            Admin
          </h2>
          <p className="mt-1 font-medium text-muted-foreground">
            System configuration and data management.
          </p>
        </div>
        <div className="flex w-fit items-center gap-2 rounded-md border border-primary/20 bg-primary/10 px-3 py-1.5 text-xs font-bold uppercase tracking-widest text-primary">
          <ShieldCheck className="h-4 w-4" />
          System Access
        </div>
      </div>
      <div className="space-y-12">
        <div className="grid gap-6 md:grid-cols-[minmax(15rem,0.72fr)_minmax(0,1.5fr)] md:items-start">
          <AdminPanel icon={<Database className="h-5 w-5" />} title="Local Import">
            <div className="mb-5 space-y-2 text-sm text-muted-foreground">
              <p>
                Place media in{' '}
                <code className="font-mono text-xs text-foreground">/data/imports/</code>.
              </p>
              <p>
                For WebDAV uploads, connect to{' '}
                <code className="break-all font-mono text-xs text-foreground">{webdavUrl}</code>{' '}
                with your Momento username and password.
              </p>
            </div>
            <ImportPanel />
          </AdminPanel>
          <AdminPanel icon={<FileText className="h-5 w-5" />} title="Metadata">
            <p className="mb-5 text-sm text-muted-foreground">Generate metadata.</p>
            <MetadataPanel />
          </AdminPanel>
        </div>
        <AiPanel />
        <AdminPanel icon={<Users className="h-5 w-5" />} title="User Management">
          <UserManagement />
        </AdminPanel>
      </div>
    </section>
  )
}

function AdminPanel({
  icon,
  title,
  children,
}: {
  icon: React.ReactNode
  title: string
  children: React.ReactNode
}) {
  return (
    <section className="overflow-hidden rounded-xl border border-border bg-card shadow-sm">
      <div className="flex items-center gap-3 border-b border-border bg-muted/30 px-6 py-5">
        <div className="flex h-10 w-10 items-center justify-center rounded-lg border border-border bg-card text-primary shadow-sm">
          {icon}
        </div>
        <h3 className="font-display text-lg font-semibold text-foreground">{title}</h3>
      </div>
      <div className="p-6">{children}</div>
    </section>
  )
}

function LocationAttribution() {
  return (
    <p className="mt-6 text-center text-xs text-muted-foreground">
      Location data adapted from{' '}
      <a
        href="https://www.geonames.org/"
        target="_blank"
        rel="noreferrer"
        className="font-semibold text-foreground hover:underline"
      >
        GeoNames
      </a>{' '}
      under{' '}
      <a
        href="https://creativecommons.org/licenses/by/4.0/"
        target="_blank"
        rel="noreferrer"
        className="font-semibold text-foreground hover:underline"
      >
        CC BY 4.0
      </a>
      .
    </p>
  )
}

export default function Settings() {
  const { user } = useAuth()
  return (
    <div className="flex-1 overflow-y-auto scrollbar-thin scrollbar-thumb-muted-foreground/20 scrollbar-track-transparent">
      <div className="mx-auto max-w-7xl px-6 py-8 md:px-10 animate-fade-in">
        <div className="mx-auto max-w-4xl">
          <header className="mb-10">
            <h1 className="font-display text-3xl font-bold tracking-tight text-foreground">
              Account Settings
            </h1>
            <p className="mt-1 font-medium text-muted-foreground">
              Manage your security and preferences.
            </p>
          </header>
          <AppearanceSettings />
          <section
            className="mb-8 overflow-hidden rounded-xl border border-border bg-card shadow-sm"
            aria-labelledby="android-app-title"
          >
            <div className="flex flex-col gap-5 p-6 sm:flex-row sm:items-center sm:justify-between sm:p-8">
              <div className="max-w-xl">
                <h2
                  id="android-app-title"
                  className="font-display text-xl font-semibold text-foreground"
                >
                  Android app
                </h2>
                <p className="mt-1 text-sm leading-6 text-muted-foreground">
                  Install the release APK provided by this Momento server. Android may ask you to
                  allow installs from your browser.
                </p>
              </div>
              <AndroidAppDownloadLink compact={false} />
            </div>
          </section>
          <SecuritySettings mustChangePassword={Boolean(user?.mustChangePassword)} />
          {user?.role === 'admin' && <AdminSettings />}
          <LocationAttribution />
        </div>
      </div>
    </div>
  )
}
