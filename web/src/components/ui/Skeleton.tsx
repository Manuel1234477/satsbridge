export function Skeleton({ height = 64 }: { height?: number }) {
  return <div className="skeleton" style={{ height }} aria-hidden="true" />;
}
