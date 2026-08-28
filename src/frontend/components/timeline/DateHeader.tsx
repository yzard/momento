import type { GroupBy } from '../../api/media'

interface DateHeaderProps {
  date: string
  count: number
  groupBy: GroupBy
}

export default function DateHeader({ date, count, groupBy }: DateHeaderProps) {
  const formatDate = (dateStr: string, mode: GroupBy) => {
    if (dateStr === 'Unknown') return 'Unknown Date'

    if (mode === 'year') return dateStr

    if (mode === 'month') {
      const [year, month] = dateStr.split('-')
      if (year && month) {
        const monthName = new Intl.DateTimeFormat('en-US', { month: 'long' }).format(
          new Date(Number(year), Number(month) - 1, 1)
        )
        return `${monthName} ${year}`
      }
    }

    if (mode === 'week') {
      const [year, weekPart] = dateStr.split('-')
      if (year && weekPart) {
        return `Week ${weekPart.replace('W', '')}, ${year}`
      }
    }

    const [year, month, day] = dateStr.split('-')
    if (!year || !month || !day) return dateStr
    return new Date(Number(year), Number(month) - 1, Number(day)).toLocaleDateString('en-US', {
      weekday: 'long',
      year: 'numeric',
      month: 'long',
      day: 'numeric',
    })
  }

  return (
    <div className="sticky top-0 z-10 bg-background/95 backdrop-blur-sm pt-6 pb-4 mb-2 flex items-baseline gap-4">
      <h3 className="text-2xl font-display font-semibold text-foreground tracking-tight">
        {formatDate(date, groupBy)}
      </h3>
      <div className="h-px flex-1 bg-border/40"></div>
      <span className="text-xs font-bold text-muted-foreground uppercase tracking-wider bg-muted/50 px-2.5 py-1 rounded-full border border-border/50">
        {count} items
      </span>
    </div>
  )
}
