// The objectives (docs/lab-objectives.md in CyberBackend) of the dev mock's labs, with the
// player's progress, answered as the backend would.
import { argText } from "@/lib/dev-mock/command";

// For the preview: "Exposed dashboard" has three challenges, one optional, with guided steps and
// hints; every other lab keeps its implicit main. Answers here are sample values, checked in the
// browser only.
const kubeObjectives = [
  {
    key: "find-dashboard",
    title: "Find the dashboard",
    prompt: "Which port does the Kubernetes dashboard listen on inside the cluster?",
    optional: false,
    points: null,
    hintCount: 2,
    steps: [],
  },
  {
    key: "read-secret",
    title: "Read the database secret",
    prompt: "What is the password stored in the db-credentials secret?",
    optional: false,
    points: null,
    hintCount: 1,
    steps: [
      { key: "service-account", title: "Find the service account", prompt: "Which service account does the dashboard run as?", hintCount: 1 },
      { key: "namespace", title: "Find the namespace", prompt: "Which namespace holds db-credentials?", hintCount: 0 },
    ],
  },
  {
    key: "node-escape",
    title: "Reach the node",
    prompt: "Read /etc/hostname on the node the dashboard runs on.",
    optional: true,
    points: null,
    hintCount: 0,
    steps: [],
  },
];
const kubeAnswers: Record<string, string> = {
  "find-dashboard": "8443",
  "read-secret": "s3cr3t-db",
  "service-account": "kubernetes-dashboard",
  namespace: "prod",
  "node-escape": "worker-1",
  main: "1337.00",
};
const kubeHints: Partial<Record<string, string[]>> = {
  "find-dashboard": ["List the services in every namespace.", "The dashboard's service maps 443 to its container port."],
  "read-secret": ["The dashboard's token can read secrets in one namespace."],
  "service-account": ["Look at the dashboard deployment's spec."],
};
const objectiveProgress = { solved: [] as string[], stepsOpened: [] as string[], hints: [] as { key: string; index: number; text: string }[] };

function mockProgress(labId: string) {
  const challenges = labId === "lab-kube" ? kubeObjectives : [{ key: "main", optional: false }];
  const required = challenges.filter((c) => !c.optional);
  const requiredSolved = required.filter((c) => objectiveProgress.solved.includes(c.key)).length;
  return { ...objectiveProgress, requiredSolved, requiredTotal: required.length, completed: requiredSolved === required.length };
}

/** An answer to the objective `key`: right or not, and whether it completes the lab. */
function submit(labId: string, v: Record<string, unknown>) {
  const key = argText(v.key);
  const before = mockProgress(labId).completed;
  const alreadySolved = objectiveProgress.solved.includes(key);
  const correct = argText(v.answer).trim().toLowerCase() === kubeAnswers[key];
  if (correct && !alreadySolved) objectiveProgress.solved.push(key);
  return { submitObjective: { correct, alreadySolved, labCompleted: before || mockProgress(labId).completed } };
}

/** An objective's guided steps or one of its hints, opened. */
function openHelp(v: Record<string, unknown>) {
  const key = argText(v.key);
  if (v.what === "STEPS") {
    if (!objectiveProgress.stepsOpened.includes(key)) objectiveProgress.stepsOpened.push(key);
    return { openObjectiveHelp: { ok: true, hint: null } };
  }
  const index = Number(v.index);
  const hints = kubeHints[key] ?? [];
  const text = index >= 0 && index < hints.length ? hints[index] : "";
  if (!objectiveProgress.hints.some((h) => h.key === key && h.index === index)) objectiveProgress.hints.push({ key, index, text });
  return { openObjectiveHelp: { ok: true, hint: text } };
}

/** The objectives operation `op` with its variables `v`. */
export function objectivesAnswer(op: string, v: Record<string, unknown>): unknown {
  const labId = argText(v.id, argText(v.labId));
  if (op === "submitObjective") return submit(labId, v);
  if (op === "openObjectiveHelp") return openHelp(v);
  if (op === "labProgress") return { labs: [{ id: labId, myProgress: mockProgress(labId) }] };
  return { labs: [{ id: labId, objectives: labId === "lab-kube" ? kubeObjectives : [] }] };
}
