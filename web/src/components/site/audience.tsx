import { NotYet, SiteSection } from "./parts";

export function Audience() {
  return (
    <SiteSection id="who-its-for" title="Who it's for">
      <div className="grid gap-10 sm:grid-cols-2 sm:gap-14">
        <div className="grid content-start gap-3 border-t border-border/70 pt-6">
          <h3 className="text-h2">People with their own brokerage account</h3>
          <p className="max-w-measure text-pretty text-muted-foreground">
            You want an agent to do the trading, and you want the final say. You connect your account, write the mandate, and read every decision the gate made.
          </p>
        </div>
        <div className="grid content-start gap-3 border-t border-border/70 pt-6">
          <div className="flex flex-wrap items-center gap-x-3 gap-y-2">
            <h3 className="text-h2">Businesses</h3>
            <NotYet />
          </div>
          <p className="max-w-measure text-pretty text-muted-foreground">
            Organizations and workspaces, roles such as approver and auditor, limits a workspace can only tighten, single sign-on through OIDC, and hosting on your own servers.
          </p>
        </div>
      </div>
    </SiteSection>
  );
}
