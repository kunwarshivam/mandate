import Link from "next/link";
import { OwlheadWordmark } from "@/components/brand/Logo";
import { Placeholder } from "@/components/domain/placeholders";
import { COLUMN, TEXT_LINK } from "./parts";

/** The landing page's own footer. Privacy and terms stay placeholders until counsel writes the pages. */
export function SiteFooter() {
  return (
    <footer className={COLUMN}>
      <div className="grid gap-8 border-t border-border/70 pt-10 pb-12 sm:pt-12 sm:pb-16">
        <div className="flex flex-wrap items-center justify-between gap-x-10 gap-y-6">
          <OwlheadWordmark className="h-6 w-auto text-foreground" />
          <nav aria-label="Site">
            <ul className="flex flex-wrap gap-x-6 gap-y-3 text-sm">
              <li>
                <a href="#how-it-works" className={TEXT_LINK}>
                  How it works
                </a>
              </li>
              <li>
                <a href="#faq" className={TEXT_LINK}>
                  Questions
                </a>
              </li>
              <li>
                <Link href="/login" className={TEXT_LINK}>
                  Sign in
                </Link>
              </li>
            </ul>
          </nav>
        </div>
        <div className="grid gap-4 text-caption text-muted-foreground">
          <p className="max-w-measure">
            <Placeholder name="siteDisclaimer" />
          </p>
          <div className="flex flex-wrap items-center gap-x-6 gap-y-3">
            <p>© 2026 Owlhead</p>
            <p className="flex flex-wrap gap-3">
              <Placeholder name="privacy" />
              <Placeholder name="terms" />
            </p>
          </div>
        </div>
      </div>
    </footer>
  );
}
