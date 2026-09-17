export interface SegmentedOption<T extends string | number> {
  value: T;
  label: string;
  /** Optional colour chip, used by the accent picker. */
  swatch?: string;
}

interface SegmentedControlProps<T extends string | number> {
  /** Shared radio group name; must be unique on the page. */
  name: string;
  value: T;
  options: SegmentedOption<T>[];
  onChange: (value: T) => void;
  disabled?: boolean;
}

/**
 * A horizontal radio group styled as one pill: segments separated by a
 * hairline, the checked segment washed in the accent colour so the highlight
 * "moves" between options. Sits right-justified inside a SettingRow.
 *
 * Built on real `<input type="radio">`, which gives arrow-key navigation and
 * screen-reader semantics for free - the input is visually hidden and the label
 * is the skin.
 */
export function SegmentedControl<T extends string | number>({
  name,
  value,
  options,
  onChange,
  disabled = false,
}: SegmentedControlProps<T>) {
  return (
    <div
      role="radiogroup"
      className="inline-flex shrink-0 overflow-hidden rounded-lg border border-line bg-surface"
    >
      {options.map((option, index) => {
        const checked = option.value === value;
        return (
          <label
            key={String(option.value)}
            className={`flex cursor-pointer items-center gap-1.5 px-2.5 py-1.5 text-[12px] transition-colors ${
              index > 0 ? "border-l border-line" : ""
            } ${
              checked
                ? "bg-accent-soft font-medium text-fg-strong"
                : "text-fg-muted hover:bg-surface-strong"
            } ${disabled ? "pointer-events-none opacity-40" : ""}`}
          >
            <input
              type="radio"
              name={name}
              value={String(option.value)}
              checked={checked}
              disabled={disabled}
              onChange={() => onChange(option.value)}
              className="sr-only"
            />
            {option.swatch !== undefined && (
              <span
                aria-hidden="true"
                className="size-3 shrink-0 rounded-full ring-1 ring-black/20"
                style={{ backgroundColor: option.swatch }}
              />
            )}
            {option.label}
          </label>
        );
      })}
    </div>
  );
}

interface ToggleProps {
  checked: boolean;
  onChange: (checked: boolean) => void;
  label: string;
  disabled?: boolean;
}

/** A switch: one boolean that applies the moment it is flipped. */
export function Toggle({
  checked,
  onChange,
  label,
  disabled = false,
}: ToggleProps) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      aria-label={label}
      disabled={disabled}
      onClick={() => onChange(!checked)}
      className={`relative h-6 w-11 shrink-0 rounded-full border transition-colors disabled:opacity-40 ${
        checked ? "border-accent/40 bg-accent-soft" : "border-line bg-surface"
      }`}
    >
      <span
        className={`absolute top-[3px] size-4 rounded-full transition-transform ${
          checked
            ? "left-[3px] translate-x-5 bg-accent"
            : "left-[3px] translate-x-0 bg-fg-ghost"
        }`}
      />
    </button>
  );
}
