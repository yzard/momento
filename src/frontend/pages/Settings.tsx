import { AlertTriangle, Monitor, Moon, Palette, ShieldCheck, Sun } from 'lucide-react'

import PasswordChangeForm from '../components/auth/PasswordChangeForm'
import AndroidAppDownloadLink from '../components/layout/AndroidAppDownloadLink'
import { PageFrame, PageHeader } from '../components/layout/PageLayout'
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
      className="overflow-hidden rounded-xl border border-border bg-card shadow-sm"
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
      <PageFrame className="animate-fade-in">
        <PageHeader
          title="Account Settings"
          description="Manage your security and preferences."
          actions={null}
        />
        <div className="grid items-start gap-8 xl:grid-cols-2">
          <div className="space-y-8">
            <AppearanceSettings />
            <section
              className="overflow-hidden rounded-xl border border-border bg-card shadow-sm"
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
          </div>
          <SecuritySettings mustChangePassword={Boolean(user?.mustChangePassword)} />
        </div>
        <LocationAttribution />
      </PageFrame>
    </div>
  )
}
