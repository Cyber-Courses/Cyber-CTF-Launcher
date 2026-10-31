import { AppShell } from "@/components/app-shell";
import { DevMockGate } from "@/components/dev-mock-gate";
import { WdioLaunch } from "@/components/wdio-launch";

export default function Home() {
  return (
    <DevMockGate>
      <WdioLaunch />
      <AppShell />
    </DevMockGate>
  );
}
