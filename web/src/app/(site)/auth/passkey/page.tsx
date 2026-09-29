import type { Metadata } from "next";
import { PasskeyEnrol } from "@/components/auth/passkey-enrol";
import { SiteMain } from "@/components/site-frame/site-main";
import { authEnabled } from "@/lib/auth-config";
import { safeNext } from "@/lib/auth-routes";

export const metadata: Metadata = { title: "Add a passkey", robots: { index: false, follow: false } };

export default async function PasskeyPage({ searchParams }: { searchParams: Promise<Record<string, string | string[] | undefined>> }) {
  const { next } = await searchParams;
  return (
    <SiteMain>
      <PasskeyEnrol next={safeNext(Array.isArray(next) ? next[0] : next)} enabled={authEnabled} />
    </SiteMain>
  );
}
