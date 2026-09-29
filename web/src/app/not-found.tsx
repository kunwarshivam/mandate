import { AppFrame } from "@/components/shell/app-frame";
import NotFound from "./(app)/not-found";

/** An address that matches no route renders in the app's frame, as a screen's own not-found does. */
export default function RootNotFound() {
  return (
    <AppFrame>
      <NotFound />
    </AppFrame>
  );
}
