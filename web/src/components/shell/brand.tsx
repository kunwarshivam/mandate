import { BrandLockup } from "@/components/brand/brand-owl";

/**
 * The Owlhead brand in the top header (DEC-452): the brand owl, and from `lg` up, where no sidebar
 * carries the brand, the wordmark beside it in the logo colour (`--logo`, DEC-203). Decorative,
 * because the link around it carries the name.
 */
export function Wordmark({ className }: { className?: string }) {
  return <BrandLockup wordmark="lg" className={className} />;
}
