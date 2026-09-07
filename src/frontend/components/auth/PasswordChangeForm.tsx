import { useState, type FormEvent } from 'react'

import { useAuth } from '../../hooks/useAuth'
import { cn } from '../../lib/utils'

interface PasswordChangeFormProps {
  onComplete: () => void
  layout: 'modal' | 'settings'
}

function validateNewPassword(newPassword: string, confirmPassword: string): string | null {
  if (newPassword !== confirmPassword) return 'New passwords do not match'
  if (newPassword.length < 8) return 'Password must be at least 8 characters'
  return null
}

export default function PasswordChangeForm({ onComplete, layout }: PasswordChangeFormProps) {
  const { user, changePassword } = useAuth()
  const [error, setError] = useState('')
  const [isLoading, setIsLoading] = useState(false)

  const handleSubmit = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault()
    const form = event.currentTarget
    const fields = new FormData(form)
    const currentPassword = String(fields.get('currentPassword') ?? '')
    const newPassword = String(fields.get('newPassword') ?? '')
    const confirmPassword = String(fields.get('confirmPassword') ?? '')
    const validationError = validateNewPassword(newPassword, confirmPassword)
    if (validationError) {
      setError(validationError)
      return
    }

    setError('')
    setIsLoading(true)
    try {
      await changePassword(currentPassword, newPassword)
      form.reset()
      onComplete()
    } catch {
      setError('Failed to change password. Please verify your current password.')
    } finally {
      setIsLoading(false)
    }
  }

  const inputClassName = cn(
    'w-full px-4 py-3 border border-input focus:border-primary outline-none transition-all font-medium rounded-lg focus:ring-4 focus:ring-primary/10 text-foreground',
    layout === 'modal' ? 'bg-muted/20 focus:bg-background' : 'bg-muted/20 focus:bg-card'
  )

  return (
    <form
      id="change-password"
      autoComplete="on"
      method="post"
      onSubmit={handleSubmit}
      className={layout === 'modal' ? 'space-y-4' : 'max-w-lg space-y-8'}
    >
      {user && (
        <input
          type="text"
          id="change-password-username"
          name="username"
          autoComplete="username"
          value={user.username}
          readOnly
          hidden
        />
      )}
      {error && (
        <div
          role="alert"
          className="rounded-lg border border-destructive/20 bg-destructive/10 p-3 text-sm font-medium text-destructive"
        >
          {error}
        </div>
      )}
      <div className="space-y-6">
        <PasswordField
          id="currentPassword"
          label="Current Password"
          minLength={null}
          inputClassName={inputClassName}
        />
        <PasswordField
          id="newPassword"
          label="New Password"
          minLength={8}
          inputClassName={inputClassName}
        />
        <PasswordField
          id="confirmPassword"
          label="Confirm New Password"
          minLength={null}
          inputClassName={inputClassName}
        />
      </div>
      <button
        type="submit"
        disabled={isLoading}
        className={cn(
          'bg-foreground text-background px-8 py-3 font-bold text-sm uppercase tracking-wider hover:bg-foreground/90 transition-all rounded-lg shadow-lg disabled:opacity-50 disabled:cursor-not-allowed',
          layout === 'modal' && 'w-full',
          isLoading && 'opacity-70'
        )}
      >
        {isLoading ? 'Updating...' : 'Update Password'}
      </button>
    </form>
  )
}

interface PasswordFieldProps {
  id: string
  label: string
  minLength: number | null
  inputClassName: string
}

function PasswordField({ id, label, minLength, inputClassName }: PasswordFieldProps) {
  return (
    <div className="space-y-2 group">
      <label
        htmlFor={id}
        className="text-xs font-bold uppercase tracking-widest text-muted-foreground group-focus-within:text-foreground transition-colors"
      >
        {label}
      </label>
      <input
        id={id}
        name={id}
        type="password"
        className={inputClassName}
        required
        minLength={minLength ?? undefined}
        autoComplete="current-password"
      />
      {id === 'newPassword' && (
        <p className="pl-1 text-xs font-medium text-muted-foreground">
          Must be at least 8 characters long.
        </p>
      )}
    </div>
  )
}
