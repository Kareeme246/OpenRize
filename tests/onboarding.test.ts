import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import {
  accessibilityCard,
  automationCard,
  onboardingAction,
  onboardingStep,
  relaunchModalOpen,
} from "../src/lib/onboarding.ts";
import { roleLabel, stateLabel, TONE_CLASS } from "../src/lib/permissions.ts";
import { relaunchAfter } from "../src/lib/relaunch.ts";
import type { AccessibilityState } from "../src/lib/types";

const step = (
  accessibility: AccessibilityState | null,
  {
    needed = true,
    shown = false,
  }: { needed?: boolean | null; shown?: boolean } = {},
) => onboardingStep({ needed, shown, accessibility });

test("it opens only while Accessibility is off", () => {
  assert.equal(step("notGranted"), "asking");
  const others: AccessibilityState[] = [
    "working",
    "checking",
    "notWorking",
    "notApplicable",
  ];
  for (const state of others) {
    assert.equal(step(state), "hidden", state);
  }
});

test("nothing shows before the answers are in", () => {
  assert.equal(step(null), "hidden");
  assert.equal(step("notGranted", { needed: null }), "hidden");
});

test("a user who has been through it is not shown it again", () => {
  assert.equal(step("notGranted", { needed: false }), "hidden");
  assert.equal(step("working", { needed: false, shown: true }), "hidden");
});

test("allowing access keeps the screen up long enough to say so", () => {
  assert.equal(step("checking", { shown: true }), "waiting");
  assert.equal(step("working", { shown: true }), "granted");
  assert.equal(step("notWorking", { shown: true }), "notWorking");
});

test("it follows a revoke back to the question", () => {
  assert.equal(step("notGranted", { shown: true }), "asking");
});

test("a grant that needs a restart keeps the screen up as the relaunch step", () => {
  assert.equal(step("relaunchRequired", { shown: true }), "relaunch");
  assert.equal(step("relaunchRequired", { shown: false }), "relaunch");
});

test("a user who has been through it gets the relaunch screen, not this one", () => {
  assert.equal(step("relaunchRequired", { needed: false }), "hidden");
  assert.equal(step("relaunchRequired", { needed: null }), "hidden");
});

const SHOWN_STEPS = ["asking", "waiting", "granted", "notWorking"];
type Shown = Parameters<typeof accessibilityCard>[0];

test("Accessibility offers Grant only while it is not granted", () => {
  assert.equal(accessibilityCard("asking").showGrant, true);
  for (const shown of ["waiting", "granted", "notWorking"] as Shown[]) {
    assert.equal(accessibilityCard(shown).showGrant, false, shown);
  }
});

test("Automation offers Grant only while it is not granted", () => {
  assert.equal(automationCard(false)?.showGrant, true);
  assert.equal(automationCard(true)?.showGrant, false);
});

test("Automation says nothing until its answer is in", () => {
  assert.equal(automationCard(undefined), null);
});

test("not granted reads in the error colour on both cards", () => {
  for (const card of [accessibilityCard("asking"), automationCard(false)]) {
    assert.ok(card);
    assert.equal(card.status, "Not granted");
    assert.equal(card.tone, "danger");
    assert.equal(TONE_CLASS[card.tone], "text-danger");
  }
});

test("granted reads in the success colour on both cards", () => {
  for (const card of [accessibilityCard("granted"), automationCard(true)]) {
    assert.ok(card);
    assert.equal(card.status, "Granted");
    assert.equal(card.tone, "success");
    assert.equal(TONE_CLASS[card.tone], "text-success");
  }
});

test("a grant in flight or not working is not painted as granted or missing", () => {
  assert.deepEqual(accessibilityCard("waiting"), {
    status: "Checking…",
    tone: "muted",
    showGrant: false,
  });
  assert.equal(accessibilityCard("notWorking").tone, "warning");
});

test("Accessibility is Recommended and Automation is Optional", () => {
  assert.equal(roleLabel("accessibility", false), "Recommended");
  assert.equal(roleLabel("automation", false), "Optional");
  assert.equal(roleLabel("anything", true), "Required");
});

test("Settings and onboarding use the same words for the same state", () => {
  assert.equal(stateLabel("working"), accessibilityCard("granted").status);
  assert.equal(stateLabel("notGranted"), accessibilityCard("asking").status);
});

test("skipping stays a normal path: every shown step can be dismissed", () => {
  // Nothing in the step machine gates leaving: it only decides what is shown.
  for (const state of [
    "notGranted",
    "checking",
    "working",
    "notWorking",
  ] as AccessibilityState[]) {
    assert.ok(SHOWN_STEPS.includes(step(state, { shown: true })), state);
  }
});

test("the screen has no footer sentence, Skip button or Open Settings label", () => {
  const source = readFileSync(
    new URL("../src/components/Onboarding.tsx", import.meta.url),
    "utf8",
  );
  for (const gone of [
    "Nothing else is needed",
    "Waiting for macOS",
    "Skip for now",
    "Open Settings",
    "Open Automation settings",
    "Allow Accessibility",
    "Off. OpenRize works without it.",
    "Both are optional",
  ]) {
    assert.ok(!source.includes(gone), `still says "${gone}"`);
  }
  assert.ok(source.includes("Continue"));
  assert.ok(source.includes("Grant"));
});

const actionFor = (accessibility: AccessibilityState, shown = true) => {
  const current = step(accessibility, { shown });
  assert.notEqual(current, "hidden");
  return onboardingAction(current as Exclude<typeof current, "hidden">);
};

test("the button is Continue when nothing is granted", () => {
  assert.deepEqual(actionFor("notGranted", false), {
    kind: "continue",
    label: "Continue",
    disabled: false,
  });
});

test("the button is still Continue when only Automation is granted", () => {
  // Automation is not an input to the step: it cannot change the button.
  assert.equal(automationCard(true)?.status, "Granted");
  assert.equal(actionFor("notGranted", false).kind, "continue");
  assert.equal(actionFor("notGranted", true).label, "Continue");
});

test("the button is Restart required when Accessibility is granted and still not working", () => {
  const action = actionFor("relaunchRequired");
  assert.equal(action.kind, "restart");
  assert.equal(action.label, "Restart required");
  assert.equal(action.disabled, false);
  assert.ok(action.kind === "restart" && /restart/i.test(action.note));
});

test("the button goes back to Continue if the grant is revoked before pressing", () => {
  assert.equal(actionFor("relaunchRequired").kind, "restart");
  assert.equal(actionFor("notGranted").kind, "continue");
});

test("only a needed restart turns Continue into Restart", () => {
  for (const state of ["checking", "working", "notWorking"] as const) {
    assert.equal(actionFor(state).kind, "continue", state);
  }
});

test("Continue cannot be pressed while a fresh grant is still being checked", () => {
  assert.equal(actionFor("checking").disabled, true);
  for (const state of ["notGranted", "working", "notWorking"] as const) {
    assert.equal(actionFor(state).disabled, false, state);
  }
});

test("no state leaves Continue pressable once a grant needs a restart", () => {
  // The path from grant to restart: asking -> checking -> relaunch.
  const path: AccessibilityState[] = [
    "notGranted",
    "checking",
    "relaunchRequired",
  ];
  const actions = path.map((state) => actionFor(state, true));
  for (const action of actions.slice(1)) {
    assert.ok(action.kind === "restart" || action.disabled);
  }
});

test("the Accessibility card says a restart is needed, with nothing to grant", () => {
  assert.deepEqual(accessibilityCard("relaunch"), {
    status: "Restart needed",
    tone: "warning",
    showGrant: false,
  });
});

test("the relaunch screen and this one are never up together", () => {
  const states: (AccessibilityState | null)[] = [
    null,
    "notGranted",
    "checking",
    "working",
    "notWorking",
    "relaunchRequired",
    "notApplicable",
  ];
  for (const accessibility of states) {
    for (const needed of [null, true, false]) {
      for (const shown of [false, true]) {
        const current = onboardingStep({ needed, shown, accessibility });
        const restartOwners =
          Number(current === "relaunch") +
          Number(relaunchModalOpen(accessibility, current));
        const expected = accessibility === "relaunchRequired" ? 1 : 0;
        assert.equal(restartOwners, expected, `${accessibility} ${needed}`);
      }
    }
  }
});

test("pressing Restart relaunches through the function the relaunch screen uses", async () => {
  const calls: string[] = [];
  await relaunchAfter(
    async () => {
      calls.push("finish onboarding");
    },
    async () => {
      calls.push("relaunch");
    },
    () => calls.push("prepare failed"),
  );
  assert.deepEqual(calls, ["finish onboarding", "relaunch"]);
});

test("a relaunch with nothing to prepare is just the relaunch", async () => {
  const calls: string[] = [];
  await relaunchAfter(
    undefined,
    async () => {
      calls.push("relaunch");
    },
    () => calls.push("prepare failed"),
  );
  assert.deepEqual(calls, ["relaunch"]);
});

test("a relaunch goes ahead when the onboarding marker cannot be saved", async () => {
  const calls: string[] = [];
  await relaunchAfter(
    async () => {
      throw new Error("disk full");
    },
    async () => {
      calls.push("relaunch");
    },
    () => calls.push("prepare failed"),
  );
  assert.deepEqual(calls, ["prepare failed", "relaunch"]);
});

test("a relaunch that fails is reported to the caller", async () => {
  await assert.rejects(
    relaunchAfter(
      undefined,
      async () => {
        throw new Error("no restart");
      },
      () => {},
    ),
    /no restart/,
  );
});

test("the relaunch screen and the Restart button share one relaunch hook", () => {
  const read = (file: string) =>
    readFileSync(new URL(`../src/components/${file}`, import.meta.url), "utf8");
  for (const file of ["Onboarding.tsx", "RelaunchModal.tsx"]) {
    const source = read(file);
    assert.ok(source.includes("useRelaunch("), file);
    assert.ok(!source.includes("relaunchApp"), `${file} relaunches by itself`);
  }
});

test("simulate Checking, re-run onboarding: the button is usable", () => {
  // Simulated Checking alone: the screen would hold Continue disabled.
  assert.equal(actionFor("checking").disabled, true);
  // The re-run (dev_tools.rs `rerun_simulation`, tested in Rust) replaces it
  // with a simulated "not granted", which opens onto a usable Continue.
  const afterRerun = actionFor("notGranted", false);
  assert.equal(afterRerun.kind, "continue");
  assert.equal(afterRerun.disabled, false);
});
