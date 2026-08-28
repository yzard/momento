import { useEffect, useState } from 'react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { AlertCircle, Bot, Eraser, Loader2, Play, Save, X } from 'lucide-react'
import type { ReactNode } from 'react'
import ConfirmationDialog from '../common/ConfirmationDialog'

import {
  aiApi,
  type AiActionResponse,
  type AiFeature,
  type AiFeatureSchedule,
  type AiJobCounts,
  type AiStatusResponse,
} from '../../api/ai'
import { cn } from '../../lib/utils'
import { queryKeys } from '../../lib/queryKeys'
import { joinCronFields, splitCronExpression, validCronFields, type CronFields } from './cron'

const FEATURES: ReadonlyArray<{ feature: AiFeature; label: string }> = [
  { feature: 'ocr', label: 'OCR' },
  { feature: 'image_tagging', label: 'Image Tagging' },
  { feature: 'screenshot_detection', label: 'Screenshot Detection' },
  { feature: 'document_detection', label: 'Document Detection' },
  { feature: 'image_aesthetics', label: 'Image Aesthetics' },
  { feature: 'deduplicate', label: 'Deduplicate' },
  { feature: 'face_detection', label: 'Face Detection' },
]

const CRON_FIELD_DEFINITIONS = [
  { key: 'minute', label: 'Minute', index: 0 },
  { key: 'hour', label: 'Hour', index: 1 },
  { key: 'day', label: 'Day', index: 2 },
  { key: 'month', label: 'Month', index: 3 },
  { key: 'weekday', label: 'Weekday', index: 4 },
] as const

export default function AiPanel() {
  const statusQuery = useQuery({
    queryKey: queryKeys.ai.status,
    queryFn: aiApi.status,
    refetchInterval: 1000,
  })

  return (
    <section className="overflow-hidden rounded-xl border border-border bg-card shadow-sm">
      <div className="border-b border-border bg-muted/30 px-5 py-5 sm:px-8 sm:py-6">
        <div className="flex items-center gap-4">
          <div className="flex h-10 w-10 items-center justify-center rounded-lg border border-border bg-card text-primary shadow-sm">
            <Bot className="h-5 w-5" />
          </div>
          <div>
            <h2 className="font-display text-xl font-semibold text-foreground">AI</h2>
            <p className="text-sm text-muted-foreground">
              Live work queues and system-timezone schedules.
            </p>
          </div>
        </div>
      </div>

      <AiStatusTable status={statusQuery.data} loading={statusQuery.isLoading} />
      <AiControlTable status={statusQuery.data} />
    </section>
  )
}

function AiStatusTable({
  status,
  loading,
}: {
  status: AiStatusResponse | undefined
  loading: boolean
}) {
  return (
    <div className="px-5 py-6 sm:px-8">
      <h3 className="mb-4 text-sm font-bold uppercase tracking-wider text-muted-foreground">
        AI work status
      </h3>
      <div className="overflow-x-auto rounded-lg border border-border">
        <table
          aria-label="AI work status"
          className="w-full min-w-[680px] border-collapse text-left text-sm"
        >
          <thead className="bg-muted/50 text-xs font-bold uppercase tracking-wider text-muted-foreground">
            <tr>
              <th scope="col" className="px-4 py-3">
                Feature
              </th>
              {['Queued', 'Submitting', 'Submitted', 'Failed', 'Completed'].map((label) => (
                <th key={label} scope="col" className="px-4 py-3 text-right">
                  {label}
                </th>
              ))}
            </tr>
          </thead>
          <tbody className="divide-y divide-border">
            {FEATURES.map(({ feature, label }) => {
              const jobs = featureJobs(status, feature)
              return (
                <tr key={feature} className="bg-card">
                  <th
                    scope="row"
                    className="whitespace-nowrap px-4 py-3 font-semibold text-foreground"
                  >
                    {label}
                  </th>
                  <JobCount value={jobs?.queued} loading={loading} />
                  <JobCount value={jobs?.submitting} loading={loading} />
                  <JobCount value={jobs?.submitted} loading={loading} />
                  <JobCount
                    value={jobs?.failed}
                    loading={loading}
                    emphasis={Boolean(jobs?.failed)}
                  />
                  <JobCount value={jobs?.completed} loading={loading} />
                </tr>
              )
            })}
          </tbody>
        </table>
      </div>
      <div className="mt-3 flex flex-wrap gap-x-6 gap-y-1 text-xs text-muted-foreground">
        <span>
          Duplicate groups:{' '}
          <strong className="text-foreground">{status?.deduplicate.clustersCreated ?? 0}</strong>
        </span>
        <span>
          Face groups: <strong className="text-foreground">{status?.faceGroups ?? 0}</strong>
        </span>
        <span>
          Deduplication comparisons:{' '}
          <strong className="text-foreground">
            {status?.deduplicate.candidateComparisons ?? 0}
          </strong>
        </span>
      </div>
    </div>
  )
}

function JobCount({
  value,
  loading,
  emphasis = false,
}: {
  value: number | undefined
  loading: boolean
  emphasis?: boolean
}) {
  return (
    <td
      className={cn(
        'px-4 py-3 text-right font-mono font-semibold tabular-nums text-foreground',
        emphasis && 'text-destructive'
      )}
    >
      {loading ? (
        <Loader2
          aria-label="Loading AI status"
          className="ml-auto h-4 w-4 animate-spin text-muted-foreground"
        />
      ) : (
        (value ?? 0).toLocaleString()
      )}
    </td>
  )
}

function featureJobs(
  status: AiStatusResponse | undefined,
  feature: AiFeature
): AiJobCounts | undefined {
  if (feature === 'deduplicate') return status?.deduplicate.jobs
  return status?.tasks.find((task) => task.task === feature)?.jobs
}

function featureState(
  status: AiStatusResponse | undefined,
  feature: AiFeature
): string | undefined {
  if (feature === 'deduplicate') return status?.deduplicate.status
  return status?.tasks.find((task) => task.task === feature)?.state
}

function isActive(status: string | undefined): boolean {
  return (
    status === 'queued' ||
    status === 'submitting' ||
    status === 'submitted' ||
    status === 'running' ||
    status === 'cancelling'
  )
}

interface ControlRowProps {
  feature: AiFeature
  label: string
  running: boolean
  start: () => Promise<AiActionResponse>
  cancel: () => Promise<AiActionResponse>
  onCleanRequest: () => void
  schedule: AiFeatureSchedule | undefined
}

interface CleanRequest {
  label: string
  action: () => Promise<AiActionResponse>
}

function AiControlTable({ status }: { status: AiStatusResponse | undefined }) {
  const allRunning = FEATURES.some(({ feature }) => isActive(featureState(status, feature)))
  const [cleanRequest, setCleanRequest] = useState<CleanRequest | null>(null)
  const cleanMutation = useAiActionMutation()

  return (
    <div className="border-t border-border px-5 py-6 sm:px-8">
      <div className="mb-4 flex flex-col justify-between gap-3 sm:flex-row sm:items-end">
        <div>
          <h3 className="mb-1 text-sm font-bold uppercase tracking-wider text-muted-foreground">
            AI job controls
          </h3>
          <p className="text-sm text-muted-foreground">
            Cron schedules use five fields and the server's system timezone.
          </p>
        </div>
        <GlobalAiControls
          running={allRunning}
          onCleanRequest={() => setCleanRequest({ label: 'all AI', action: aiApi.clean })}
        />
      </div>
      <div className="overflow-x-auto rounded-lg border border-border">
        <table
          aria-label="AI feature controls"
          className="w-full min-w-[1120px] border-collapse text-left text-sm"
        >
          <thead className="bg-muted/50 text-xs font-bold uppercase tracking-wider text-muted-foreground">
            <tr>
              {[
                'Feature',
                'Minute',
                'Hour',
                'Day',
                'Month',
                'Weekday',
                'Save',
                'Start / Cancel',
                'Clean',
              ].map((label) => (
                <th
                  key={label}
                  scope="col"
                  className={cn('px-3 py-3', label === 'Feature' ? 'text-left' : 'text-center')}
                >
                  {label}
                </th>
              ))}
            </tr>
          </thead>
          <tbody className="divide-y divide-border">
            {FEATURES.map(({ feature, label }) => (
              <FeatureControlRow
                key={feature}
                feature={feature}
                label={label}
                running={isActive(featureState(status, feature))}
                start={() => aiApi.startFeature(feature)}
                cancel={() => aiApi.cancelFeature(feature)}
                onCleanRequest={() =>
                  setCleanRequest({
                    label,
                    action: () => aiApi.cleanFeature(feature),
                  })
                }
                schedule={status?.schedules.find((entry) => entry.feature === feature)}
              />
            ))}
          </tbody>
        </table>
      </div>
      {cleanRequest && (
        <ConfirmationDialog
          title={`Clean ${cleanRequest.label} data?`}
          description="This permanently removes the selected AI results and job history. The data can only be restored by running the AI work again."
          confirmLabel="Clean data"
          isProcessing={cleanMutation.isPending}
          destructive
          onConfirm={() =>
            cleanMutation.mutate(cleanRequest.action, {
              onSuccess: () => setCleanRequest(null),
            })
          }
          onCancel={() => setCleanRequest(null)}
        />
      )}
    </div>
  )
}

function useAiActionMutation() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (action: () => Promise<AiActionResponse>) => action(),
    onSuccess: () =>
      void Promise.all([
        queryClient.invalidateQueries({ queryKey: queryKeys.ai.all }),
        queryClient.invalidateQueries({ queryKey: queryKeys.duplicates.all }),
      ]),
  })
}

function GlobalAiControls({
  running,
  onCleanRequest,
}: {
  running: boolean
  onCleanRequest: () => void
}) {
  const mutation = useAiActionMutation()
  const primaryActionLabel = `${running ? 'Cancel' : 'Start'} All AI Jobs`

  return (
    <div className="flex flex-wrap items-center gap-2">
      <span className="mr-1 text-xs font-semibold uppercase tracking-wide text-muted-foreground">
        All AI Jobs
      </span>
      <ActionButton
        accessibleLabel={primaryActionLabel}
        visibleLabel={running ? 'Cancel all' : 'Start all'}
        icon={
          running ? (
            <X aria-hidden="true" className="h-4 w-4" />
          ) : (
            <Play aria-hidden="true" className="h-4 w-4" />
          )
        }
        pending={mutation.isPending}
        disabled={mutation.isPending}
        variant={running ? 'destructive' : 'primary'}
        onClick={() => mutation.mutate(running ? aiApi.cancel : aiApi.start)}
      />
      <ActionButton
        accessibleLabel="Clean All AI Data"
        visibleLabel="Clean all"
        icon={<Eraser aria-hidden="true" className="h-4 w-4" />}
        pending={false}
        disabled={running || mutation.isPending}
        variant="clean"
        onClick={onCleanRequest}
      />
      {mutation.isError && (
        <span role="alert" className="flex items-center gap-1 text-xs text-destructive">
          <AlertCircle className="h-3.5 w-3.5" />
          The all-jobs action failed.
        </span>
      )}
    </div>
  )
}

function featureRowStatus(invalidFields: boolean, actionFailed: boolean, running: boolean): string {
  if (invalidFields) return 'Invalid cron values'
  if (actionFailed) return 'Action failed'
  if (running) return 'Active work'
  return 'Idle'
}

function CronFieldCell({
  definition,
  feature,
  featureLabel,
  value,
  disabled,
  scheduleLoaded,
  onChange,
}: {
  definition: (typeof CRON_FIELD_DEFINITIONS)[number]
  feature: AiFeature
  featureLabel: string
  value: string
  disabled: boolean
  scheduleLoaded: boolean
  onChange: (value: string) => void
}) {
  const inputId = `cron-${feature}-${definition.key}`
  const normalizedValue = value.trim()
  const fieldInvalid =
    scheduleLoaded && (normalizedValue.length === 0 || /\s/.test(normalizedValue))

  return (
    <td className="w-24 px-2 py-3 text-center">
      <label htmlFor={inputId} className="sr-only">
        {featureLabel} cron {definition.label.toLowerCase()}
      </label>
      <input
        id={inputId}
        type="text"
        value={value}
        onChange={(event) => onChange(event.target.value)}
        aria-invalid={fieldInvalid}
        disabled={disabled}
        autoComplete="off"
        spellCheck={false}
        className="min-h-10 w-full min-w-20 rounded-md border border-input bg-background px-2 text-center font-mono text-sm text-foreground outline-none transition-colors focus:border-primary focus:ring-2 focus:ring-primary/20 disabled:cursor-wait disabled:opacity-50"
      />
    </td>
  )
}

function useFeatureSchedule(schedule: AiFeatureSchedule | undefined, featureLabel: string) {
  const queryClient = useQueryClient()
  const scheduleExpression = schedule?.cronExpression
  const [cronFields, setCronFields] = useState<CronFields>(() =>
    scheduleExpression ? splitCronExpression(scheduleExpression) : ['', '', '', '', '']
  )

  useEffect(() => {
    if (!scheduleExpression) return
    setCronFields(splitCronExpression(scheduleExpression))
  }, [scheduleExpression])

  const cronExpression = joinCronFields(cronFields)
  const fieldsValid = validCronFields(cronFields)
  const mutation = useMutation({
    mutationFn: () => {
      if (!schedule) throw new Error(`${featureLabel} schedule is not loaded`)
      if (!fieldsValid) throw new Error(`${featureLabel} schedule is invalid`)
      return aiApi.updateSchedule(schedule.feature, cronExpression)
    },
    onSuccess: (updatedSchedule) => {
      setCronFields(splitCronExpression(updatedSchedule.cronExpression))
      void queryClient.invalidateQueries({ queryKey: queryKeys.ai.status })
    },
  })
  const unchanged =
    !schedule || cronExpression === joinCronFields(splitCronExpression(schedule.cronExpression))
  const updateField = (index: number, value: string) => {
    setCronFields((currentFields) => {
      const nextFields = [...currentFields] as CronFields
      nextFields[index] = value
      return nextFields
    })
  }

  return { cronFields, fieldsValid, mutation, unchanged, updateField }
}

function FeatureActionCells({
  label,
  running,
  start,
  cancel,
  onCleanRequest,
  scheduleLoaded,
  fieldsValid,
  scheduleUnchanged,
  schedulePending,
  actionPending,
  onSaveSchedule,
  onRunAction,
}: Pick<ControlRowProps, 'label' | 'running' | 'start' | 'cancel' | 'onCleanRequest'> & {
  scheduleLoaded: boolean
  fieldsValid: boolean
  scheduleUnchanged: boolean
  schedulePending: boolean
  actionPending: boolean
  onSaveSchedule: () => void
  onRunAction: (action: () => Promise<AiActionResponse>) => void
}) {
  const primaryActionLabel = `${running ? 'Cancel' : 'Start'} ${label}`

  return (
    <>
      <td className="px-3 py-3 text-center">
        <ActionButton
          accessibleLabel={`Save ${label} cron schedule`}
          visibleLabel="Save"
          icon={<Save aria-hidden="true" className="h-4 w-4" />}
          pending={schedulePending}
          disabled={!scheduleLoaded || schedulePending || scheduleUnchanged || !fieldsValid}
          variant="neutral"
          onClick={onSaveSchedule}
        />
      </td>
      <td className="px-3 py-3 text-center">
        <ActionButton
          accessibleLabel={primaryActionLabel}
          visibleLabel={running ? 'Cancel' : 'Start'}
          icon={
            running ? (
              <X aria-hidden="true" className="h-4 w-4" />
            ) : (
              <Play aria-hidden="true" className="h-4 w-4" />
            )
          }
          pending={actionPending}
          disabled={actionPending}
          variant={running ? 'destructive' : 'primary'}
          onClick={() => onRunAction(running ? cancel : start)}
        />
      </td>
      <td className="px-3 py-3 text-center">
        <ActionButton
          accessibleLabel={`Clean ${label} Data`}
          visibleLabel="Clean"
          icon={<Eraser aria-hidden="true" className="h-4 w-4" />}
          pending={false}
          disabled={running || actionPending}
          variant="clean"
          onClick={onCleanRequest}
        />
      </td>
    </>
  )
}

function FeatureControlRow({
  feature,
  label,
  running,
  start,
  cancel,
  onCleanRequest,
  schedule,
}: ControlRowProps) {
  const actionMutation = useAiActionMutation()
  const featureSchedule = useFeatureSchedule(schedule, label)
  const invalidFields = Boolean(schedule) && !featureSchedule.fieldsValid
  const actionFailed = actionMutation.isError || featureSchedule.mutation.isError
  const hasError = actionFailed || invalidFields
  const rowStatus = featureRowStatus(invalidFields, actionFailed, running)

  return (
    <tr className="bg-card align-middle">
      <th scope="row" className="min-w-44 px-3 py-3">
        <span className="block whitespace-nowrap font-semibold text-foreground">{label}</span>
        <span
          className={cn('text-xs', hasError ? 'text-destructive' : 'text-muted-foreground')}
          role={hasError ? 'alert' : undefined}
        >
          {rowStatus}
        </span>
      </th>
      {CRON_FIELD_DEFINITIONS.map((definition) => (
        <CronFieldCell
          key={definition.key}
          definition={definition}
          feature={feature}
          featureLabel={label}
          value={featureSchedule.cronFields[definition.index]}
          disabled={!schedule || featureSchedule.mutation.isPending}
          scheduleLoaded={Boolean(schedule)}
          onChange={(value) => featureSchedule.updateField(definition.index, value)}
        />
      ))}
      <FeatureActionCells
        label={label}
        running={running}
        start={start}
        cancel={cancel}
        onCleanRequest={onCleanRequest}
        scheduleLoaded={Boolean(schedule)}
        fieldsValid={featureSchedule.fieldsValid}
        scheduleUnchanged={featureSchedule.unchanged}
        schedulePending={featureSchedule.mutation.isPending}
        actionPending={actionMutation.isPending}
        onSaveSchedule={() => featureSchedule.mutation.mutate()}
        onRunAction={(action) => actionMutation.mutate(action)}
      />
    </tr>
  )
}

type ActionButtonVariant = 'primary' | 'destructive' | 'clean' | 'neutral'

interface ActionButtonProps {
  accessibleLabel: string
  visibleLabel: string
  icon: ReactNode
  pending: boolean
  disabled: boolean
  variant: ActionButtonVariant
  onClick: () => void
}

function ActionButton({
  accessibleLabel,
  visibleLabel,
  icon,
  pending,
  disabled,
  variant,
  onClick,
}: ActionButtonProps) {
  return (
    <button
      type="button"
      aria-label={accessibleLabel}
      title={accessibleLabel}
      onClick={onClick}
      disabled={disabled}
      className={cn(
        'inline-flex min-h-10 min-w-20 items-center justify-center gap-2 rounded-md border px-3 text-xs font-bold transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary focus-visible:ring-offset-2 disabled:cursor-not-allowed disabled:opacity-50',
        variant === 'primary' &&
          'border-primary bg-primary text-primary-foreground hover:bg-primary/90',
        variant === 'destructive' &&
          'border-destructive bg-destructive text-destructive-foreground hover:bg-destructive/90',
        variant === 'clean' &&
          'border-destructive/20 bg-destructive/5 text-destructive hover:bg-destructive/10',
        variant === 'neutral' && 'border-border bg-card text-foreground hover:bg-muted'
      )}
    >
      {pending ? <Loader2 aria-hidden="true" className="h-4 w-4 animate-spin" /> : icon}
      <span>{visibleLabel}</span>
    </button>
  )
}
