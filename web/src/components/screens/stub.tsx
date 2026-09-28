import type { ReactNode } from "react";

/** A screen this slice does not build yet: what it will hold, and nothing that pretends to work. */
export function Stub({ title, children }: { title: string; children: ReactNode }) {
  return (
    <section className="grid max-w-3xl gap-3" aria-labelledby="stub-title">
      <h1 id="stub-title" className="text-title sm:text-display">
        {title}
      </h1>
      <div className="grid max-w-prose gap-2 text-muted-foreground">{children}</div>
    </section>
  );
}
