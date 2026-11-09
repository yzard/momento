import { Check } from 'lucide-react'

export default function SelectionMark({ selected }: { selected: boolean }) {
  return (
    <span
      aria-hidden="true"
      className={`flex h-6 w-6 items-center justify-center rounded-full border-2 border-white ${selected ? 'bg-black/75' : 'bg-transparent'}`}
    >
      {selected && <Check className="h-4 w-4 text-white" strokeWidth={3} />}
    </span>
  )
}
