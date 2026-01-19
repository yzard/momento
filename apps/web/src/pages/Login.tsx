import { useState, type FormEvent } from 'react'
import { useNavigate } from 'react-router-dom'
import { useAuth } from '../hooks/useAuth'
import ChangePassword from '../components/auth/ChangePassword'
import { cn } from '../lib/utils'
import { ArrowRight, Loader2, Aperture } from 'lucide-react'

export default function Login() {
  const [username, setUsername] = useState('')
  const [password, setPassword] = useState('')
  const [error, setError] = useState('')
  const [isLoading, setIsLoading] = useState(false)
  const [showChangePassword, setShowChangePassword] = useState(false)
  const { login } = useAuth()
  const navigate = useNavigate()

  const handleSubmit = async (e: FormEvent) => {
    e.preventDefault()
    setError('')
    setIsLoading(true)

    try {
      const user = await login(username, password)
      if (user.mustChangePassword) {
        setShowChangePassword(true)
        setPassword('')
        return
      }
      navigate('/')
    } catch {
      setError('Invalid username or password')
    } finally {
      setIsLoading(false)
    }
  }

  return (
    <div className="min-h-screen flex w-full font-sans bg-background text-foreground overflow-hidden selection:bg-primary/20 selection:text-foreground">
      {/* Visual Section */}
      <div className="hidden lg:flex lg:w-1/2 relative items-center justify-center p-12 bg-muted/20 border-r border-border">
        <div className="relative z-10 max-w-lg">
          <div className="mb-12 inline-flex p-4 rounded-full bg-white shadow-sm border border-border">
            <Aperture className="w-10 h-10 text-primary" strokeWidth={1.5} />
          </div>
          <h1 className="text-7xl font-display font-semibold mb-8 tracking-tighter leading-[0.9] text-foreground">
            Momento.
          </h1>
          <p className="text-2xl text-muted-foreground font-light leading-relaxed max-w-md">
            Your personal gallery.<br/>
            <span className="text-foreground font-normal">Private. Secure. Yours.</span>
          </p>
        </div>
      </div>

      {/* Login Form Section */}
      <div className="flex-1 flex items-center justify-center bg-background p-8 lg:p-16">
        <div className="w-full max-w-sm space-y-10 animate-fade-in">
          <div className="space-y-1">
            <h2 className="text-2xl font-bold tracking-tight text-foreground">Sign In</h2>
            <p className="text-muted-foreground">Welcome back to your memories.</p>
          </div>

          <form onSubmit={handleSubmit} className="space-y-8">
            {error && (
              <div className="bg-destructive/5 text-destructive px-4 py-3 text-sm rounded-md flex items-center gap-3 border border-destructive/10 font-medium">
                <span className="font-bold">!</span> {error}
              </div>
            )}
            
            <div className="space-y-6">
              <div className="space-y-2 group">
                <label htmlFor="username" className="text-xs font-bold uppercase tracking-widest text-muted-foreground group-focus-within:text-foreground transition-colors">
                  Username
                </label>
                <input
                  id="username"
                  type="text"
                  value={username}
                  onChange={(e) => setUsername(e.target.value)}
                  className="w-full px-4 py-3 bg-white border border-input focus:border-primary outline-none transition-all text-base rounded-lg focus:ring-4 focus:ring-primary/10 shadow-sm"
                  placeholder="Enter your username"
                  required
                />
              </div>
              
              <div className="space-y-2 group">
                <label htmlFor="password" className="text-xs font-bold uppercase tracking-widest text-muted-foreground group-focus-within:text-foreground transition-colors">
                  Password
                </label>
                <input
                  id="password"
                  type="password"
                  value={password}
                  onChange={(e) => setPassword(e.target.value)}
                  className="w-full px-4 py-3 bg-white border border-input focus:border-primary outline-none transition-all text-base rounded-lg focus:ring-4 focus:ring-primary/10 shadow-sm"
                  placeholder="••••••••"
                  required
                />
              </div>
            </div>

            <button
              type="submit"
              disabled={isLoading}
              className={cn(
                "w-full bg-foreground text-background py-3.5 font-bold text-sm uppercase tracking-widest hover:bg-foreground/90 transition-all rounded-lg flex items-center justify-center gap-3 shadow-lg hover:shadow-xl focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-offset-2 focus-visible:ring-foreground",
                isLoading && "opacity-70 cursor-not-allowed"
              )}
            >
              {isLoading ? (
                <>
                  <Loader2 className="w-4 h-4 animate-spin" />
                  CONNECTING...
                </>
              ) : (
                <>
                  SIGN IN <ArrowRight className="w-4 h-4" />
                </>
              )}
            </button>
          </form>
          
          <div className="pt-8 text-center">
            <p className="text-xs font-medium text-muted-foreground/60 uppercase tracking-widest">
              Momento v0.1
            </p>
          </div>
        </div>
      </div>

      {showChangePassword && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-background/80 backdrop-blur-sm px-4">
          <div className="bg-white shadow-2xl border border-border w-full max-w-md p-8 rounded-2xl animate-scale-in">
            <h2 className="text-2xl font-bold font-display mb-2 text-foreground">Change Password</h2>
            <p className="text-sm text-muted-foreground mb-8 pb-4 border-b border-border">
              Security check. Please update your password to continue.
            </p>
            <ChangePassword
              onComplete={() => {
                setShowChangePassword(false)
                navigate('/')
              }}
            />
          </div>
        </div>
      )}
    </div>
  )
}

