import { useState, type FormEvent } from 'react'
import { useNavigate } from 'react-router-dom'
import { useAuth } from '../hooks/useAuth'
import PasswordChangeForm from '../components/auth/PasswordChangeForm'
import { cn } from '../lib/utils'
import { MOMENTO_VERSION } from '../lib/version'
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
      <LoginBrand />
      <div className="flex-1 flex items-center justify-center bg-background p-8 lg:p-16">
        <div className="w-full max-w-sm space-y-10 animate-fade-in">
          <LoginForm
            username={username}
            password={password}
            error={error}
            isLoading={isLoading}
            onUsernameChange={setUsername}
            onPasswordChange={setPassword}
            onSubmit={handleSubmit}
          />
          <div className="pt-8 text-center">
            <p className="text-xs font-medium text-muted-foreground/60 uppercase tracking-widest">
              v{MOMENTO_VERSION}
            </p>
          </div>
        </div>
      </div>

      {showChangePassword && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-background/80 backdrop-blur-sm px-4">
          <div className="bg-card text-card-foreground shadow-2xl border border-border w-full max-w-md p-8 rounded-2xl animate-scale-in">
            <h2 className="text-2xl font-bold font-display mb-2 text-foreground">
              Change Password
            </h2>
            <p className="text-sm text-muted-foreground mb-8 pb-4 border-b border-border">
              Security check. Please update your password to continue.
            </p>
            <PasswordChangeForm
              layout="modal"
              onComplete={() => {
                setShowChangePassword(false)
              }}
            />
          </div>
        </div>
      )}
    </div>
  )
}

function LoginBrand() {
  return (
    <div className="relative hidden items-center justify-center border-r border-border bg-muted/20 p-12 lg:flex lg:w-1/2">
      <div className="relative z-10 max-w-lg">
        <div className="mb-12 inline-flex rounded-full border border-border bg-card p-4 shadow-sm">
          <Aperture className="h-10 w-10 text-primary" strokeWidth={1.5} />
        </div>
        <h1 className="mb-8 font-display text-7xl font-semibold leading-[0.9] tracking-tighter text-foreground">
          Momento.
        </h1>
        <p className="max-w-md text-2xl font-light leading-relaxed text-muted-foreground">
          Your personal gallery.
          <br />
          <span className="font-normal text-foreground">Private. Secure. Yours.</span>
        </p>
      </div>
    </div>
  )
}

interface LoginFormProps {
  username: string
  password: string
  error: string
  isLoading: boolean
  onUsernameChange: (value: string) => void
  onPasswordChange: (value: string) => void
  onSubmit: (event: FormEvent) => void
}

function LoginForm(props: LoginFormProps) {
  const inputClassName =
    'w-full rounded-lg border border-input bg-card px-4 py-3 text-base text-foreground shadow-sm outline-none transition-all focus:border-primary focus:ring-4 focus:ring-primary/10'
  return (
    <>
      <div className="space-y-1">
        <h2 className="text-2xl font-bold tracking-tight text-foreground">Sign In</h2>
        <p className="text-muted-foreground">Welcome back to your memories.</p>
      </div>
      <form onSubmit={props.onSubmit} className="space-y-8">
        {props.error && (
          <div
            role="alert"
            className="flex items-center gap-3 rounded-md border border-destructive/10 bg-destructive/5 px-4 py-3 text-sm font-medium text-destructive"
          >
            <span className="font-bold">!</span>
            {props.error}
          </div>
        )}
        <div className="space-y-6">
          <div className="group space-y-2">
            <label
              htmlFor="username"
              className="text-xs font-bold uppercase tracking-widest text-muted-foreground"
            >
              Username
            </label>
            <input
              id="username"
              type="text"
              value={props.username}
              onChange={(event) => props.onUsernameChange(event.target.value)}
              className={inputClassName}
              placeholder="Enter your username"
              autoComplete="username"
              required
            />
          </div>
          <div className="group space-y-2">
            <label
              htmlFor="password"
              className="text-xs font-bold uppercase tracking-widest text-muted-foreground"
            >
              Password
            </label>
            <input
              id="password"
              type="password"
              value={props.password}
              onChange={(event) => props.onPasswordChange(event.target.value)}
              className={inputClassName}
              placeholder="••••••••"
              autoComplete="current-password"
              required
            />
          </div>
        </div>
        <button
          type="submit"
          disabled={props.isLoading}
          className={cn(
            'flex w-full items-center justify-center gap-3 rounded-lg bg-foreground py-3.5 text-sm font-bold uppercase tracking-widest text-background shadow-lg transition-all hover:bg-foreground/90',
            props.isLoading && 'cursor-not-allowed opacity-70'
          )}
        >
          {props.isLoading ? (
            <>
              <Loader2 className="h-4 w-4 animate-spin" />
              CONNECTING...
            </>
          ) : (
            <>
              SIGN IN <ArrowRight className="h-4 w-4" />
            </>
          )}
        </button>
      </form>
    </>
  )
}
