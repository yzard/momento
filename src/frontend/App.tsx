import { Routes, Route, Navigate } from 'react-router-dom'
import { AuthProvider } from './context/AuthContext'
import { useAuth } from './hooks/useAuth'
import Layout from './components/layout/Layout'
import Login from './pages/Login'
import Timeline from './pages/Timeline'
import Albums from './pages/Albums'
import Map from './pages/Map'
import Settings from './pages/Settings'
import Admin from './pages/Admin'
import Trash from './pages/Trash'
import Deduplicate from './pages/Deduplicate'
import Faces from './pages/Faces'

function ProtectedRoute({ children }: { children: React.ReactNode }) {
  const { isAuthenticated, isLoading } = useAuth()

  if (isLoading) {
    return <div className="flex items-center justify-center h-screen">Loading...</div>
  }

  if (!isAuthenticated) {
    return <Navigate to="/login" replace />
  }

  return <>{children}</>
}

function AdminRoute({ children }: { children: React.ReactNode }) {
  const { user, isLoading } = useAuth()

  if (isLoading) {
    return <div className="flex items-center justify-center h-screen">Loading...</div>
  }

  if (!user || user.role !== 'admin') {
    return <Navigate to="/timeline" replace />
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
        <Route path="timeline" element={<Timeline mediaType={null} />} />
        <Route path="timeline/photos" element={<Timeline mediaType="image" />} />
        <Route path="timeline/videos" element={<Timeline mediaType="video" />} />
        <Route path="albums" element={<Albums />} />
        <Route path="map" element={<Map />} />
        <Route path="faces" element={<Faces />} />
        <Route path="faces/:faceGroupId" element={<Faces />} />
        <Route path="utility" element={<Navigate to="/utility/deduplicate" replace />} />
        <Route path="utility/deduplicate" element={<Deduplicate />} />
        <Route path="settings" element={<Settings />} />
        <Route path="trash" element={<Trash />} />
        <Route
          path="admin"
          element={
            <AdminRoute>
              <Admin />
            </AdminRoute>
          }
        />
      </Route>
    </Routes>
  )
}

export default function App() {
  return (
    <AuthProvider>
      <AppRoutes />
    </AuthProvider>
  )
}
