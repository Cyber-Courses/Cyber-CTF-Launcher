import { describe, expect, it } from "vitest";
import { answer, scenarioFrom, scenarioReport } from "@/lib/dev-mock";
import type { LabStatus, ServerList, SystemReport } from "@/lib/tauri";

// The preview and the end-to-end tests run on the dev mock's answers: each has every field the
// core's answer has (the fixtures are typed against the generated types; this checks the values
// a screen reads are really there, not just declared).
describe("the dev mock answers like the core", () => {
  it("reports Docker access on the system check", () => {
    for (const r of [answer("system_check", undefined) as SystemReport, scenarioReport(scenarioFrom("?os=linux&machine=fresh"))]) {
      expect(r.dockerDenied).toBe(false);
      expect(r).toHaveProperty("dockerDeniedHint", null);
    }
  });

  it("gives every lab status its attacker, outputs and message", () => {
    for (const id of ["lab-invoice", "lab-goad"]) {
      const s = answer("lab_status", { id }) as LabStatus;
      expect(s).toHaveProperty("attacker");
      expect(Array.isArray(s.outputs)).toBe(true);
      expect(s).toHaveProperty("message");
      expect(s).toHaveProperty("reapError", null);
    }
  });

  it("gives every saved host its budget, project and credential fields", () => {
    for (const h of (answer("server_list", undefined) as ServerList).hosts) {
      for (const field of ["monthlyLimit", "gcpProject", "useCliCreds", "awsProfile"]) expect(h, `${h.id}.${field}`).toHaveProperty(field);
    }
  });
});
