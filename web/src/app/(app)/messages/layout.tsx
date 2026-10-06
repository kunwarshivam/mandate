import type { ReactNode } from "react";
import { ConversationsProvider } from "@/components/messages/conversations";

export default function MessagesLayout({ children }: { children: ReactNode }) {
  return <ConversationsProvider>{children}</ConversationsProvider>;
}
