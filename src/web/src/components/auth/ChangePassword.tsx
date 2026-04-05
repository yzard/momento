import { useState, type FormEvent } from 'react'
import { authApi } from '../../api/auth'
import { useAuth } from '../../hooks/useAuth'
import { cn } from '../../lib/utils'

interface ChangePasswordProps {
  onComplete: () => void
}

export default function ChangePassword({ onComplete }: ChangePasswordProps) {
  const { refreshUser } = useAuth()
  const [currentPassword, setCurrentPassword] = useState('')
  const [newPassword, setNewPassword] = useState('')
  const [confirmPassword, setConfirmPassword] = useState('')
  const [error, setError] = useState('')
  const [isLoading, setIsLoading] = useState(false)

  const handleSubmit = async (event: FormEvent) => {
    event.preventDefault()
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
      await authApi.changePassword(currentPassword, newPassword)
      await refreshUser()
      setCurrentPassword('')
      setNewPassword('')
      setConfirmPassword('')
      onComplete()
    } catch {
      setError('Failed to change password. Please verify your current password.')
    } finally {
      setIsLoading(false)
    }
  }

  return (
    <form onSubmit={handleSubmit} className="space-y-4">
      {error && (
        <div className="bg-destructive/10 text-destructive p-3 rounded-lg text-sm border border-destructive/20 font-medium">
          {error}
        </div>
      )}
      <div className="space-y-2">
        <label htmlFor="currentPassword" className="text-xs font-bold uppercase tracking-wider text-muted-foreground">
          Current Password
        </label>
        <input
          id="currentPassword"
          type="password"
          value={currentPassword}
          onChange={(event) => setCurrentPassword(event.target.value)}
          className="w-full px-4 py-3 bg-muted/20 border-2 border-input focus:border-primary focus:bg-background outline-none transition-all font-medium"
          required
        />
      </div>
      <div className="space-y-2">
        <label htmlFor="newPassword" className="text-xs font-bold uppercase tracking-wider text-muted-foreground">
          New Password
        </label>
        <input
          id="newPassword"
          type="password"
          value={newPassword}
          onChange={(event) => setNewPassword(event.target.value)}
          className="w-full px-4 py-3 bg-muted/20 border-2 border-input focus:border-primary focus:bg-background outline-none transition-all font-medium"
          required
          minLength={8}
        />
      </div>
      <div className="space-y-2">
        <label htmlFor="confirmPassword" className="text-xs font-bold uppercase tracking-wider text-muted-foreground">
          Confirm New Password
        </label>
        <input
          id="confirmPassword"
          type="password"
          value={confirmPassword}
          onChange={(event) => setConfirmPassword(event.target.value)}
          className="w-full px-4 py-3 bg-muted/20 border-2 border-input focus:border-primary focus:bg-background outline-none transition-all font-medium"
          required
        />
      </div>
      <button
        type="submit"
        disabled={isLoading}
        className={cn(
          "w-full bg-primary text-primary-foreground py-3 font-bold uppercase tracking-wider transition-all hover:bg-primary/90 hover:shadow-[4px_4px_0px_0px_rgba(0,0,0,0.1)] hover:-translate-y-1 active:translate-y-0 active:shadow-none disabled:opacity-50 mt-4 border-2 border-transparent",
          isLoading && "cursor-not-allowed"
        )}
      >
        {isLoading ? 'Updating...' : 'Update Password'}
      </button>
    </form>
  )
}

