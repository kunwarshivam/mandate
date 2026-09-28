import type { ReactNode } from "react";
import { cn } from "@/lib/utils";

/**
 * Kumo's ResourceListPage layout (`kumo add ResourceListPage`), in the calm system: the list takes the main
 * column and an optional aside holds context. The shell already draws the page background and the
 * width, so this block draws neither.
 */
export interface ResourceListPageProps {
  header: ReactNode;
  aside?: ReactNode;
  children: ReactNode;
  className?: string;
}

export function ResourceListPage({ header, aside, children, className }: ResourceListPageProps) {
  return (
    <div className={cn("grid grid-cols-1", className)}>
      {header}
      <div className="grid grid-cols-1 gap-(--section-gap) xl:grid-cols-[minmax(0,1fr)_22rem]">
        <div className="min-w-0">{children}</div>
        {aside ? <aside className="grid content-start gap-(--block-gap) xl:sticky xl:top-28">{aside}</aside> : null}
      </div>
    </div>
  );
}
