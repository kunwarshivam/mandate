import type { Metadata } from "next";
import { Stub } from "@/components/screens/stub";

export const metadata: Metadata = { title: "Settings" };

export default function SettingsPage() {
  return (
    <Stub title="Settings">
      <p>Not built in this slice. It will hold notification channels and quiet hours, members and roles, policies, and connections.</p>
      <p>The theme switch is in the header; it is the only preference this app stores on your device.</p>
    </Stub>
  );
}
