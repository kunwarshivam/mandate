import { redirect } from "next/navigation";

/** Connections live in Settings now (DEC-2293). */
export default function ConnectionsPage() {
  redirect("/settings");
}
