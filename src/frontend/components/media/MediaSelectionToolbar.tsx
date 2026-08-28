import type { ReactNode } from 'react'
import { CheckSquare2, FolderPlus, FolderX, Trash2, X } from 'lucide-react'

interface MediaSelectionToolbarProps {
  selectedCount: number
  isProcessing: boolean
  onClear: () => void
  onDone: () => void
  onAddToAlbum: (() => void) | null
  onRemoveFromAlbum: (() => void) | null
  onMoveToTrash: (() => void) | null
}

export function MediaSelectButton({ onClick }: { onClick: () => void }) {
  return (
    <button
      type="button"
      onClick={onClick}
      className="inline-flex min-h-11 cursor-pointer items-center gap-2 rounded-lg border border-border bg-background px-4 py-2 text-sm font-semibold text-foreground transition-colors duration-200 hover:bg-muted active:bg-muted/80"
    >
      <CheckSquare2 className="h-4 w-4" />
      Select
    </button>
  )
}

export default function MediaSelectionToolbar({
  selectedCount,
  isProcessing,
  onClear,
  onDone,
  onAddToAlbum,
  onRemoveFromAlbum,
  onMoveToTrash,
}: MediaSelectionToolbarProps) {
  const hasSelection = selectedCount > 0

  return (
    <div
      role="toolbar"
      aria-label="Media selection"
      className="flex flex-col gap-3 rounded-xl border border-border bg-card/95 p-3 shadow-sm backdrop-blur-md sm:flex-row sm:items-center sm:justify-between"
    >
      <div className="flex min-h-11 items-center gap-3 px-1">
        <span className="flex h-9 w-9 items-center justify-center rounded-lg bg-primary/10 text-primary">
          <CheckSquare2 className="h-5 w-5" />
        </span>
        <div>
          <p className="text-sm font-semibold text-foreground">{selectedCount} selected</p>
          <p className="text-xs text-muted-foreground">Choose media, then apply an action.</p>
        </div>
      </div>

      <div className="flex flex-wrap items-center gap-2">
        {onAddToAlbum && (
          <SelectionActionButton
            label="Add to album"
            disabled={!hasSelection || isProcessing}
            onClick={onAddToAlbum}
            icon={<FolderPlus className="h-4 w-4" />}
            destructive={false}
          />
        )}
        {onRemoveFromAlbum && (
          <SelectionActionButton
            label="Remove from album"
            disabled={!hasSelection || isProcessing}
            onClick={onRemoveFromAlbum}
            icon={<FolderX className="h-4 w-4" />}
            destructive={false}
          />
        )}
        {onMoveToTrash && (
          <SelectionActionButton
            label="Move to Trash"
            disabled={!hasSelection || isProcessing}
            onClick={onMoveToTrash}
            icon={<Trash2 className="h-4 w-4" />}
            destructive
          />
        )}
        <button
          type="button"
          onClick={onClear}
          disabled={!hasSelection || isProcessing}
          className="min-h-11 cursor-pointer rounded-lg px-3 py-2 text-sm font-medium text-muted-foreground transition-colors duration-200 hover:bg-muted hover:text-foreground disabled:cursor-not-allowed disabled:opacity-40"
        >
          Clear
        </button>
        <button
          type="button"
          onClick={onDone}
          disabled={isProcessing}
          aria-label="Finish selecting media"
          className="inline-flex h-11 w-11 cursor-pointer items-center justify-center rounded-lg bg-foreground text-background transition-opacity duration-200 hover:opacity-85 disabled:cursor-not-allowed disabled:opacity-40"
        >
          <X className="h-4 w-4" />
        </button>
      </div>
    </div>
  )
}

interface SelectionActionButtonProps {
  label: string
  disabled: boolean
  onClick: () => void
  icon: ReactNode
  destructive: boolean
}

function SelectionActionButton({
  label,
  disabled,
  onClick,
  icon,
  destructive,
}: SelectionActionButtonProps) {
  return (
    <button
      type="button"
      onClick={onClick}
      disabled={disabled}
      className={
        destructive
          ? 'inline-flex min-h-11 cursor-pointer items-center gap-2 rounded-lg px-3 py-2 text-sm font-semibold text-destructive transition-colors duration-200 hover:bg-destructive/10 disabled:cursor-not-allowed disabled:opacity-40'
          : 'inline-flex min-h-11 cursor-pointer items-center gap-2 rounded-lg bg-primary px-3 py-2 text-sm font-semibold text-primary-foreground transition-opacity duration-200 hover:opacity-90 disabled:cursor-not-allowed disabled:opacity-40'
      }
    >
      {icon}
      {label}
    </button>
  )
}
