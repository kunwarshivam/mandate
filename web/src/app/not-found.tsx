import Link from "next/link";

export default function NotFound() {
  return (
    <section className="grid max-w-3xl gap-3">
      <h1 className="text-title sm:text-display">No such page</h1>
      <p className="text-muted-foreground">The address does not match a screen or an ID in this workspace.</p>
      <Link href="/" className="w-fit font-bold text-primary underline underline-offset-4 hover:decoration-2">
        Go to the dashboard
      </Link>
    </section>
  );
}
