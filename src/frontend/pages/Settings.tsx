import { useState, type FormEvent } from 'react'
import { useAuth } from '../hooks/useAuth'
import { cn } from '../lib/utils'
import { AlertTriangle, Database, FileText, Loader2, ShieldCheck, Users } from 'lucide-react'
import ImportPanel from '../components/admin/ImportPanel'
import MetadataPanel from '../components/admin/MetadataPanel'
import UserManagement from '../components/admin/UserManagement'
import AiPanel from '../components/admin/AiPanel'

export default function Settings() {
  const { user, changePassword } = useAuth()
  const [currentPassword, setCurrentPassword] = useState('')
  const [newPassword, setNewPassword] = useState('')
  const [confirmPassword, setConfirmPassword] = useState('')
  const [error, setError] = useState('')
  const [isLoading, setIsLoading] = useState(false)
  const webdavUrl = new URL('/webdav/', window.location.origin).toString()

  const handleSubmit = async (e: FormEvent) => {
    e.preventDefault()
    setError('')

    if (newPassword !== confirmPassword) {
      setError('New passwords do not match')
      return
    }

    if (newPassword.length < 8) {
      setError('Password must be at least 8 characters')
      return
    }

    setIsLoading(true)

    try {
      await changePassword(currentPassword, newPassword)
    } catch {
      setError('Failed to change password. Please verify your current password.')
    } finally {
      setIsLoading(false)
    }
  }

  return (
    <div className="flex-1 overflow-y-auto scrollbar-thin scrollbar-thumb-muted-foreground/20 scrollbar-track-transparent">
    <div className="max-w-7xl mx-auto animate-fade-in py-8 px-6 md:px-10">
      <div className="max-w-4xl mx-auto">
      <div className="mb-10">
        <h1 className="text-3xl font-display font-bold text-foreground tracking-tight">Account Settings</h1>
        <p className="mt-1 text-muted-foreground font-medium">Manage your security and preferences.</p>
      </div>

      <div className="bg-white border border-border rounded-xl shadow-sm overflow-hidden">
        <div className="px-8 py-6 border-b border-border bg-muted/30 flex items-center gap-4">
            <div className="w-10 h-10 bg-white border border-border rounded-lg flex items-center justify-center text-primary shadow-sm">
                <ShieldCheck className="w-5 h-5" />
            </div>
             <div>
                <h2 className="text-xl font-display font-semibold text-foreground">Security</h2>
                <p className="text-sm text-muted-foreground">Update your password and security settings.</p>
            </div>
        </div>

        <div className="p-8 sm:p-10">
          {user?.mustChangePassword && (
            <div className="mb-8 bg-amber-500/10 border border-amber-500/20 p-4 rounded-lg flex items-start gap-4">
              <AlertTriangle className="h-5 w-5 text-amber-600 flex-shrink-0 mt-0.5" strokeWidth={2} />
              <div>
                <h3 className="text-sm font-bold text-amber-700 uppercase tracking-wide">Action Required</h3>
                <p className="mt-1 text-sm font-medium text-amber-600/90">
                  Your account requires a password update. Please set a new password to continue using all features.
                </p>
              </div>
            </div>
          )}

          <form onSubmit={handleSubmit} className="space-y-8 max-w-lg">
            {error && (
              <div className="bg-destructive/5 text-destructive p-4 border border-destructive/20 rounded-lg font-medium text-sm flex items-center gap-3">
                <AlertTriangle className="w-5 h-5" strokeWidth={2} />
                {error}
              </div>
            )}
            
            <div className="space-y-6">
              <div className="space-y-2 group">
                <label htmlFor="currentPassword" className="text-xs font-bold uppercase tracking-widest text-muted-foreground group-focus-within:text-foreground transition-colors flex items-center gap-2">
                  Current Password
                </label>
                <input
                  id="currentPassword"
                  type="password"
                  value={currentPassword}
                  onChange={(e) => setCurrentPassword(e.target.value)}
                  className="w-full px-4 py-3 bg-muted/20 border border-input focus:border-primary focus:bg-white outline-none transition-all font-medium rounded-lg focus:ring-4 focus:ring-primary/10 text-foreground"
                  required
                />
              </div>

              <div className="space-y-2 group">
                <label htmlFor="newPassword" className="text-xs font-bold uppercase tracking-widest text-muted-foreground group-focus-within:text-foreground transition-colors flex items-center gap-2">
                  New Password
                </label>
                <input
                  id="newPassword"
                  type="password"
                  value={newPassword}
                  onChange={(e) => setNewPassword(e.target.value)}
                  className="w-full px-4 py-3 bg-muted/20 border border-input focus:border-primary focus:bg-white outline-none transition-all font-medium rounded-lg focus:ring-4 focus:ring-primary/10 text-foreground"
                  required
                  minLength={8}
                />
                <p className="text-xs font-medium text-muted-foreground pl-1">Must be at least 8 characters long.</p>
              </div>

              <div className="space-y-2 group">
                <label htmlFor="confirmPassword" className="text-xs font-bold uppercase tracking-widest text-muted-foreground group-focus-within:text-foreground transition-colors flex items-center gap-2">
                   Confirm New Password
                </label>
                <input
                  id="confirmPassword"
                  type="password"
                  value={confirmPassword}
                  onChange={(e) => setConfirmPassword(e.target.value)}
                  className="w-full px-4 py-3 bg-muted/20 border border-input focus:border-primary focus:bg-white outline-none transition-all font-medium rounded-lg focus:ring-4 focus:ring-primary/10 text-foreground"
                  required
                />
              </div>
            </div>

            <div className="pt-4">
              <button
                type="submit"
                disabled={isLoading}
                className={cn(
                  "px-8 py-3 bg-foreground text-background font-bold text-sm uppercase tracking-wider hover:bg-foreground/90 transition-all rounded-lg shadow-lg hover:shadow-xl disabled:opacity-50 disabled:cursor-not-allowed flex items-center justify-center gap-2 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-offset-2 focus-visible:ring-foreground",
                  isLoading && "opacity-70"
                )}
              >
                {isLoading ? (
                  <>
                    <Loader2 className="w-4 h-4 animate-spin" />
                    Updating...
                  </>
                ) : 'Update Password'}
              </button>
            </div>
          </form>
        </div>
      </div>
      </div>

      {user?.role === 'admin' && (
        <section className="mt-16 border-t border-border pt-12" aria-labelledby="admin-settings-title">
          <div className="mb-12 flex flex-col gap-4 sm:flex-row sm:items-end sm:justify-between">
            <div>
              <h2 id="admin-settings-title" className="text-3xl font-display font-bold text-foreground tracking-tight">Admin</h2>
              <p className="mt-1 text-muted-foreground font-medium">System configuration and data management.</p>
            </div>
            <div className="flex w-fit items-center gap-2 px-3 py-1.5 bg-primary/10 border border-primary/20 text-primary font-bold text-xs uppercase tracking-widest rounded-md">
              <ShieldCheck className="w-4 h-4" />
              System Access
            </div>
          </div>

          <div className="space-y-12">
            <div className="grid gap-6 md:grid-cols-[minmax(15rem,0.72fr)_minmax(0,1.5fr)] md:items-start">
              <section className="bg-white border border-border rounded-xl shadow-sm overflow-hidden">
                <div className="px-6 py-5 border-b border-border bg-muted/30 flex items-center gap-3">
                  <div className="w-10 h-10 bg-white border border-border rounded-lg flex items-center justify-center text-primary shadow-sm">
                    <Database className="w-5 h-5" />
                  </div>
                  <h3 className="text-lg font-display font-semibold text-foreground">Local Import</h3>
                </div>
                <div className="p-6">
                  <div className="mb-5 space-y-2 text-sm text-muted-foreground">
                    <p>Place media in <code className="font-mono text-xs text-foreground">/data/imports/</code>.</p>
                    <p>For WebDAV uploads, connect to <code className="break-all font-mono text-xs text-foreground">{webdavUrl}</code> with your Momento username and password.</p>
                  </div>
                  <ImportPanel />
                </div>
              </section>

              <section className="bg-white border border-border rounded-xl shadow-sm overflow-hidden">
                <div className="px-6 py-5 border-b border-border bg-muted/30 flex items-center gap-3">
                  <div className="w-10 h-10 bg-white border border-border rounded-lg flex items-center justify-center text-primary shadow-sm">
                    <FileText className="w-5 h-5" />
                  </div>
                  <div>
                    <h3 className="text-lg font-display font-semibold text-foreground">Metadata</h3>
                    <p className="text-sm text-muted-foreground">Generate metadata.</p>
                  </div>
                </div>
                <div className="p-6">
                  <MetadataPanel />
                </div>
              </section>
            </div>

            <AiPanel />

            <section className="bg-white border border-border rounded-xl shadow-sm overflow-hidden group">
              <div className="px-8 py-6 border-b border-border bg-muted/30 flex items-center gap-4">
                <div className="w-10 h-10 bg-white border border-border rounded-lg flex items-center justify-center text-secondary shadow-sm">
                  <Users className="w-5 h-5" />
                </div>
                <div>
                  <h3 className="text-xl font-display font-semibold text-foreground">User Management</h3>
                  <p className="text-sm text-muted-foreground">Manage user access and permissions.</p>
                </div>
              </div>
              <div className="p-8">
                <UserManagement />
              </div>
            </section>
          </div>
        </section>
      )}

      <p className="mt-6 text-center text-xs text-muted-foreground">
        Location data adapted from{' '}
        <a
          href="https://www.geonames.org/"
          target="_blank"
          rel="noreferrer"
          className="font-semibold text-foreground underline-offset-4 hover:underline"
        >
          GeoNames
        </a>{' '}
        under{' '}
        <a
          href="https://creativecommons.org/licenses/by/4.0/"
          target="_blank"
          rel="noreferrer"
          className="font-semibold text-foreground underline-offset-4 hover:underline"
        >
          CC BY 4.0
        </a>.
      </p>
    </div>
    </div>
  )
}
