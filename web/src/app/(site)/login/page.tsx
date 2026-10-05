import type { Metadata } from "next";
import { LoginPanel } from "@/components/auth/login-panel";
import { Logon } from "@/components/site/logon";
import { authEnabled, emailSignInEnabled } from "@/lib/auth-config";
import { safeNext } from "@/lib/auth-routes";

export const metadata: Metadata = { title: "Sign in", robots: { index: false, follow: false } };

type Search = Promise<Record<string, string | string[] | undefined>>;

const first = (value: string | string[] | undefined) => (Array.isArray(value) ? value[0] : value);

export default async function LoginPage({ searchParams }: { searchParams: Search }) {
  const params = await searchParams;
  return (
    <Logon title="Sign in - Owlhead">
      <LoginPanel next={safeNext(first(params.next))} failed={first(params.error) === "1"} enabled={authEnabled} emailEnabled={emailSignInEnabled} />
    </Logon>
  );
}
