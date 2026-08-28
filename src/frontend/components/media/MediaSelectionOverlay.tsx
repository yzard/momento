import { Check, Circle } from 'lucide-react'

export default function MediaSelectionOverlay({ selected }: { selected: boolean }) {
  return (
    <div
      className={`absolute inset-0 transition-colors duration-150 motion-reduce:transition-none ${selected ? 'bg-primary/25' : 'bg-black/10'}`}
    >
      <span
        className={`absolute left-2 top-2 flex h-8 w-8 items-center justify-center rounded-full border-2 shadow-sm transition-colors duration-150 ${selected ? 'border-primary bg-primary text-primary-foreground' : 'border-white/90 bg-black/35 text-white'}`}
      >
        {selected ? <Check className="h-4 w-4" strokeWidth={3} /> : <Circle className="h-4 w-4" />}
      </span>
    </div>
  )
}
