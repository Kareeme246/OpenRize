// Pins the app's clock to 4:41 PM on the seeded day, so the Calendar opens on that day with the
// "now" line under the last entry. Injected into index.html by capture.sh; never commit it.
(() => {
  const frozen = Date.parse("2026-09-28T16:41:00-05:00");
  const offset = frozen - Date.now();
  const Real = Date;
  class FrozenDate extends Real {
    constructor(...args) {
      if (args.length === 0) super(Real.now() + offset);
      else super(...args);
    }
    static now() {
      return Real.now() + offset;
    }
  }
  window.Date = FrozenDate;
})();
