// Original mark: an arc over two pillars (a bridge), with a single node. Not derived from any brand.
export function LogoMark({ size = 32 }: { size?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 32 32" aria-hidden="true" focusable="false">
      <rect width="32" height="32" rx="9" fill="var(--accent)" />
      <path d="M6 22 C10 12, 22 12, 26 22" fill="none" stroke="var(--accent-ink)" strokeWidth="2.8" strokeLinecap="round" />
      <path d="M10.5 22 V16.5 M21.5 22 V16.5" stroke="var(--accent-ink)" strokeWidth="2.8" strokeLinecap="round" />
      <circle cx="16" cy="11.5" r="2.2" fill="var(--accent-ink)" />
    </svg>
  );
}
