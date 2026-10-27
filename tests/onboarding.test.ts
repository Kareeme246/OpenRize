import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import {
  accessibilityCard,
  automationCard,
  onboardingStep,
} from "../src/lib/onboarding.ts";
import { roleLabel, stateLabel, TONE_CLASS } from "../src/lib/permissions.ts";
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
    "relaunchRequired",
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

test("it steps aside for the relaunch screen", () => {
  assert.equal(step("relaunchRequired", { shown: true }), "hidden");
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
