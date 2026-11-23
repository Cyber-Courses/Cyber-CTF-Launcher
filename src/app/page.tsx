import { AppShell } from "@/components/app-shell";
import { DevMockGate } from "@/components/dev-mock-gate";
import { SettingsGate } from "@/components/settings-gate";
import { WdioHooks } from "@/components/wdio-hooks";

export default function Home() {
  return (
    <DevMockGate>
      <SettingsGate>
        <WdioHooks />
        <AppShell />
      </SettingsGate>
    </DevMockGate>
  );
}
