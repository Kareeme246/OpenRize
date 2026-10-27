import assert from "node:assert/strict";
import { test } from "node:test";
import { onboardingStep } from "../src/lib/onboarding.ts";

const step = (accessibility, { needed = true, shown = false } = {}) =>
  onboardingStep({ needed, shown, accessibility });

test("it opens only while Accessibility is off", () => {
  assert.equal(step("notGranted"), "asking");
  for (const state of [
    "working",
    "checking",
    "notWorking",
    "relaunchRequired",
    "notApplicable",
  ]) {
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
