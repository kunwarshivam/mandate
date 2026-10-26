import type { Metadata } from "next";
import { GoLive } from "@/components/api-screens/go-live";

export const metadata: Metadata = { title: "Going live" };

/** The risks read before an agent trades, the limits counsel sets, and the request for real money (DEC-2134); every text a placeholder pending counsel. */
export default function GoLivePage() {
  return <GoLive />;
}
