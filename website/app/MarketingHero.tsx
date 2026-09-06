import type { ReactNode } from "react";

/** Shared marketing header. Intentionally not used by Docs or navigation. */
export function MarketingHero({ eyebrow, title, children, actions }: {
  eyebrow: string;
  title: ReactNode;
  children: ReactNode;
  actions?: ReactNode;
}) {
  return (
    <section className="marketing-hero" aria-labelledby="page-title">
      <p className="kicker">{eyebrow}</p>
      <h1 id="page-title">{title}</h1>
      <div className="marketing-hero__lede">{children}</div>
      {actions && <div className="marketing-actions">{actions}</div>}
    </section>
  );
}
