import type { ReactNode } from "react";
import { KEY, PixelIcon } from "@/components/site/pixel-icons";
import { cn } from "@/lib/utils";
import { LOGON_HEADING } from "./logon-styles";

/** The logon window's heading, the key leading it, so the text, notices and buttons under it share one left edge. */
export function LogonHeading({ id, children }: { id: string; children: ReactNode }) {
  return (
    <h1 id={id} className={cn(LOGON_HEADING, "flex items-center gap-3")}>
      <PixelIcon sprite={KEY} className="size-10" />
      {children}
    </h1>
  );
}
