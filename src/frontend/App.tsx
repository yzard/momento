import { lazy, Suspense } from 'react'
import { Routes, Route, Navigate } from 'react-router-dom'
import { AuthProvider } from './context/AuthContext'
import { ThemeProvider } from './context/ThemeContext'
import { useAuth } from './hooks/useAuth'
import Layout from './components/layout/Layout'

const Login = lazy(() => import('./pages/Login'))
const Timeline = lazy(() => import('./pages/Timeline'))
const Albums = lazy(() => import('./pages/Albums'))
const Map = lazy(() => import('./pages/Map'))
const Settings = lazy(() => import('./pages/Settings'))
const Trash = lazy(() => import('./pages/Trash'))
const Deduplicate = lazy(() => import('./pages/Deduplicate'))
const Faces = lazy(() => import('./pages/Faces'))
const Places = lazy(() => import('./pages/Places'))

function LoadingScreen() {
  return <div className="flex items-center justify-center h-screen">Loading...</div>
}

function ProtectedRoute({ children }: { children: React.ReactNode }) {
  const { isAuthenticated, isLoading } = useAuth()

  if (isLoading) {
    return <LoadingScreen />
  }

  if (!isAuthenticated) {
    return <Navigate to="/login" replace />
  }

  return <>{children}</>
}

function AppRoutes() {
  return (
    <Routes>
      <Route path="/login" element={<Login />} />
      <Route
        path="/"
        element={
          <ProtectedRoute>
            <Layout />
          </ProtectedRoute>
        }
      >
        <Route index element={<Navigate to="/timeline" replace />} />
        <Route path="timeline" element={<Timeline mediaType={null} classification={null} />} />
        <Route path="timeline/photos" element={<Timeline mediaType="image" classification={null} />} />
        <Route path="timeline/videos" element={<Timeline mediaType="video" classification={null} />} />
        <Route path="timeline/screenshots" element={<Timeline mediaType="image" classification="screenshot" />} />
        <Route path="timeline/documents" element={<Timeline mediaType="image" classification="document" />} />
        <Route path="albums" element={<Albums />} />
        <Route path="map" element={<Map />} />
        <Route path="places" element={<Places />} />
        <Route path="places/:placeId" element={<Places />} />
        <Route path="faces" element={<Faces />} />
        <Route path="faces/:faceGroupId" element={<Faces />} />
        <Route path="utility" element={<Navigate to="/utility/deduplicate" replace />} />
        <Route path="utility/deduplicate" element={<Deduplicate />} />
        <Route path="settings" element={<Settings />} />
        <Route path="admin" element={<Navigate to="/settings" replace />} />
        <Route path="trash" element={<Trash />} />
      </Route>
    </Routes>
  )
}

export default function App() {
  return (
    <ThemeProvider>
      <AuthProvider>
        <Suspense fallback={<LoadingScreen />}>
          <AppRoutes />
        </Suspense>
      </AuthProvider>
    </ThemeProvider>
  )
}
