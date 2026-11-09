interface BrandMarkProps {
  className: string
  alt: string
}

export default function BrandMark({ className, alt }: BrandMarkProps) {
  return (
    <img src="/brand/momento-192.png" alt={alt} width={192} height={192} className={className} />
  )
}
