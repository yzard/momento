import ImportPanel from '../components/admin/ImportPanel'
import MetadataPanel from '../components/admin/MetadataPanel'
import UserManagement from '../components/admin/UserManagement'
import { Database, Users, ShieldCheck, FileText } from 'lucide-react'

export default function Admin() {
  return (
    <div className="flex-1 overflow-y-auto scrollbar-thin scrollbar-thumb-muted-foreground/20 scrollbar-track-transparent">
    <div className="max-w-7xl mx-auto animate-fade-in px-6 md:px-10 py-6 md:py-10">
      <div className="mb-12 flex flex-col sm:flex-row sm:items-end justify-between gap-4">
        <div>
          <h1 className="text-3xl font-display font-bold text-foreground tracking-tight">Admin Console</h1>
          <p className="mt-1 text-muted-foreground font-medium">System configuration and data management.</p>
        </div>
        <div className="flex items-center gap-2 px-3 py-1.5 bg-primary/10 border border-primary/20 text-primary font-bold text-xs uppercase tracking-widest rounded-md">
          <ShieldCheck className="w-4 h-4" />
          System Access
        </div>
      </div>

      <div className="space-y-12">
        <section className="bg-white border border-border rounded-xl shadow-sm overflow-hidden group">
            <div className="px-8 py-6 border-b border-border bg-muted/30 flex items-center gap-4">
                <div className="w-10 h-10 bg-white border border-border rounded-lg flex items-center justify-center text-primary shadow-sm">
                    <Database className="w-5 h-5" />
                </div>
                <div>
                    <h2 className="text-xl font-display font-semibold text-foreground">Data Import</h2>
                    <p className="text-sm text-muted-foreground">Import photos and videos from external sources.</p>
                </div>
            </div>
            <div className="p-8">
                 <ImportPanel />
            </div>
        </section>

        <section className="bg-white border border-border rounded-xl shadow-sm overflow-hidden group">
            <div className="px-8 py-6 border-b border-border bg-muted/30 flex items-center gap-4">
                <div className="w-10 h-10 bg-white border border-border rounded-lg flex items-center justify-center text-primary shadow-sm">
                    <FileText className="w-5 h-5" />
                </div>
                <div>
                    <h2 className="text-xl font-display font-semibold text-foreground">Metadata</h2>
                    <p className="text-sm text-muted-foreground">Manage photo and video metadata and thumbnails.</p>
                </div>
            </div>
            <div className="p-8">
                 <MetadataPanel />
            </div>
        </section>

        <section className="bg-white border border-border rounded-xl shadow-sm overflow-hidden group">
            <div className="px-8 py-6 border-b border-border bg-muted/30 flex items-center gap-4">
                <div className="w-10 h-10 bg-white border border-border rounded-lg flex items-center justify-center text-secondary shadow-sm">
                    <Users className="w-5 h-5" />
                </div>
                 <div>
                    <h2 className="text-xl font-display font-semibold text-foreground">User Management</h2>
                    <p className="text-sm text-muted-foreground">Manage user access and permissions.</p>
                </div>
            </div>
             <div className="p-8">
                 <UserManagement />
            </div>
        </section>
      </div>
    </div>
    </div>
  )
}

