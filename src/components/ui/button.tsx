import type { ButtonHTMLAttributes } from "react";
import { cn } from "@/lib/utils";

/**
 * The family Button: a pill with an inset highlight (cyber-design-system). Dependency-free.
 * - `default` (alias `primary`): the emerald jewel gradient with a soft glow.
 *   One per view, for the main action (Launch, Start lab, Submit).
 * - `outline` / `secondary`: ghost glass. Everyday actions.
 * - `inverted`: white pill (ink in Light). Rare, for a strong neutral action.
 * - `ghost` / `link`: text only. `destructive`: fail text on glass with a fail ring.
 * Press feedback is scale(.97). Sizes: lg 3rem, default 2.25rem, sm 2rem, xs 1.75rem (rows).
 */
type Variant = "default" | "primary" | "outline" | "secondary" | "inverted" | "ghost" | "destructive" | "link";
type Size = "default" | "sm" | "xs" | "lg" | "icon" | "icon-sm";

const BASE =
  "inline-flex shrink-0 items-center justify-center gap-1.5 rounded-full text-[0.8125rem] font-medium leading-none whitespace-nowrap select-none transition-[transform,box-shadow,background-color,color] duration-200 active:scale-[0.97] disabled:pointer-events-none disabled:opacity-45 [&_svg]:pointer-events-none [&_svg]:shrink-0 [&_svg:not([class*='size-'])]:size-4";

const VARIANTS: Record<Variant, string> = {
  default: "btn-jewel",
  primary: "btn-jewel",
  outline: "btn-glass",
  secondary: "btn-glass",
  inverted: "btn-inverted",
  ghost: "text-muted-foreground hover:bg-accent hover:text-foreground",
  destructive: "btn-stop",
  link: "rounded-none px-0 text-link underline-offset-4 hover:underline",
};

const SIZES: Record<Size, string> = {
  default: "h-9 px-4",
  sm: "h-8 px-3.5",
  // Compact, for actions inside dense list rows.
  xs: "h-7 px-3 text-[0.75rem] [&_svg:not([class*='size-'])]:size-3.5",
  lg: "h-12 px-6 text-[0.9375rem]",
  icon: "size-9",
  "icon-sm": "size-7 [&_svg:not([class*='size-'])]:size-3.5",
};

function buttonVariants({ variant = "default", size = "default", className }: { variant?: Variant; size?: Size; className?: string } = {}) {
  return cn(BASE, VARIANTS[variant], variant !== "link" && SIZES[size], className);
}

export function Button({
  variant = "default",
  size = "default",
  className,
  type = "button",
  ...props
}: ButtonHTMLAttributes<HTMLButtonElement> & { variant?: Variant; size?: Size }) {
  return <button type={type} className={buttonVariants({ variant, size, className })} {...props} />;
}
