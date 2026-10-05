import Link from "next/link";
import { BrandLockup } from "@/components/brand/brand-owl";
import { SiteAction } from "./site-action";

/**
 * The public pages' header: the brand, which opens `/`, and one action at the right. It is on the
 * frame's glass like the app's header, and carries none of the app's controls: no Stop, no paper
 * badge and no dock, because nothing here acts on an account.
 */
export function SiteHeader({ signedIn }: { signedIn: boolean }) {
  return (
    <header className="glass sticky top-0 z-30 shrink-0 border-b" data-slot="site-header">
      <div className="mx-auto flex h-16 w-full max-w-(--content-max) items-center justify-between gap-4 px-(--page-x)">
        <Link
          href="/"
          aria-label="Owlhead"
          className="inline-flex min-h-11 shrink-0 items-center px-1 outline-none focus-visible:ring-3 focus-visible:ring-ring"
        >
          <BrandLockup />
        </Link>
        <SiteAction signedIn={signedIn} />
      </div>
    </header>
  );
}
