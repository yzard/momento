import { Cloud, Database, FileText, ShieldCheck, Users } from 'lucide-react'
import { Outlet } from 'react-router-dom'

import AiPanel from '../../components/admin/AiPanel'
import { AdminPanel } from '../../components/admin/AdminComponents'
import ImportPanel from '../../components/admin/ImportPanel'
import MetadataPanel from '../../components/admin/MetadataPanel'
import UserManagement from '../../components/admin/UserManagement'

export default function AdminLayout() {
  return (
    <div className="flex-1 overflow-y-auto scrollbar-thin scrollbar-thumb-muted-foreground/20 scrollbar-track-transparent">
      <div className="mx-auto max-w-7xl px-6 py-8 md:px-10 animate-fade-in">
        <header className="mb-10 flex flex-col gap-4 sm:flex-row sm:items-end sm:justify-between">
          <div>
            <h1 className="font-display text-3xl font-bold tracking-tight text-foreground">
              Admin
            </h1>
            <p className="mt-1 font-medium text-muted-foreground">
              System access, imports, metadata, and AI processing.
            </p>
          </div>
          <div className="flex w-fit items-center gap-2 rounded-md border border-primary/20 bg-primary/10 px-3 py-1.5 text-xs font-bold uppercase tracking-widest text-primary">
            <ShieldCheck aria-hidden="true" className="h-4 w-4" />
            System Access
          </div>
        </header>
        <Outlet />
      </div>
    </div>
  )
}

export function AdminImportPage() {
  const webDAVURL = new URL('/webdav/', window.location.origin).toString()

  return (
    <div className="space-y-6">
      <AdminPanel
        icon={Cloud}
        title="WebDAV"
        description="Upload media through a WebDAV client using your Momento credentials."
      >
        <dl className="space-y-3 text-sm">
          <div>
            <dt className="font-semibold text-foreground">WebDAV URL</dt>
            <dd className="mt-1 break-all font-mono text-xs text-muted-foreground">{webDAVURL}</dd>
          </div>
          <div>
            <dt className="font-semibold text-foreground">Authentication</dt>
            <dd className="mt-1 text-muted-foreground">
              Use the same username and password used to sign in to Momento.
            </dd>
          </div>
        </dl>
      </AdminPanel>
      <AdminPanel
        icon={Database}
        title="Local Import"
        description="Import media staged in the server import directory."
      >
        <p className="mb-5 text-sm text-muted-foreground">
          Place media in <code className="font-mono text-xs text-foreground">/data/imports/</code>.
        </p>
        <ImportPanel />
      </AdminPanel>
    </div>
  )
}

export function AdminMetadataPage() {
  return (
    <AdminPanel
      icon={FileText}
      title="Metadata"
      description="Generate thumbnails and technical metadata before AI processing."
    >
      <MetadataPanel />
    </AdminPanel>
  )
}

export function AdminAIPage() {
  return (
    <div aria-label="AI administration">
      <AiPanel />
    </div>
  )
}

export function AdminUsersPage() {
  return (
    <AdminPanel
      icon={Users}
      title="User Management"
      description="Manage sign-in access and administrator permissions."
    >
      <UserManagement />
    </AdminPanel>
  )
}
