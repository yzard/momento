import { useState, useEffect, useRef } from 'react'
import { AxiosError } from 'axios'
import { adminApi } from '../../api/admin'
import type { User } from '../../api/auth'
import { AlertCircle } from 'lucide-react'

export default function UserManagement() {
  const [users, setUsers] = useState<User[]>([])
  const [isLoading, setIsLoading] = useState(true)
  const [showCreateModal, setShowCreateModal] = useState(false)
  const [newUser, setNewUser] = useState({ username: '', email: '', password: '', role: 'user' as 'admin' | 'user' })
  
  const [touched, setTouched] = useState<Record<string, boolean>>({})
  const [serverError, setServerError] = useState<string | null>(null)
  const [actionError, setActionError] = useState<string | null>(null)
  const usernameInputRef = useRef<HTMLInputElement>(null)

  const validate = (data: typeof newUser) => {
    const errors: Record<string, string> = {}
    if (!data.username) errors.username = 'Username is required'
    if (!data.email) errors.email = 'Email is required'
    if (!data.password) {
      errors.password = 'Password is required'
    } else if (data.password.length < 8) {
      errors.password = 'Password must be at least 8 characters'
    }
    return errors
  }

  const errors = validate(newUser)
  const isValid = Object.keys(errors).length === 0

  const loadUsers = async () => {
    try {
      const users = await adminApi.listUsers()
      setUsers(users)
      setActionError(null)
    } catch {
      setActionError('Could not load users.')
    } finally {
      setIsLoading(false)
    }
  }

  useEffect(() => {
    void loadUsers()
  }, [])

  useEffect(() => {
    if (!showCreateModal) {
      setNewUser({ username: '', email: '', password: '', role: 'user' })
      setTouched({})
      setServerError(null)
    }
  }, [showCreateModal])

  useEffect(() => {
    if (!showCreateModal) return
    usernameInputRef.current?.focus()
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') setShowCreateModal(false)
    }
    window.addEventListener('keydown', handleKeyDown)
    return () => window.removeEventListener('keydown', handleKeyDown)
  }, [showCreateModal])

  const handleCreate = async () => {
    setTouched({
      username: true,
      email: true,
      password: true
    })

    if (!isValid) return

    setServerError(null)
    
    try {
      await adminApi.createUser(newUser)
      setShowCreateModal(false)
      loadUsers()
    } catch (error) {
      if (error instanceof AxiosError && error.response?.data?.detail) {
        setServerError(error.response.data.detail)
      } else {
        const message = error instanceof Error ? error.message : 'Failed to create user'
        setServerError(message)
      }
    }
  }

  const handleToggleActive = async (user: User) => {
    try {
      await adminApi.updateUser({ userId: user.id, isActive: !user.isActive })
      loadUsers()
    } catch {
      setActionError('Could not update the user.')
    }
  }

  const handleDelete = async (userId: number) => {
    if (!confirm('Delete this user? This cannot be undone.')) return
    try {
      await adminApi.deleteUser(userId)
      loadUsers()
    } catch {
      setActionError('Could not delete the user.')
    }
  }

  if (isLoading) {
    return <div className="text-muted-foreground">Loading users...</div>
  }

  return (
    <div>
      <div className="flex justify-between items-center mb-6">
        <h3 className="text-lg font-medium text-foreground">Users</h3>
        <button
          type="button"
          onClick={() => setShowCreateModal(true)}
          className="bg-primary text-primary-foreground px-4 py-2 rounded-lg hover:bg-primary/90 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary shadow-sm font-medium text-sm transition-all hover:shadow-primary/20"
        >
          Add User
        </button>
      </div>
      {actionError && <p role="alert" className="mb-4 text-sm text-destructive">{actionError}</p>}

      <div className="bg-card/30 rounded-xl border border-border/50 overflow-hidden backdrop-blur-sm">
        <table className="w-full text-sm">
          <thead className="bg-muted/30">
            <tr>
              <th className="px-4 py-3 text-left font-medium text-muted-foreground uppercase tracking-wider text-xs">Username</th>
              <th className="px-4 py-3 text-left font-medium text-muted-foreground uppercase tracking-wider text-xs">Email</th>
              <th className="px-4 py-3 text-left font-medium text-muted-foreground uppercase tracking-wider text-xs">Role</th>
              <th className="px-4 py-3 text-left font-medium text-muted-foreground uppercase tracking-wider text-xs">Status</th>
              <th className="px-4 py-3 text-left font-medium text-muted-foreground uppercase tracking-wider text-xs">Actions</th>
            </tr>
          </thead>
          <tbody className="divide-y divide-border/50">
            {users.map((user) => (
              <tr key={user.id} className="hover:bg-muted/20 transition-colors">
                <td className="px-4 py-3 font-medium text-foreground">{user.username}</td>
                <td className="px-4 py-3 text-muted-foreground">{user.email}</td>
                <td className="px-4 py-3">
                  <span className={`px-2 py-1 rounded text-xs font-bold uppercase tracking-wide ${user.role === 'admin' ? 'bg-secondary/10 text-secondary border border-secondary/20' : 'bg-muted text-muted-foreground border border-border'}`}>
                    {user.role}
                  </span>
                </td>
                <td className="px-4 py-3">
                  <span className={`px-2 py-1 rounded text-xs font-bold uppercase tracking-wide ${user.isActive ? 'bg-primary/10 text-primary border border-primary/20' : 'bg-destructive/10 text-destructive border border-destructive/20'}`}>
                    {user.isActive ? 'Active' : 'Inactive'}
                  </span>
                </td>
                <td className="px-4 py-3">
                  <button
                    type="button"
                    onClick={() => handleToggleActive(user)}
                    className="mr-3 text-primary hover:text-primary/80 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary font-medium hover:underline"
                  >
                    {user.isActive ? 'Deactivate' : 'Activate'}
                  </button>
                  <button
                    type="button"
                    onClick={() => handleDelete(user.id)}
                    className="text-destructive hover:text-destructive/80 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-destructive font-medium hover:underline"
                  >
                    Delete
                  </button>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>

      {showCreateModal && (
        <div className="fixed inset-0 bg-background/80 backdrop-blur-sm flex items-center justify-center z-50 p-4">
          <div role="dialog" aria-modal="true" aria-labelledby="add-user-title" className="bg-card border border-border/50 rounded-xl shadow-2xl p-6 w-full max-w-md animate-scale-in">
            <h3 id="add-user-title" className="text-xl font-display font-medium mb-6">Add User</h3>
            
            {serverError && (
              <div className="mb-4 p-3 bg-destructive/10 border border-destructive/20 rounded-lg flex items-center gap-2 text-destructive text-sm">
                <AlertCircle className="w-4 h-4 shrink-0" />
                <span>{serverError}</span>
              </div>
            )}

            <div className="space-y-4">
              <div className="space-y-1">
                <label htmlFor="new-user-username" className="text-sm font-medium text-foreground">Username</label>
                <input
                  ref={usernameInputRef}
                  id="new-user-username"
                  type="text"
                  value={newUser.username}
                  onChange={(e) => setNewUser({ ...newUser, username: e.target.value })}
                  onBlur={() => setTouched({ ...touched, username: true })}
                  aria-invalid={touched.username && Boolean(errors.username)}
                  aria-describedby={touched.username && errors.username ? 'new-user-username-error' : undefined}
                  className={`w-full px-4 py-2 bg-muted/20 border rounded-lg outline-none transition-all ${
                    touched.username && errors.username 
                      ? 'border-destructive focus:border-destructive' 
                      : 'border-input focus:border-primary focus:bg-background'
                  }`}
                />
                {touched.username && errors.username && (
                  <p id="new-user-username-error" className="text-xs text-destructive font-medium ml-1">{errors.username}</p>
                )}
              </div>

              <div className="space-y-1">
                <label htmlFor="new-user-email" className="text-sm font-medium text-foreground">Email</label>
                <input
                  id="new-user-email"
                  type="email"
                  placeholder="Email"
                  value={newUser.email}
                  onChange={(e) => setNewUser({ ...newUser, email: e.target.value })}
                  onBlur={() => setTouched({ ...touched, email: true })}
                  aria-invalid={touched.email && Boolean(errors.email)}
                  aria-describedby={touched.email && errors.email ? 'new-user-email-error' : undefined}
                  className={`w-full px-4 py-2 bg-muted/20 border rounded-lg outline-none transition-all ${
                    touched.email && errors.email 
                      ? 'border-destructive focus:border-destructive' 
                      : 'border-input focus:border-primary focus:bg-background'
                  }`}
                />
                {touched.email && errors.email && (
                  <p id="new-user-email-error" className="text-xs text-destructive font-medium ml-1">{errors.email}</p>
                )}
              </div>

              <div className="space-y-1">
                <label htmlFor="new-user-password" className="text-sm font-medium text-foreground">Password</label>
                <input
                  id="new-user-password"
                  type="password"
                  placeholder="Password"
                  value={newUser.password}
                  onChange={(e) => setNewUser({ ...newUser, password: e.target.value })}
                  onBlur={() => setTouched({ ...touched, password: true })}
                  aria-invalid={touched.password && Boolean(errors.password)}
                  aria-describedby={touched.password && errors.password ? 'new-user-password-error' : undefined}
                  className={`w-full px-4 py-2 bg-muted/20 border rounded-lg outline-none transition-all ${
                    touched.password && errors.password 
                      ? 'border-destructive focus:border-destructive' 
                      : 'border-input focus:border-primary focus:bg-background'
                  }`}
                />
                {touched.password && errors.password && (
                  <p id="new-user-password-error" className="text-xs text-destructive font-medium ml-1">{errors.password}</p>
                )}
              </div>

              <div className="space-y-1">
              <label htmlFor="new-user-role" className="text-sm font-medium text-foreground">Role</label>
              <select
                id="new-user-role"
                value={newUser.role}
                onChange={(e) => setNewUser({ ...newUser, role: e.target.value as 'admin' | 'user' })}
                className="w-full px-4 py-2 bg-muted/20 border border-input rounded-lg focus:border-primary focus:bg-background outline-none transition-all"
              >
                <option value="user">User</option>
                <option value="admin">Admin</option>
              </select>
              </div>
            </div>
            <div className="flex justify-end gap-3 mt-8">
              <button type="button" onClick={() => setShowCreateModal(false)} className="px-4 py-2 text-muted-foreground hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary transition-colors font-medium">
                Cancel
              </button>
              <button 
                type="button"
                onClick={handleCreate} 
                disabled={!isValid}
                className={`px-6 py-2 rounded-lg font-medium shadow-lg transition-all ${
                  isValid 
                    ? 'bg-primary text-primary-foreground hover:bg-primary/90 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary shadow-primary/20'
                    : 'bg-muted text-muted-foreground cursor-not-allowed shadow-none'
                }`}
              >
                Create User
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  )
}
