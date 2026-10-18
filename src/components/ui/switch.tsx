import { cn } from "@/lib/utils";

/** A dependency-free on/off switch: the jewel when on. */
export function Switch({
  checked,
  onCheckedChange,
  disabled,
  className,
  ...props
}: {
  checked: boolean;
  onCheckedChange: (checked: boolean) => void;
  disabled?: boolean;
  className?: string;
  "aria-label"?: string;
  id?: string;
}) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      disabled={disabled}
      onClick={() => {
        onCheckedChange(!checked);
      }}
      className={cn(
        "relative inline-flex h-5 w-9 shrink-0 items-center rounded-full transition-colors disabled:opacity-50",
        checked ? "bg-jewel-solid" : "bg-input",
        className,
      )}
      {...props}
    >
      <span
        className={cn("inline-block size-4 rounded-full switch-thumb shadow-sm transition-transform", checked ? "translate-x-[1.125rem]" : "translate-x-0.5")}
      />
    </button>
  );
}
