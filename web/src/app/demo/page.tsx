import type { Metadata } from "next";
import { DemoApp } from "@/components/demo/demo-app";
import { buildWorkspace } from "@/fixtures/workspace";

export const metadata: Metadata = { title: "Example workspace" };

/**
 * The app over the example workspace, which the landing page's browser shows in its tab (DEC-906).
 * It is public, so it reads nothing from a request: no session, no cookie, no account, only the
 * fixture workspace with requests waiting. `src/components/demo/demo.test.tsx` holds it to that.
 */
export default function DemoPage() {
  return <DemoApp workspace={buildWorkspace("approvals")} />;
}
