import type { GroupBy } from '../../api/media'

interface DateHeaderProps {
  date: string
  count: number
  groupBy?: GroupBy
}

export default function DateHeader({ date, count, groupBy = 'day' }: DateHeaderProps) {
  const formatDate = (dateStr: string, mode: GroupBy) => {
    if (dateStr === 'Unknown') return 'Unknown Date'

    try {
      if (mode === 'year') {
        return dateStr // Backend already returns "YYYY"
      }

      if (mode === 'month') {
        const [year, month] = dateStr.split('-')
        if (year && month) {
          // Manually create a date to avoid browser-specific parsing issues
          const monthName = new Intl.DateTimeFormat('en-US', { month: 'long' }).format(
            new Date(parseInt(year), parseInt(month) - 1, 1)
          )
          return `${monthName} ${year}`
        }
      }

      if (mode === 'week') {
        const [year, weekPart] = dateStr.split('-')
        if (year && weekPart) {
          const weekNum = weekPart.replace('W', '')
          return `Week ${weekNum}, ${year}`
        }
      }

      // Default or 'day' mode: dateStr is "YYYY-MM-DD"
      const [year, month, day] = dateStr.split('-')
      if (year && month && day) {
        const d = new Date(parseInt(year), parseInt(month) - 1, parseInt(day))
        return d.toLocaleDateString('en-US', {
          weekday: 'long',
          year: 'numeric',
          month: 'long',
          day: 'numeric',
        })
      }

      // Fallback for unexpected formats
      const d = new Date(dateStr)
      if (isNaN(d.getTime())) return dateStr
      return d.toLocaleDateString('en-US', {
        weekday: 'long',
        year: 'numeric',
        month: 'long',
        day: 'numeric',
      })
    } catch {
      return dateStr
    }
  }

  return (
    <div className="sticky top-0 z-10 bg-background/95 backdrop-blur-sm pt-6 pb-4 mb-2 flex items-baseline gap-4">
      <h3 className="text-2xl font-display font-semibold text-foreground tracking-tight">
        {formatDate(date, groupBy)}
      </h3>
      <div className="h-px flex-1 bg-border/40"></div>
      <span className="text-xs font-bold text-muted-foreground uppercase tracking-wider bg-muted/50 px-2.5 py-1 rounded-full border border-border/50">{count} items</span>
    </div>
  )
}

