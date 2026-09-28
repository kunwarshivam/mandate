import Link from "next/link";

export default function NotFound() {
  return (
    <section className="grid max-w-2xl gap-3">
      <h1 className="text-title">No such page</h1>
      <p className="text-muted-foreground">The address does not match a screen or an ID in this workspace.</p>
      <Link href="/" className="w-fit text-primary underline-offset-4 hover:underline">
        Go to the dashboard
      </Link>
    </section>
  );
}
