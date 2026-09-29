"use client";

import Link from "next/link";
import { usePathname } from "next/navigation";

const ACTION =
  "press inline-flex h-11 shrink-0 items-center justify-center rounded-full border border-foreground/25 bg-card px-5 text-sm font-semibold outline-none hover:bg-background focus-visible:ring-3 focus-visible:ring-ring";

/** "Sign in" at the right of the public header; the sign-in pages themselves need no link to it. */
export function SiteAction({ signedIn }: { signedIn: boolean }) {
  const pathname = usePathname();
  if (pathname === "/login" || pathname.startsWith("/auth/")) return null;
  return signedIn ? (
    <Link href="/" className={ACTION}>
      Open Owlhead
    </Link>
  ) : (
    <Link href="/login" className={ACTION}>
      Sign in
    </Link>
  );
}
