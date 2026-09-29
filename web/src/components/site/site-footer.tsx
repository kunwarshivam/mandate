import Link from "next/link";
import { cn } from "@/lib/utils";
import { LINK, RULE } from "./letter";
import { Badges } from "./retro";

/** The homepage's footer: its badges, the plain terms the page is offered on, and the way back up. */
export function SiteFooter() {
  return (
    <footer className="px-4 pb-8 text-[0.9375rem] leading-[1.55] text-muted-foreground sm:px-8">
      <hr className={cn(RULE, "mt-0 sm:mt-0")} />
      <div className="grid justify-items-center gap-4 text-center">
        <Badges className="justify-center" />
        <p className="max-w-[36rem] text-pretty">Owlhead is in private beta. It is software, not investment advice. Trading involves risk, and you can lose money.</p>
        <p>
          © 2026 Owlhead.{" "}
          <Link href="/login" className={LINK}>
            Sign in
          </Link>
          {" | "}
          <a href="#top" className={LINK}>
            Back to top
          </a>
        </p>
        <p className="text-sm">Best viewed in any browser, at any size.</p>
      </div>
    </footer>
  );
}
