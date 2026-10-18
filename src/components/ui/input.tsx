import { forwardRef, type InputHTMLAttributes, type SelectHTMLAttributes } from "react";
import { cn } from "@/lib/utils";

/**
 * Text fields at app density: 2.25rem, glass fill, an input ring; focus shows the jewel ring
 * with a soft glow. `size="sm"` (2rem) for fields inside panels and rows. Use `mono` for
 * addresses, images, tokens.
 */
export const fieldClass = (size: "default" | "sm" = "default", mono = false) =>
  cn(
    "w-full rounded-control border-0 bg-glass text-foreground shadow-[inset_0_0_0_1px_var(--input)] outline-none transition-shadow placeholder:text-faint",
    "focus:shadow-[inset_0_0_0_1px_var(--jewel),0_0_0_3px_rgb(var(--jewel-rgb)/0.18)] disabled:opacity-50",
    size === "sm" ? "h-8 px-2.5 text-[0.75rem]" : "h-9 px-3 text-[0.8125rem]",
    mono && "font-mono",
  );

type FieldProps = { fieldSize?: "default" | "sm"; mono?: boolean };

export const Input = forwardRef<HTMLInputElement, InputHTMLAttributes<HTMLInputElement> & FieldProps>(function Input(
  { className, fieldSize = "default", mono = false, ...props },
  ref,
) {
  return <input ref={ref} className={cn(fieldClass(fieldSize, mono), className)} {...props} />;
});

export function Select({ className, fieldSize = "default", mono = false, ...props }: SelectHTMLAttributes<HTMLSelectElement> & FieldProps) {
  return <select className={cn(fieldClass(fieldSize, mono), "pr-8", className)} {...props} />;
}
