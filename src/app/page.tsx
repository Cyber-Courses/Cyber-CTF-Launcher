import { AppShell } from "@/components/app-shell";
import { DevMockGate } from "@/components/dev-mock-gate";
import { WdioHooks } from "@/components/wdio-hooks";

export default function Home() {
  return (
    <DevMockGate>
      <WdioHooks />
      <AppShell />
    </DevMockGate>
  );
}
