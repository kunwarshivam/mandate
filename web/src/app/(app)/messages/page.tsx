import type { Metadata } from "next";
import { ApiMessages } from "@/components/api-screens/messages";
import { BySource } from "@/components/api-screens/api-screens";
import { MessagesScreen } from "@/components/messages/messages-screen";

export const metadata: Metadata = { title: "Messages" };

export default function MessagesPage() {
  return <BySource fixtures={<MessagesScreen />} api={<ApiMessages />} />;
}
