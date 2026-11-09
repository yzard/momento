import SelectionMark from './SelectionMark'

export default function MediaSelectionOverlay({ selected }: { selected: boolean }) {
  return (
    <div
      className={`absolute inset-0 transition-colors duration-150 motion-reduce:transition-none ${selected ? 'bg-primary/25' : 'bg-black/10'}`}
    >
      <span className="absolute left-2 top-2">
        <SelectionMark selected={selected} />
      </span>
    </div>
  )
}
