import { useState, type FormEvent } from 'react'
import { authApi } from '../api/auth'
import { useAuth } from '../hooks/useAuth'
import { cn } from '../lib/utils'
import { AlertTriangle, CheckCircle, Loader2, ShieldCheck } from 'lucide-react'

export default function Settings() {
  const { user, refreshUser } = useAuth()
  const [currentPassword, setCurrentPassword] = useState('')
  const [newPassword, setNewPassword] = useState('')
  const [confirmPassword, setConfirmPassword] = useState('')
  const [error, setError] = useState('')
  const [success, setSuccess] = useState('')
  const [isLoading, setIsLoading] = useState(false)

  const handleSubmit = async (e: FormEvent) => {
    e.preventDefault()
    setError('')
    setSuccess('')

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
      await authApi.changePassword(currentPassword, newPassword)
      await refreshUser()
      setSuccess('Password changed successfully')
      setCurrentPassword('')
      setNewPassword('')
      setConfirmPassword('')
    } catch {
      setError('Failed to change password. Please verify your current password.')
    } finally {
      setIsLoading(false)
    }
  }

  return (
    <div className="flex-1 overflow-y-auto scrollbar-thin scrollbar-thumb-muted-foreground/20 scrollbar-track-transparent">
    <div className="max-w-4xl mx-auto animate-fade-in py-8 px-6 md:px-10">
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
            
            {success && (
              <div className="bg-green-500/10 text-green-600 p-4 border border-green-500/20 rounded-lg font-medium text-sm flex items-center gap-3">
                <CheckCircle className="w-5 h-5" strokeWidth={2} />
                {success}
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
    </div>
  )
}

