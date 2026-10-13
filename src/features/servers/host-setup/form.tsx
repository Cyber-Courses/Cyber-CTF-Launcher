"use client";

import { type ReactNode } from "react";
import { ChevronDown, Cloud, ExternalLink, Server } from "lucide-react";
import { type RemoteProvider } from "@/lib/tauri";
import { cn } from "@/lib/utils";
import { openExternal } from "@/lib/failure";
import { fieldClass } from "@/components/ui/input";
import { useT } from "@/lib/i18n";

/** Proxmox's own icon mark (official media kit, unaltered), or a neutral mark for ESXi / AWS. */
export function HypervisorMark({ provider }: { provider: RemoteProvider }) {
  if (provider === "proxmox")
    // eslint-disable-next-line @next/next/no-img-element -- static export, plain asset
    return <img src="/brands/proxmox-icon.svg" alt="" className="size-5 object-contain" draggable={false} />;
  // VMware/Broadcom logos need Broadcom's approval, so ESXi gets a neutral mark until then.
  if (provider === "aws") return <Cloud className="size-4" />;
  return <Server className="size-4" />;
}

/** Trademark line, shown pinned at the bottom of the setup window. */
export function SetupTrademarks({ cloud = false }: { cloud?: boolean }) {
  const t = useT();
  if (cloud) {
    return <p className="text-[0.6875rem] leading-relaxed text-faint">{t("servers.setup.trademarks.cloud")}</p>;
  }
  return (
    <p className="text-[0.6875rem] leading-relaxed text-faint">
      {t("servers.setup.trademarks.proxmox")}{" "}
      <button
        onClick={() => openExternal("https://www.proxmox.com")}
        className="inline-flex items-center gap-0.5 underline-offset-2 hover:text-muted-foreground hover:underline"
      >
        proxmox.com <ExternalLink className="size-3" />
      </button>{" "}
      {t("servers.setup.trademarks.vmware")}
    </p>
  );
}

/** A wizard step: its icon, title and description over its body. It grows to the window's height
 *  (see the server-setup window), so its Nav sits at the bottom from step to step. */
export function Step({ icon: Icon, title, description, children }: { icon: typeof Server; title: string; description: string; children: ReactNode }) {
  return (
    <div className="mt-3 flex flex-1 flex-col">
      <div className="flex items-start gap-3.5">
        <span className="flex size-9 shrink-0 items-center justify-center rounded-control bg-glass-2 text-muted-foreground shadow-[inset_0_0_0_1px_var(--input)]">
          <Icon className="size-4" />
        </span>
        <div className="min-w-0">
          <h2 className="text-[1rem] font-medium tracking-tight text-foreground">{title}</h2>
          <p className="mt-0.5 text-[0.8125rem] leading-relaxed text-muted-foreground">{description}</p>
        </div>
      </div>
      <div className="mt-5 flex flex-1 flex-col">{children}</div>
    </div>
  );
}

/** A step's Back / Continue: pinned to the bottom of the window's scroller, so they stay in view
 *  however tall the step is; the step scrolls under them, behind a short fade. */
export function Nav({ left, right }: { left?: ReactNode; right: ReactNode }) {
  return (
    <div data-step-nav className="sticky bottom-0 z-10 -mx-2 mt-auto bg-[linear-gradient(to_bottom,transparent,var(--background)_1.5rem)] px-2 pt-6">
      <div className="flex items-center justify-between gap-2 border-t border-border bg-background pt-4 pb-5">
        <div>{left}</div>
        <div>{right}</div>
      </div>
    </div>
  );
}

/** A labelled field; its hint goes under the input, so fields side by side keep their labels
 *  and inputs on the same lines whatever the hints' lengths. */
export function Field({ label, hint, children }: { label: string; hint?: string; children: ReactNode }) {
  return (
    <label className="block space-y-1.5">
      <span className="block text-[0.75rem] font-medium text-muted-foreground">{label}</span>
      {children}
      {hint && <span className="block text-[0.6875rem] leading-snug text-faint">{hint}</span>}
    </label>
  );
}

/** A wizard text field: the shared glass field, mono by default (hosts, keys, ids). */
export function Input({ mono = true, className, ...props }: React.InputHTMLAttributes<HTMLInputElement> & { mono?: boolean }) {
  return <input spellCheck={false} {...props} className={cn(fieldClass("default", mono), className)} />;
}

export function Select({ className, children, ...props }: React.SelectHTMLAttributes<HTMLSelectElement>) {
  return (
    <div className="relative">
      <select {...props} className={cn(fieldClass(), "cursor-pointer appearance-none pr-9", className)}>
        {children}
      </select>
      <ChevronDown className="pointer-events-none absolute top-1/2 right-3 size-4 -translate-y-1/2 text-faint" />
    </div>
  );
}

/** A quiet inline box in a step (sign-in state, a key to copy): glass with a hairline ring. */
export function Note({ className, children }: { className?: string; children: ReactNode }) {
  return <div className={cn("rounded-control bg-glass px-3 py-2.5 text-[0.75rem] shadow-[inset_0_0_0_1px_var(--border)]", className)}>{children}</div>;
}

/** A provider mark in a small glass tile, leading a pick-one row. */
export function MarkTile({ children }: { children: ReactNode }) {
  return (
    <span className="grid size-8 place-items-center rounded-control bg-glass-2 text-muted-foreground shadow-[inset_0_0_0_1px_var(--input)]">{children}</span>
  );
}
