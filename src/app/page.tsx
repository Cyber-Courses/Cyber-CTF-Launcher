import { AppShell } from "@/components/app-shell";
import { DevMockGate } from "@/components/dev-mock-gate";

export default function Home() {
  return (
    <DevMockGate>
      <AppShell />
    </DevMockGate>
  );
}
