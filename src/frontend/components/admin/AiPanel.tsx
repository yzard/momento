import { useEffect, useState } from 'react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { AlertCircle, Bot, Eraser, Loader2, Play, Save, X } from 'lucide-react'

import {
  aiApi,
  type AiActionResponse,
  type AiFeature,
  type AiFeatureSchedule,
  type AiJobCounts,
  type AiStatusResponse,
} from '../../api/ai'
import { cn } from '../../lib/utils'

const FEATURES: ReadonlyArray<{ feature: AiFeature; label: string }> = [
  { feature: 'ocr', label: 'OCR' },
  { feature: 'image_tagging', label: 'Image Tagging' },
  { feature: 'screenshot_detection', label: 'Screenshot Detection' },
  { feature: 'document_detection', label: 'Document Detection' },
  { feature: 'image_aesthetics', label: 'Image Aesthetics' },
  { feature: 'deduplicate', label: 'Deduplicate' },
  { feature: 'face_detection', label: 'Face Detection' },
]

export default function AiPanel() {
  const statusQuery = useQuery({ queryKey: ['ai', 'status'], queryFn: aiApi.status, refetchInterval: 1000 })
  const allRunning = FEATURES.some(({ feature }) => isActive(featureState(statusQuery.data, feature)))

  return (
    <section className="overflow-hidden rounded-xl border border-border bg-card shadow-sm">
      <div className="border-b border-border bg-muted/30 px-5 py-5 sm:px-8 sm:py-6">
        <div className="flex items-center gap-4">
          <div className="flex h-10 w-10 items-center justify-center rounded-lg border border-border bg-card text-primary shadow-sm">
            <Bot className="h-5 w-5" />
          </div>
          <div>
            <h2 className="font-display text-xl font-semibold text-foreground">AI</h2>
            <p className="text-sm text-muted-foreground">Live work queues and system-timezone schedules.</p>
          </div>
        </div>
      </div>

      <AiStatusTable status={statusQuery.data} loading={statusQuery.isLoading} />

      <div className="border-t border-border px-5 py-6 sm:px-8">
        <h3 className="mb-1 text-sm font-bold uppercase tracking-wider text-muted-foreground">AI job controls</h3>
        <p className="mb-4 text-sm text-muted-foreground">Cron schedules use five fields and the server's system timezone.</p>
        <div className="space-y-3">
          <ControlRow
            label="All AI Jobs"
            running={allRunning}
            start={() => aiApi.start()}
            cancel={() => aiApi.cancel()}
            clean={() => aiApi.clean()}
          />
          {FEATURES.map(({ feature, label }) => (
            <ControlRow
              key={feature}
              label={label}
              running={isActive(featureState(statusQuery.data, feature))}
              start={() => aiApi.startFeature(feature)}
              cancel={() => aiApi.cancelFeature(feature)}
              clean={() => aiApi.cleanFeature(feature)}
              schedule={statusQuery.data?.schedules.find((entry) => entry.feature === feature)}
            />
          ))}
        </div>
      </div>
    </section>
  )
}

function AiStatusTable({ status, loading }: { status: AiStatusResponse | undefined; loading: boolean }) {
  return (
    <div className="px-5 py-6 sm:px-8">
      <h3 className="mb-4 text-sm font-bold uppercase tracking-wider text-muted-foreground">AI work status</h3>
      <div className="overflow-x-auto rounded-lg border border-border">
        <table className="w-full min-w-[680px] border-collapse text-left text-sm">
          <thead className="bg-muted/50 text-xs font-bold uppercase tracking-wider text-muted-foreground">
            <tr>
              <th scope="col" className="px-4 py-3">Feature</th>
              {['Queued', 'Submitting', 'Submitted', 'Failed', 'Completed'].map((label) => (
                <th key={label} scope="col" className="px-4 py-3 text-right">{label}</th>
              ))}
            </tr>
          </thead>
          <tbody className="divide-y divide-border">
            {FEATURES.map(({ feature, label }) => {
              const jobs = featureJobs(status, feature)
              return (
                <tr key={feature} className="bg-card">
                  <th scope="row" className="whitespace-nowrap px-4 py-3 font-semibold text-foreground">{label}</th>
                  <JobCount value={jobs?.queued} loading={loading} />
                  <JobCount value={jobs?.submitting} loading={loading} />
                  <JobCount value={jobs?.submitted} loading={loading} />
                  <JobCount value={jobs?.failed} loading={loading} emphasis={Boolean(jobs?.failed)} />
                  <JobCount value={jobs?.completed} loading={loading} />
                </tr>
              )
            })}
          </tbody>
        </table>
      </div>
      <div className="mt-3 flex flex-wrap gap-x-6 gap-y-1 text-xs text-muted-foreground">
        <span>Duplicate groups: <strong className="text-foreground">{status?.deduplicate.clustersCreated ?? 0}</strong></span>
        <span>Face groups: <strong className="text-foreground">{status?.faceGroups ?? 0}</strong></span>
        <span>Deduplication comparisons: <strong className="text-foreground">{status?.deduplicate.candidateComparisons ?? 0}</strong></span>
      </div>
    </div>
  )
}

function JobCount({ value, loading, emphasis = false }: { value: number | undefined; loading: boolean; emphasis?: boolean }) {
  return (
    <td className={cn('px-4 py-3 text-right font-mono font-semibold tabular-nums text-foreground', emphasis && 'text-destructive')}>
      {loading ? <Loader2 aria-label="Loading AI status" className="ml-auto h-4 w-4 animate-spin text-muted-foreground" /> : (value ?? 0).toLocaleString()}
    </td>
  )
}

function featureJobs(status: AiStatusResponse | undefined, feature: AiFeature): AiJobCounts | undefined {
  if (feature === 'deduplicate') return status?.deduplicate.jobs
  return status?.tasks.find((task) => task.task === feature)?.jobs
}

function featureState(status: AiStatusResponse | undefined, feature: AiFeature): string | undefined {
  if (feature === 'deduplicate') return status?.deduplicate.status
  return status?.tasks.find((task) => task.task === feature)?.state
}

function isActive(status: string | undefined): boolean {
  return status === 'queued' || status === 'submitting' || status === 'submitted' || status === 'running' || status === 'cancelling'
}

interface ControlRowProps {
  label: string
  running: boolean
  start: () => Promise<AiActionResponse>
  cancel: () => Promise<AiActionResponse>
  clean: () => Promise<AiActionResponse>
  schedule?: AiFeatureSchedule
}

function ControlRow({ label, running, start, cancel, clean, schedule }: ControlRowProps) {
  const queryClient = useQueryClient()
  const mutation = useMutation({
    mutationFn: (action: () => Promise<AiActionResponse>) => action(),
    onSuccess: () => void Promise.all([
      queryClient.invalidateQueries({ queryKey: ['ai'] }),
      queryClient.invalidateQueries({ queryKey: ['duplicates'] }),
    ]),
  })

  return (
    <div className="border-b border-border pb-3 last:border-b-0 last:pb-0">
      <div className={cn('grid gap-3', schedule ? 'lg:grid-cols-[minmax(12rem,1fr)_minmax(12rem,1fr)_minmax(18rem,1.35fr)]' : 'sm:grid-cols-2')}>
        <button type="button" onClick={() => mutation.mutate(running ? cancel : start)} disabled={mutation.isPending} className={cn('flex min-h-11 items-center justify-center gap-2 rounded-lg px-4 py-2 text-sm font-bold transition-colors disabled:cursor-not-allowed disabled:opacity-50', running ? 'bg-destructive text-destructive-foreground hover:bg-destructive/90' : 'bg-primary text-primary-foreground hover:bg-primary/90')}>
          {mutation.isPending ? <Loader2 className="h-4 w-4 animate-spin" /> : running ? <X className="h-4 w-4" /> : <Play className="h-4 w-4" />}
          {running ? 'Cancel' : 'Start'} {label}
        </button>
        <button type="button" onClick={() => mutation.mutate(clean)} disabled={running || mutation.isPending} className="flex min-h-11 items-center justify-center gap-2 rounded-lg border border-destructive/20 bg-destructive/5 px-4 py-2 text-sm font-bold text-destructive transition-colors hover:bg-destructive/10 disabled:cursor-not-allowed disabled:opacity-50">
          <Eraser className="h-4 w-4" />
          Clean {label === 'All AI Jobs' ? 'All AI Data' : `${label} Data`}
        </button>
        {schedule && <ScheduleEditor label={label} schedule={schedule} />}
      </div>
      {mutation.isError && <p role="alert" className="mt-2 flex items-center gap-2 text-sm text-destructive"><AlertCircle className="h-4 w-4" />The {label} action failed. Status updates automatically; try again.</p>}
    </div>
  )
}

function ScheduleEditor({ label, schedule }: { label: string; schedule: AiFeatureSchedule }) {
  const queryClient = useQueryClient()
  const [cronExpression, setCronExpression] = useState(schedule.cronExpression)
  useEffect(() => setCronExpression(schedule.cronExpression), [schedule.cronExpression])

  const mutation = useMutation({
    mutationFn: () => aiApi.updateSchedule(schedule.feature, cronExpression),
    onSuccess: (updatedSchedule) => {
      setCronExpression(updatedSchedule.cronExpression)
      void queryClient.invalidateQueries({ queryKey: ['ai', 'status'] })
    },
  })
  const unchanged = cronExpression.trim().replace(/\s+/g, ' ') === schedule.cronExpression

  return (
    <form
      className="flex min-w-0 gap-2"
      onSubmit={(event) => {
        event.preventDefault()
        mutation.mutate()
      }}
    >
      <label className="sr-only" htmlFor={`cron-${schedule.feature}`}>{label} cron schedule</label>
      <input
        id={`cron-${schedule.feature}`}
        type="text"
        value={cronExpression}
        onChange={(event) => setCronExpression(event.target.value)}
        aria-invalid={mutation.isError}
        className="min-h-11 min-w-0 flex-1 rounded-lg border border-input bg-background px-3 font-mono text-sm text-foreground outline-none transition-colors focus:border-primary focus:ring-2 focus:ring-primary/20"
      />
      <button
        type="submit"
        aria-label={`Save ${label} cron schedule`}
        disabled={mutation.isPending || unchanged || cronExpression.trim().length === 0}
        className="flex min-h-11 items-center justify-center gap-2 rounded-lg border border-border bg-card px-3 text-sm font-bold text-foreground transition-colors hover:bg-muted disabled:cursor-not-allowed disabled:opacity-50"
      >
        {mutation.isPending ? <Loader2 className="h-4 w-4 animate-spin" /> : <Save className="h-4 w-4" />}
        <span className="hidden sm:inline">Save</span>
      </button>
      {mutation.isError && <span role="alert" className="sr-only">The {label} cron schedule is invalid or could not be saved.</span>}
    </form>
  )
}
