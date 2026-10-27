import assert from "node:assert/strict";
import { test } from "node:test";
import {
  type AccessibilityChoice,
  type AutomationChoice,
  accessibilityChoice,
  accessibilityFromChoice,
  automationChoice,
  automationFromChoice,
  DEV_TOOLS,
  isReleaseIdentifier,
  tccResetCommand,
} from "../src/lib/devTools.ts";
import type { AccessibilityState } from "../src/lib/types";

const ACCESSIBILITY_CHOICES: AccessibilityChoice[] = [
  "real",
  "notGranted",
  "checking",
  "notWorking",
  "relaunchRequired",
  "working",
];

const AUTOMATION_CHOICES: AutomationChoice[] = ["real", "missing", "allowed"];

test("dev tools are off outside Vite's dev server", () => {
  assert.equal(DEV_TOOLS, false);
});

test("a real Accessibility state is the real choice", () => {
  assert.equal(accessibilityChoice(null), "real");
  assert.equal(accessibilityChoice("notApplicable"), "real");
});

test("every simulated Accessibility state round-trips through its picker key", () => {
  const states: AccessibilityState[] = [
    "notGranted",
    "checking",
    "notWorking",
    "relaunchRequired",
    "working",
  ];
  for (const state of states) {
    const choice = accessibilityChoice(state);
    assert.equal(choice, state);
    assert.equal(accessibilityFromChoice(choice), state);
  }
});

test("the real Accessibility choice simulates nothing", () => {
  assert.equal(accessibilityFromChoice("real"), null);
});

test("every Accessibility choice maps back to itself through a state", () => {
  for (const choice of ACCESSIBILITY_CHOICES) {
    const state = accessibilityFromChoice(choice);
    assert.equal(accessibilityChoice(state), choice);
  }
});

test("a real Automation grant is the real choice", () => {
  assert.equal(automationChoice(null), "real");
});

test("a simulated Automation grant round-trips through its picker key", () => {
  assert.equal(automationChoice(true), "allowed");
  assert.equal(automationChoice(false), "missing");
  assert.equal(automationFromChoice("allowed"), true);
  assert.equal(automationFromChoice("missing"), false);
  assert.equal(automationFromChoice("real"), null);
});

test("every Automation choice maps back to itself through a grant", () => {
  for (const choice of AUTOMATION_CHOICES) {
    assert.equal(automationChoice(automationFromChoice(choice)), choice);
  }
});

test("the revoke command names the build's own identifier", () => {
  assert.equal(
    tccResetCommand("com.offlinestudios.openrize.dev"),
    "tccutil reset Accessibility com.offlinestudios.openrize.dev",
  );
});

test("only the exact release identifier is refused", () => {
  const release = "com.offlinestudios.openrize";
  assert.equal(isReleaseIdentifier(release, release), true);
  assert.equal(
    isReleaseIdentifier("com.offlinestudios.openrize.dev", release),
    false,
  );
  assert.equal(
    isReleaseIdentifier("com.offlinestudios.openrizex", release),
    false,
  );
});
