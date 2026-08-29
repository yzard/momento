import { useCallback, useEffect, useRef, useState, type RefObject } from 'react'
import { AxiosError } from 'axios'
import { AlertCircle } from 'lucide-react'

import { adminApi } from '../../api/admin'
import type { User } from '../../api/auth'
import { useAuth } from '../../hooks/useAuth'
import ConfirmationDialog from '../common/ConfirmationDialog'

interface NewUser {
  username: string
  email: string
  password: string
  role: 'admin' | 'user'
}

const EMPTY_USER: NewUser = { username: '', email: '', password: '', role: 'user' }

function validateNewUser(user: NewUser): Record<string, string> {
  const errors: Record<string, string> = {}
  if (!user.username) errors.username = 'Username is required'
  if (!user.email) errors.email = 'Email is required'
  if (!user.password) errors.password = 'Password is required'
  else if (user.password.length < 8) errors.password = 'Password must be at least 8 characters'
  return errors
}

function useManagedUsers(currentUserId: number | undefined) {
  const [users, setUsers] = useState<User[]>([])
  const [isLoading, setIsLoading] = useState(true)
  const [busyUserIds, setBusyUserIds] = useState<Set<number>>(new Set())
  const [actionError, setActionError] = useState<string | null>(null)
  const loadUsers = useCallback(async () => {
    try {
      setUsers(await adminApi.listUsers())
      setActionError(null)
    } catch {
      setActionError('Could not load users.')
    } finally {
      setIsLoading(false)
    }
  }, [])
  useEffect(() => {
    void loadUsers()
  }, [loadUsers])

  const updateUser = async (user: User, update: { role?: User['role']; isActive?: boolean }) => {
    if (busyUserIds.has(user.id)) return
    setBusyUserIds((current) => new Set(current).add(user.id))
    try {
      await adminApi.updateUser({ userId: user.id, ...update })
      await loadUsers()
    } catch {
      setActionError('Could not update the user.')
    } finally {
      setBusyUserIds((current) => {
        const next = new Set(current)
        next.delete(user.id)
        return next
      })
    }
  }

  const toggleActive = async (user: User) => {
    if (user.isReserved || user.id === currentUserId) return
    await updateUser(user, { isActive: !user.isActive })
  }

  const changeRole = async (user: User, role: User['role']) => {
    if (user.id === currentUserId && role === 'user') return
    await updateUser(user, { role })
  }

  const deleteUser = async (user: User) => {
    if (user.isReserved || user.id === currentUserId || busyUserIds.has(user.id)) return
    setBusyUserIds((current) => new Set(current).add(user.id))
    try {
      await adminApi.deleteUser(user.id)
      await loadUsers()
    } catch {
      setActionError('Could not delete the user.')
    } finally {
      setBusyUserIds((current) => {
        const next = new Set(current)
        next.delete(user.id)
        return next
      })
    }
  }
  return {
    users,
    isLoading,
    busyUserIds,
    actionError,
    loadUsers,
    toggleActive,
    changeRole,
    deleteUser,
  }
}

function UsersTable({
  users,
  currentUserId,
  busyUserIds,
  onToggleActive,
  onChangeRole,
  onDelete,
}: {
  users: User[]
  currentUserId: number | undefined
  busyUserIds: ReadonlySet<number>
  onToggleActive: (user: User) => void
  onChangeRole: (user: User, role: User['role']) => void
  onDelete: (user: User) => void
}) {
  return (
    <div className="overflow-x-auto rounded-xl border border-border/50 bg-card/30">
      <table aria-label="Managed users" className="w-full min-w-[780px] text-sm">
        <thead className="bg-muted/30">
          <tr>
            {['Username', 'Email', 'Role', 'Status', 'Actions'].map((label) => (
              <th
                key={label}
                className="px-4 py-3 text-left text-xs font-medium uppercase tracking-wider text-muted-foreground"
              >
                {label}
              </th>
            ))}
          </tr>
        </thead>
        <tbody className="divide-y divide-border/50">
          {users.map((user) => {
            const protectedUser = user.isReserved || user.id === currentUserId
            const busy = busyUserIds.has(user.id)
            const protectedTitle = user.isReserved
              ? 'The reserved admin account cannot be deactivated or deleted'
              : 'The current account cannot be deactivated or deleted'

            return (
              <tr key={user.id} className="transition-colors duration-200 hover:bg-muted/20">
                <td className="px-4 py-3 font-medium text-foreground">
                  <span>{user.username}</span>
                  {user.isReserved && (
                    <span className="ml-2 rounded border border-primary/30 bg-primary/10 px-2 py-1 text-[10px] font-bold uppercase tracking-wide text-primary">
                      Reserved
                    </span>
                  )}
                </td>
                <td className="px-4 py-3 text-muted-foreground">{user.email}</td>
                <td className="px-4 py-3">
                  <label htmlFor={`role-${user.id}`} className="sr-only">
                    Role for {user.username}
                  </label>
                  <select
                    id={`role-${user.id}`}
                    aria-label={`Role for ${user.username}`}
                    value={user.role}
                    onChange={(event) => onChangeRole(user, event.target.value as User['role'])}
                    disabled={busy || user.id === currentUserId}
                    className="min-h-11 cursor-pointer rounded-lg border border-input bg-background px-3 text-sm font-semibold text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary disabled:cursor-not-allowed disabled:opacity-50"
                  >
                    <option value="user">User</option>
                    <option value="admin">Admin</option>
                  </select>
                </td>
                <td className="px-4 py-3">
                  <span
                    className={`rounded border px-2 py-1 text-xs font-bold uppercase tracking-wide ${user.isActive ? 'border-primary/20 bg-primary/10 text-primary' : 'border-destructive/20 bg-destructive/10 text-destructive'}`}
                  >
                    {user.isActive ? 'Active' : 'Inactive'}
                  </span>
                </td>
                <td className="px-4 py-3">
                  <button
                    type="button"
                    onClick={() => onToggleActive(user)}
                    disabled={protectedUser || busy}
                    title={protectedUser ? protectedTitle : undefined}
                    className="mr-3 min-h-11 cursor-pointer font-medium text-primary hover:underline focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary disabled:cursor-not-allowed disabled:text-muted-foreground disabled:no-underline"
                  >
                    {user.isActive ? 'Deactivate' : 'Activate'}
                  </button>
                  <button
                    type="button"
                    onClick={() => onDelete(user)}
                    disabled={protectedUser || busy}
                    title={protectedUser ? protectedTitle : undefined}
                    className="min-h-11 cursor-pointer font-medium text-destructive hover:underline focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-destructive disabled:cursor-not-allowed disabled:text-muted-foreground disabled:no-underline"
                  >
                    Delete
                  </button>
                </td>
              </tr>
            )
          })}
        </tbody>
      </table>
    </div>
  )
}

interface UserFieldProps {
  id: keyof Pick<NewUser, 'username' | 'email' | 'password'>
  type: 'text' | 'email' | 'password'
  label: string
  value: string
  error: string | undefined
  touched: boolean
  inputRef: RefObject<HTMLInputElement> | null
  onChange: (value: string) => void
  onBlur: () => void
}

function UserField({
  id,
  type,
  label,
  value,
  error,
  touched,
  inputRef,
  onChange,
  onBlur,
}: UserFieldProps) {
  const inputId = `new-user-${id}`
  const errorId = `${inputId}-error`
  const invalid = touched && Boolean(error)
  return (
    <div className="space-y-1">
      <label htmlFor={inputId} className="text-sm font-medium text-foreground">
        {label}
      </label>
      <input
        ref={inputRef}
        id={inputId}
        type={type}
        value={value}
        onChange={(event) => onChange(event.target.value)}
        onBlur={onBlur}
        aria-invalid={invalid}
        aria-describedby={invalid ? errorId : undefined}
        autoComplete={type === 'password' ? 'new-password' : undefined}
        className={`w-full rounded-lg border bg-muted/20 px-4 py-2 outline-none transition-all ${invalid ? 'border-destructive focus:border-destructive' : 'border-input focus:border-primary focus:bg-background'}`}
      />
      {invalid && (
        <p id={errorId} className="ml-1 text-xs font-medium text-destructive">
          {error}
        </p>
      )}
    </div>
  )
}

function UserRoleField({
  role,
  onChange,
}: {
  role: NewUser['role']
  onChange: (role: NewUser['role']) => void
}) {
  return (
    <div className="space-y-1">
      <label htmlFor="new-user-role" className="text-sm font-medium text-foreground">
        Role
      </label>
      <select
        id="new-user-role"
        value={role}
        onChange={(event) => onChange(event.target.value as NewUser['role'])}
        className="w-full rounded-lg border border-input bg-muted/20 px-4 py-2 outline-none transition-all focus:border-primary focus:bg-background"
      >
        <option value="user">User</option>
        <option value="admin">Admin</option>
      </select>
    </div>
  )
}

function CreateUserActions({
  isValid,
  onCancel,
  onCreate,
}: {
  isValid: boolean
  onCancel: () => void
  onCreate: () => void
}) {
  return (
    <div className="mt-8 flex justify-end gap-3">
      <button
        type="button"
        onClick={onCancel}
        className="px-4 py-2 font-medium text-muted-foreground hover:text-foreground"
      >
        Cancel
      </button>
      <button
        type="button"
        onClick={onCreate}
        disabled={!isValid}
        className={`rounded-lg px-6 py-2 font-medium shadow-lg transition-all ${isValid ? 'bg-primary text-primary-foreground hover:bg-primary/90' : 'cursor-not-allowed bg-muted text-muted-foreground shadow-none'}`}
      >
        Create User
      </button>
    </div>
  )
}

function CreateUserModal({
  onClose,
  onCreated,
}: {
  onClose: () => void
  onCreated: () => Promise<void>
}) {
  const [user, setUser] = useState<NewUser>(EMPTY_USER)
  const [touched, setTouched] = useState<Record<string, boolean>>({})
  const [serverError, setServerError] = useState<string | null>(null)
  const usernameInputRef = useRef<HTMLInputElement>(null)
  const errors = validateNewUser(user)
  const isValid = Object.keys(errors).length === 0

  useEffect(() => {
    usernameInputRef.current?.focus()
    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key === 'Escape') onClose()
    }
    window.addEventListener('keydown', closeOnEscape)
    return () => window.removeEventListener('keydown', closeOnEscape)
  }, [onClose])

  const createUser = async () => {
    setTouched({ username: true, email: true, password: true })
    if (!isValid) return
    try {
      await adminApi.createUser(user)
      await onCreated()
      onClose()
    } catch (error) {
      const detail = error instanceof AxiosError ? error.response?.data?.detail : null
      setServerError(detail || (error instanceof Error ? error.message : 'Failed to create user'))
    }
  }

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-background/80 p-4 backdrop-blur-sm">
      <div
        role="dialog"
        aria-modal="true"
        aria-labelledby="add-user-title"
        className="w-full max-w-md rounded-xl border border-border/50 bg-card p-6 shadow-2xl animate-scale-in"
      >
        <h3 id="add-user-title" className="mb-6 font-display text-xl font-medium">
          Add User
        </h3>
        {serverError && (
          <div
            role="alert"
            className="mb-4 flex items-center gap-2 rounded-lg border border-destructive/20 bg-destructive/10 p-3 text-sm text-destructive"
          >
            <AlertCircle className="h-4 w-4 shrink-0" />
            {serverError}
          </div>
        )}
        <div className="space-y-4">
          <UserField
            id="username"
            type="text"
            label="Username"
            value={user.username}
            error={errors.username}
            touched={Boolean(touched.username)}
            inputRef={usernameInputRef}
            onChange={(username) => setUser({ ...user, username })}
            onBlur={() => setTouched({ ...touched, username: true })}
          />
          <UserField
            id="email"
            type="email"
            label="Email"
            value={user.email}
            error={errors.email}
            touched={Boolean(touched.email)}
            inputRef={null}
            onChange={(email) => setUser({ ...user, email })}
            onBlur={() => setTouched({ ...touched, email: true })}
          />
          <UserField
            id="password"
            type="password"
            label="Password"
            value={user.password}
            error={errors.password}
            touched={Boolean(touched.password)}
            inputRef={null}
            onChange={(password) => setUser({ ...user, password })}
            onBlur={() => setTouched({ ...touched, password: true })}
          />
          <UserRoleField role={user.role} onChange={(role) => setUser({ ...user, role })} />
        </div>
        <CreateUserActions
          isValid={isValid}
          onCancel={onClose}
          onCreate={() => void createUser()}
        />
      </div>
    </div>
  )
}

export default function UserManagement() {
  const { user: currentUser } = useAuth()
  const managedUsers = useManagedUsers(currentUser?.id)
  const [showCreateModal, setShowCreateModal] = useState(false)
  const [pendingDeleteUser, setPendingDeleteUser] = useState<User | null>(null)
  if (managedUsers.isLoading) return <div className="text-muted-foreground">Loading users...</div>
  return (
    <div>
      <div className="mb-6 flex items-center justify-between">
        <h3 className="text-lg font-medium text-foreground">Users</h3>
        <button
          type="button"
          onClick={() => setShowCreateModal(true)}
          className="min-h-11 cursor-pointer rounded-lg bg-primary px-4 py-2 text-sm font-medium text-primary-foreground shadow-sm transition-colors duration-200 hover:bg-primary/90 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary"
        >
          Add User
        </button>
      </div>
      {managedUsers.actionError && (
        <p role="alert" className="mb-4 text-sm text-destructive">
          {managedUsers.actionError}
        </p>
      )}
      <UsersTable
        users={managedUsers.users}
        currentUserId={currentUser?.id}
        busyUserIds={managedUsers.busyUserIds}
        onToggleActive={(user) => void managedUsers.toggleActive(user)}
        onChangeRole={(user, role) => void managedUsers.changeRole(user, role)}
        onDelete={setPendingDeleteUser}
      />
      {showCreateModal && (
        <CreateUserModal
          onClose={() => setShowCreateModal(false)}
          onCreated={managedUsers.loadUsers}
        />
      )}
      {pendingDeleteUser && (
        <ConfirmationDialog
          title={`Delete ${pendingDeleteUser.username}?`}
          description="This permanently removes the user account and cannot be undone."
          confirmLabel="Delete user"
          isProcessing={managedUsers.busyUserIds.has(pendingDeleteUser.id)}
          destructive
          onConfirm={() => {
            void managedUsers
              .deleteUser(pendingDeleteUser)
              .finally(() => setPendingDeleteUser(null))
          }}
          onCancel={() => setPendingDeleteUser(null)}
        />
      )}
    </div>
  )
}
