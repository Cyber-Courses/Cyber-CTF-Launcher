import { AppShell } from "@/components/app-shell";
import { DevMockGate } from "@/components/dev-mock-gate";
import { WdioLaunch } from "@/components/wdio-launch";
import { WdioAttack } from "@/components/wdio-attack";

export default function Home() {
  return (
    <DevMockGate>
      <WdioLaunch />
      <WdioAttack />
      <AppShell />
    </DevMockGate>
  );
}
