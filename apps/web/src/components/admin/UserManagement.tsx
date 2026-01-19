import { useState, useEffect } from 'react'
import { adminApi } from '../../api/admin'

interface User {
  id: number
  username: string
  email: string
  role: 'admin' | 'user'
  must_change_password: boolean
  is_active: boolean
  created_at: string
}

export default function UserManagement() {
  const [users, setUsers] = useState<User[]>([])
  const [isLoading, setIsLoading] = useState(true)
  const [showCreateModal, setShowCreateModal] = useState(false)
  const [newUser, setNewUser] = useState({ username: '', email: '', password: '', role: 'user' as 'admin' | 'user' })

  const loadUsers = async () => {
    try {
      const data = await adminApi.listUsers()
      setUsers(data)
    } catch {
      console.error('Failed to load users')
    } finally {
      setIsLoading(false)
    }
  }

  useEffect(() => {
    loadUsers()
  }, [])

  const handleCreate = async () => {
    if (!newUser.username || !newUser.email || !newUser.password) return
    try {
      await adminApi.createUser(newUser)
      setNewUser({ username: '', email: '', password: '', role: 'user' })
      setShowCreateModal(false)
      loadUsers()
    } catch {
      alert('Failed to create user')
    }
  }

  const handleToggleActive = async (user: User) => {
    try {
      await adminApi.updateUser(user.id, { is_active: !user.is_active })
      loadUsers()
    } catch {
      alert('Failed to update user')
    }
  }

  const handleDelete = async (userId: number) => {
    if (!confirm('Delete this user? This cannot be undone.')) return
    try {
      await adminApi.deleteUser(userId)
      loadUsers()
    } catch {
      alert('Failed to delete user')
    }
  }

  if (isLoading) {
    return <div className="text-gray-500">Loading users...</div>
  }

  return (
    <div>
      <div className="flex justify-between items-center mb-6">
        <h3 className="text-lg font-medium text-foreground">Users</h3>
        <button
          onClick={() => setShowCreateModal(true)}
          className="bg-primary text-primary-foreground px-4 py-2 rounded-lg hover:bg-primary/90 shadow-sm font-medium text-sm transition-all hover:shadow-primary/20"
        >
          Add User
        </button>
      </div>

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
                  <span className={`px-2 py-1 rounded text-xs font-bold uppercase tracking-wide ${user.is_active ? 'bg-primary/10 text-primary border border-primary/20' : 'bg-destructive/10 text-destructive border border-destructive/20'}`}>
                    {user.is_active ? 'Active' : 'Inactive'}
                  </span>
                </td>
                <td className="px-4 py-3">
                  <button
                    onClick={() => handleToggleActive(user)}
                    className="text-primary hover:text-primary/80 mr-3 font-medium hover:underline"
                  >
                    {user.is_active ? 'Deactivate' : 'Activate'}
                  </button>
                  <button
                    onClick={() => handleDelete(user.id)}
                    className="text-destructive hover:text-destructive/80 font-medium hover:underline"
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
          <div className="bg-card border border-border/50 rounded-xl shadow-2xl p-6 w-full max-w-md animate-scale-in">
            <h3 className="text-xl font-display font-medium mb-6">Add User</h3>
            <div className="space-y-4">
              <input
                type="text"
                placeholder="Username"
                value={newUser.username}
                onChange={(e) => setNewUser({ ...newUser, username: e.target.value })}
                className="w-full px-4 py-2 bg-muted/20 border border-input rounded-lg focus:border-primary focus:bg-background outline-none transition-all"
              />
              <input
                type="email"
                placeholder="Email"
                value={newUser.email}
                onChange={(e) => setNewUser({ ...newUser, email: e.target.value })}
                className="w-full px-4 py-2 bg-muted/20 border border-input rounded-lg focus:border-primary focus:bg-background outline-none transition-all"
              />
              <input
                type="password"
                placeholder="Password"
                value={newUser.password}
                onChange={(e) => setNewUser({ ...newUser, password: e.target.value })}
                className="w-full px-4 py-2 bg-muted/20 border border-input rounded-lg focus:border-primary focus:bg-background outline-none transition-all"
              />
              <select
                value={newUser.role}
                onChange={(e) => setNewUser({ ...newUser, role: e.target.value as 'admin' | 'user' })}
                className="w-full px-4 py-2 bg-muted/20 border border-input rounded-lg focus:border-primary focus:bg-background outline-none transition-all"
              >
                <option value="user">User</option>
                <option value="admin">Admin</option>
              </select>
            </div>
            <div className="flex justify-end gap-3 mt-8">
              <button onClick={() => setShowCreateModal(false)} className="px-4 py-2 text-muted-foreground hover:text-foreground transition-colors font-medium">
                Cancel
              </button>
              <button onClick={handleCreate} className="px-6 py-2 bg-primary text-primary-foreground rounded-lg hover:bg-primary/90 font-medium shadow-lg shadow-primary/20">
                Create User
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  )
}
