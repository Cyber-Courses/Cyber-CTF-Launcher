// @vitest-environment jsdom
import { act, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { LabDetail } from "@/features/labs/lab-detail";
import type { Lab } from "@/features/labs/use-labs";
import type { LabStatus } from "@/lib/tauri";
import { installTauri, type Call, type Handler } from "@/test/tauri";

const lab: Lab = {
  id: "lab-goat",
  slug: "kube-goat",
  title: "Kube goat",
  description: null,
  question: "What is the password in db-credentials?",
  difficulty: 2,
  category: "Containers",
  runtime: { runtime: "DOCKER", architectures: ["arm64", "x86_64"], providers: ["aws"] },
  skills: [],
};
const stopped: LabStatus = {
  running: false,
  parked: null,
  machines: [],
  networks: [],
  url: null,
  host: null,
  expiresAt: null,
  place: null,
  provider: null,
  attacker: null,
  outputs: [],
  message: null,
};

const OBJECTIVES = [
  { key: "find-dashboard", title: "Find the dashboard", prompt: "Which port does the dashboard use?", optional: false, points: null, hintCount: 2, steps: [] },
  {
    key: "read-secret",
    title: "Read the secret",
    prompt: "What is the password in db-credentials?",
    optional: false,
    points: null,
    hintCount: 0,
    steps: [
      { key: "service-account", title: "Find the service account", prompt: "Which service account?", hintCount: 1 },
      { key: "namespace", title: "Find the namespace", prompt: "Which namespace?", hintCount: 0 },
    ],
  },
  { key: "node-escape", title: "Reach the node", prompt: "Read the node's hostname.", optional: true, points: null, hintCount: 0, steps: [] },
];
const HINTS: Record<string, string[]> = {
  "find-dashboard": ["List the services.", "443 maps to the container port."],
  "service-account": ["Read the deployment."],
};
const ANSWERS: Record<string, string> = { "find-dashboard": "8443", "read-secret": "s3cr3t", "service-account": "dashboard", namespace: "prod", main: "42" };

/** A small CyberBackend: objectives, the player's progress, and the two mutations. */
function backend({ objectives = OBJECTIVES as unknown[], signedIn = true, solved = [] as string[] } = {}) {
  const progress = { solved: [...solved], stepsOpened: [] as string[], hints: [] as { key: string; index: number; text: string }[] };
  const required = () => (objectives.length ? (objectives as { key: string; optional: boolean }[]).filter((o) => !o.optional).map((o) => o.key) : ["main"]);
  const summary = () => {
    const r = required();
    const n = r.filter((k) => progress.solved.includes(k)).length;
    return { ...progress, requiredSolved: n, requiredTotal: r.length, completed: n === r.length };
  };
  const api: Handler = (a) => {
    const q = String(a?.operation);
    const v = (a?.variables ?? {}) as Record<string, unknown>;
    if (q === "submitObjective") {
      const key = String(v.key);
      const was = progress.solved.includes(key);
      const correct = String(v.answer).trim() === ANSWERS[key];
      if (correct && !was) progress.solved.push(key);
      return { submitObjective: { correct, alreadySolved: was, labCompleted: summary().completed } };
    }
    if (q === "openObjectiveHelp") {
      const key = String(v.key);
      if (v.what === "STEPS") {
        progress.stepsOpened.push(key);
        return { openObjectiveHelp: { ok: true, hint: null } };
      }
      const text = HINTS[key][Number(v.index)];
      progress.hints.push({ key, index: Number(v.index), text });
      return { openObjectiveHelp: { ok: true, hint: text } };
    }
    if (q === "labProgress") return { labs: [{ id: lab.id, myProgress: signedIn ? summary() : null }] };
    if (q === "labObjectives") return { labs: [{ id: lab.id, objectives }] };
    return {};
  };
  return api;
}

const flush = () =>
  act(async () => {
    await new Promise((r) => setTimeout(r, 30));
  });

function renderLab(api: Handler, props: Partial<Parameters<typeof LabDetail>[0]> = {}): Call[] {
  const calls = installTauri({ lab_status: () => stopped, deploying_labs: () => [], active_operations: () => [], api_query: api });
  render(
    <LabDetail
      lab={lab}
      status={stopped}
      busy={false}
      logs={[]}
      times={[]}
      loggedIn
      hostArch="x86_64"
      onBack={vi.fn()}
      onStart={vi.fn()}
      onStop={vi.fn()}
      {...props}
    />,
  );
  return calls;
}

const panel = () => {
  const heading = screen.getByRole("heading", { name: "Objectives" });
  return within(heading.closest(".surface-panel") as HTMLElement);
};
const mutations = (calls: Call[]) =>
  calls.filter((c) => c.cmd === "api_query" && ["submitObjective", "openObjectiveHelp"].includes(String(c.args?.operation))).map((c) => c.args?.variables);

describe("Objectives panel", () => {
  it("lists the challenges with required progress and the optional badge", async () => {
    renderLab(backend({ solved: ["find-dashboard"] }));
    await flush();
    const p = panel();
    expect(p.getByText("1/2 required")).toBeTruthy();
    expect(p.getByRole("progressbar").getAttribute("aria-valuenow")).toBe("1");
    expect(p.getByText("optional")).toBeTruthy();
    expect(p.getAllByLabelText("Solved").length).toBeGreaterThan(0);
    // Three challenges: all open, one-shot (a prompt and an answer field each, none for the solved one).
    expect(p.getByText("Which port does the dashboard use?")).toBeTruthy();
    expect(p.queryByLabelText("Answer for Find the dashboard")).toBeNull();
    expect(p.getByLabelText("Answer for Read the secret")).toBeTruthy();
    expect(p.getByLabelText("Answer for Reach the node")).toBeTruthy();
    // Guided steps stay hidden until asked for.
    expect(p.queryByText("Find the service account")).toBeNull();
  });

  it("submits an answer and says whether it was right", async () => {
    const calls = renderLab(backend());
    await flush();
    const user = userEvent.setup();
    const p = panel();
    const field = p.getByLabelText("Answer for Find the dashboard");
    await user.type(field, "80");
    await user.click(within(field.closest("form") as HTMLElement).getByRole("button", { name: "Submit" }));
    await flush();
    expect(p.getByText("Not quite. Check it and try again.")).toBeTruthy();
    await user.clear(field);
    await user.type(field, "8443{Enter}");
    await flush();
    expect(p.getByText("Correct.")).toBeTruthy();
    expect(p.getByText("1/2 required")).toBeTruthy();
    expect(p.queryByLabelText("Answer for Find the dashboard")).toBeNull();
    expect(mutations(calls)).toEqual([
      { labId: "lab-goat", key: "find-dashboard", answer: "80" },
      { labId: "lab-goat", key: "find-dashboard", answer: "8443" },
    ]);
  });

  it("completes the lab when the last required challenge is solved; optional ones don't count", async () => {
    renderLab(backend({ solved: ["find-dashboard"] }));
    await flush();
    const user = userEvent.setup();
    const p = panel();
    await user.type(p.getByLabelText("Answer for Read the secret"), "s3cr3t{Enter}");
    await flush();
    expect(p.getByText("Correct. That completes the lab.")).toBeTruthy();
    expect(p.getByText("Completed")).toBeTruthy();
    expect(p.getByText("Lab completed: every required challenge is solved.")).toBeTruthy();
    expect(p.queryByText(/required$/)).toBeNull();
    // The optional one can still be answered.
    expect(p.getByLabelText("Answer for Reach the node")).toBeTruthy();
  });

  it("opens guided steps on demand, records it, and answers a step by its own key", async () => {
    const calls = renderLab(backend());
    await flush();
    const user = userEvent.setup();
    const p = panel();
    expect(p.getAllByText(/Opening the steps or a hint is recorded/).length).toBeGreaterThan(0);
    await user.click(p.getByRole("button", { name: /Guided mode/ }));
    await flush();
    expect(p.getByText("Find the service account")).toBeTruthy();
    expect(p.getByText("Step 2")).toBeTruthy();
    // The challenge now reads as guided, and its own note is gone.
    expect(p.getByText("guided")).toBeTruthy();
    await user.type(p.getByLabelText("Answer for Find the service account"), "dashboard{Enter}");
    await flush();
    expect(p.getByText("Correct.")).toBeTruthy();
    expect(mutations(calls)).toEqual([
      { labId: "lab-goat", key: "read-secret", what: "STEPS", index: null },
      { labId: "lab-goat", key: "service-account", answer: "dashboard" },
    ]);
    // A step solve doesn't count toward the required total.
    expect(p.getByText("0/2 required")).toBeTruthy();
  });

  it("reveals hints one at a time, in order", async () => {
    const calls = renderLab(backend());
    await flush();
    const user = userEvent.setup();
    const p = panel();
    await user.click(p.getByRole("button", { name: /Reveal hint 1 of 2/ }));
    await flush();
    expect(p.getByText("List the services.")).toBeTruthy();
    await user.click(p.getByRole("button", { name: /Reveal hint 2 of 2/ }));
    await flush();
    expect(p.getByText("443 maps to the container port.")).toBeTruthy();
    expect(p.queryByRole("button", { name: /Reveal hint \d of 2/ })).toBeNull();
    expect(mutations(calls)).toEqual([
      { labId: "lab-goat", key: "find-dashboard", what: "HINT", index: 0 },
      { labId: "lab-goat", key: "find-dashboard", what: "HINT", index: 1 },
    ]);
  });

  it("shows hints opened earlier from the player's progress", async () => {
    const api = backend();
    const seeded: Handler = (a) => {
      const r = api(a) as { labs?: { myProgress?: { hints: unknown[] } }[] };
      if (a?.operation === "labProgress" && r.labs?.[0]?.myProgress)
        r.labs[0].myProgress.hints = [{ key: "find-dashboard", index: 0, text: "List the services." }];
      return r;
    };
    renderLab(seeded);
    await flush();
    const p = panel();
    expect(p.getByText("List the services.")).toBeTruthy();
    expect(p.getByRole("button", { name: /Reveal hint 2 of 2/ })).toBeTruthy();
  });

  it("a lab without objectives answers its question as the main challenge", async () => {
    const calls = renderLab(backend({ objectives: [] }));
    await flush();
    const user = userEvent.setup();
    const p = panel();
    expect(p.getByText("What is the password in db-credentials?")).toBeTruthy();
    expect(p.getByText("0/1 required")).toBeTruthy();
    expect(p.queryByRole("button", { name: /Guided mode/ })).toBeNull();
    await user.type(p.getByLabelText("Answer for Objective"), "42{Enter}");
    await flush();
    expect(p.getByText("Correct. That completes the lab.")).toBeTruthy();
    expect(mutations(calls)).toEqual([{ labId: "lab-goat", key: "main", answer: "42" }]);
  });

  it("signed out: shows the challenges but asks to sign in before answering", async () => {
    const calls = renderLab(backend({ signedIn: false }), { loggedIn: false });
    await flush();
    const p = panel();
    expect(p.getByText("Sign in to submit answers and track your progress.")).toBeTruthy();
    expect(p.getByLabelText("Answer for Find the dashboard").hasAttribute("disabled")).toBe(true);
    expect(p.getByRole("button", { name: /Guided mode/ }).hasAttribute("disabled")).toBe(true);
    expect(calls.some((c) => c.args?.operation === "labProgress")).toBe(false);
  });

  it("says why a submit failed", async () => {
    const api = backend();
    renderLab((a) => {
      if (a?.operation === "submitObjective") throw "API unreachable: timed out";
      return api(a);
    });
    await flush();
    const user = userEvent.setup();
    await user.type(panel().getByLabelText("Answer for Find the dashboard"), "8443{Enter}");
    await flush();
    expect(panel().getByText("Couldn't check the answer: API unreachable: timed out")).toBeTruthy();
  });

  it("an older backend without objectives keeps the plain question", async () => {
    renderLab((a) => {
      const q = String(a?.operation);
      if (q === "labObjectives" || q === "labProgress") throw 'Cannot query field "objectives" on type "Lab".';
      return {};
    });
    await flush();
    expect(screen.queryByRole("heading", { name: "Objectives" })).toBeNull();
    expect(screen.getByRole("heading", { name: "Objective" })).toBeTruthy();
    expect(screen.getByText("What is the password in db-credentials?")).toBeTruthy();
  });
});
