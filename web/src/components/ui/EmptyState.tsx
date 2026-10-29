import type { ReactNode } from 'react';

export function EmptyState({ icon, title, body, action }: { icon: ReactNode; title: string; body: string; action?: ReactNode }) {
  return (
    <div className="empty-state">
      <span className="empty-icon" aria-hidden="true">
        {icon}
      </span>
      <h2 className="text-xl font-semibold">{title}</h2>
      <p className="label max-w-xs">{body}</p>
      {action}
    </div>
  );
}
