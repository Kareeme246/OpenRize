import {
  type KeyboardEvent,
  type ReactNode,
  useCallback,
  useEffect,
  useId,
  useRef,
  useState,
} from "react";

function OptionDot({ color, size = 7 }: { color: string; size?: number }) {
  return (
    <span
      aria-hidden="true"
      className="inline-block shrink-0 rounded-full"
      style={{ backgroundColor: color, width: size, height: size }}
    />
  );
}

export interface PickerOption<T extends string | number> {
  value: T;
  label: string;
  disabled?: boolean;
  color?: string;
  description?: string;
}

export interface PickerGroup<T extends string | number> {
  group: string;
  options: PickerOption<T>[];
}

export type PickerItem<T extends string | number> =
  | PickerOption<T>
  | PickerGroup<T>;

function isGroup<T extends string | number>(
  item: PickerItem<T>,
): item is PickerGroup<T> {
  return "group" in item && Array.isArray(item.options);
}

function flattenOptions<T extends string | number>(
  items: PickerItem<T>[],
): PickerOption<T>[] {
  const result: PickerOption<T>[] = [];
  for (const item of items) {
    if (isGroup(item)) {
      for (const opt of item.options) {
        result.push(opt);
      }
    } else {
      result.push(item);
    }
  }
  return result;
}

export interface PickerProps<T extends string | number> {
  id?: string;
  name?: string;
  label?: string;
  "aria-label"?: string;
  ariaLabel?: string;
  value: T;
  options: PickerItem<T>[];
  onChange: (value: T) => void;
  placeholder?: string;
  disabled?: boolean;
  variant?: "field" | "filter" | "compact" | "inline";
  className?: string;
  buttonClassName?: string;
  color?: string;
  displayColor?: string;
  icon?: ReactNode;
}

export function Picker<T extends string | number>({
  id,
  label,
  "aria-label": ariaLabelProp,
  ariaLabel: ariaLabelAlt,
  value,
  options,
  onChange,
  placeholder,
  disabled = false,
  variant = "field",
  className = "",
  buttonClassName = "",
  color,
  displayColor: displayColorProp,
  icon,
}: PickerProps<T>) {
  const ariaLabel = ariaLabelProp || ariaLabelAlt;
  const explicitColor = displayColorProp || color;
  const [isOpen, setIsOpen] = useState(false);
  const [openUpward, setOpenUpward] = useState(false);
  const [highlightedIndex, setHighlightedIndex] = useState(-1);

  const containerRef = useRef<HTMLDivElement>(null);
  const triggerRef = useRef<HTMLButtonElement>(null);
  const listboxRef = useRef<HTMLDivElement>(null);

  const listboxId = useId();
  const flatOptions = flattenOptions(options);

  const selectedIndex = flatOptions.findIndex(
    (opt) => opt.value === value && !opt.disabled,
  );
  const selectedOption = flatOptions[selectedIndex];

  // Close when clicking outside or pressing Escape
  useEffect(() => {
    if (!isOpen) return;

    function handlePointerDown(event: PointerEvent) {
      if (
        containerRef.current &&
        !containerRef.current.contains(event.target as Node)
      ) {
        setIsOpen(false);
      }
    }

    function handleGlobalKeyDown(event: globalThis.KeyboardEvent) {
      if (event.key === "Escape") {
        event.preventDefault();
        event.stopPropagation();
        setIsOpen(false);
        triggerRef.current?.focus();
      }
    }

    document.addEventListener("pointerdown", handlePointerDown);
    document.addEventListener("keydown", handleGlobalKeyDown, true);
    return () => {
      document.removeEventListener("pointerdown", handlePointerDown);
      document.removeEventListener("keydown", handleGlobalKeyDown, true);
    };
  }, [isOpen]);

  // Viewport-aware open position (drop up if near viewport bottom)
  const openMenu = useCallback(() => {
    if (disabled) return;
    if (triggerRef.current) {
      const rect = triggerRef.current.getBoundingClientRect();
      const spaceBelow = window.innerHeight - rect.bottom;
      const spaceAbove = rect.top;
      // If less than 240px below and more room above, open upward
      setOpenUpward(spaceBelow < 240 && spaceAbove > spaceBelow);
    }
    setIsOpen(true);
    setHighlightedIndex(selectedIndex >= 0 ? selectedIndex : 0);
  }, [disabled, selectedIndex]);

  const closeMenu = useCallback(() => {
    setIsOpen(false);
    triggerRef.current?.focus();
  }, []);

  const selectOption = useCallback(
    (option: PickerOption<T>) => {
      if (option.disabled) return;
      onChange(option.value);
      setIsOpen(false);
      triggerRef.current?.focus();
    },
    [onChange],
  );

  // Keep highlighted option visible in scroll container
  useEffect(() => {
    if (!isOpen || highlightedIndex < 0 || !listboxRef.current) return;
    const element = listboxRef.current.querySelector(
      `[data-picker-index="${highlightedIndex}"]`,
    );
    if (element) {
      element.scrollIntoView({ block: "nearest" });
    }
  }, [isOpen, highlightedIndex]);

  // Keyboard navigation
  const handleKeyDown = (event: KeyboardEvent<HTMLButtonElement>) => {
    if (disabled) return;

    if (!isOpen) {
      if (
        event.key === "ArrowDown" ||
        event.key === "ArrowUp" ||
        event.key === "Enter" ||
        event.key === " "
      ) {
        event.preventDefault();
        openMenu();
      }
      return;
    }

    if (event.key === "Escape") {
      event.preventDefault();
      closeMenu();
      return;
    }

    if (event.key === "Tab") {
      setIsOpen(false);
      return;
    }

    if (event.key === "Enter" || event.key === " ") {
      event.preventDefault();
      if (highlightedIndex >= 0 && highlightedIndex < flatOptions.length) {
        const option = flatOptions[highlightedIndex];
        if (option && !option.disabled) {
          selectOption(option);
        }
      }
      return;
    }

    if (event.key === "ArrowDown") {
      event.preventDefault();
      let nextIndex = highlightedIndex + 1;
      while (
        nextIndex < flatOptions.length &&
        flatOptions[nextIndex]?.disabled
      ) {
        nextIndex++;
      }
      if (nextIndex >= flatOptions.length) {
        nextIndex = 0;
        while (
          nextIndex < flatOptions.length &&
          flatOptions[nextIndex]?.disabled
        ) {
          nextIndex++;
        }
      }
      if (nextIndex < flatOptions.length) {
        setHighlightedIndex(nextIndex);
      }
      return;
    }

    if (event.key === "ArrowUp") {
      event.preventDefault();
      let prevIndex = highlightedIndex - 1;
      while (prevIndex >= 0 && flatOptions[prevIndex]?.disabled) {
        prevIndex--;
      }
      if (prevIndex < 0) {
        prevIndex = flatOptions.length - 1;
        while (prevIndex >= 0 && flatOptions[prevIndex]?.disabled) {
          prevIndex--;
        }
      }
      if (prevIndex >= 0) {
        setHighlightedIndex(prevIndex);
      }
      return;
    }

    if (event.key === "Home") {
      event.preventDefault();
      const firstEnabled = flatOptions.findIndex((opt) => !opt.disabled);
      if (firstEnabled >= 0) setHighlightedIndex(firstEnabled);
      return;
    }

    if (event.key === "End") {
      event.preventDefault();
      for (let i = flatOptions.length - 1; i >= 0; i--) {
        if (!flatOptions[i]?.disabled) {
          setHighlightedIndex(i);
          break;
        }
      }
      return;
    }

    // Typeahead search
    if (event.key.length === 1 && !event.ctrlKey && !event.metaKey) {
      const char = event.key.toLowerCase();
      const matchIndex = flatOptions.findIndex(
        (opt, idx) =>
          idx > highlightedIndex &&
          !opt.disabled &&
          opt.label.toLowerCase().startsWith(char),
      );
      if (matchIndex >= 0) {
        setHighlightedIndex(matchIndex);
      } else {
        const wrapIndex = flatOptions.findIndex(
          (opt) => !opt.disabled && opt.label.toLowerCase().startsWith(char),
        );
        if (wrapIndex >= 0) setHighlightedIndex(wrapIndex);
      }
    }
  };

  // Determine display label
  const displayLabel =
    selectedOption?.label ??
    placeholder ??
    (value !== "" ? String(value) : "Select…");
  const displayColor = selectedOption?.color ?? explicitColor;

  // Variants styling
  let triggerClasses =
    "group flex items-center justify-between gap-1.5 cursor-pointer select-none text-left transition-colors outline-hidden";

  if (variant === "filter") {
    const active = value !== "";
    triggerClasses += ` h-7 w-36 truncate rounded-md border px-2 font-medium text-[12px] focus:border-accent ${
      active
        ? "border-accent/40 bg-accent-soft text-fg-strong"
        : "border-line bg-panel text-fg-soft hover:text-fg"
    }`;
  } else if (variant === "compact") {
    triggerClasses +=
      " h-7 rounded-lg border border-line bg-surface py-1.5 pl-2.5 pr-2 text-[12px] text-fg-muted hover:bg-surface-strong focus:border-accent/40";
  } else if (variant === "inline") {
    const empty = value === "";
    triggerClasses += ` h-6 min-w-0 max-w-full rounded-md border py-0 px-1.5 text-[11.5px] focus:border-accent disabled:cursor-default ${
      empty
        ? "border-dashed border-review/50 bg-transparent text-review"
        : "border-line-soft bg-transparent text-fg-muted hover:border-line"
    }`;
  } else {
    // "field" variant (default)
    triggerClasses +=
      " h-8 w-full rounded-md border border-line bg-surface px-2.5 py-1.5 text-[12px] text-fg focus:border-accent";
  }

  if (disabled) {
    triggerClasses += " pointer-events-none opacity-50";
  }

  const effectiveButtonClass = buttonClassName
    ? `${triggerClasses} ${buttonClassName}`
    : triggerClasses;

  let flatIndexCounter = 0;

  return (
    <div
      ref={containerRef}
      className={`relative inline-block ${variant === "field" ? "w-full" : ""} ${className}`}
    >
      <button
        ref={triggerRef}
        type="button"
        id={id}
        role="combobox"
        aria-haspopup="listbox"
        aria-expanded={isOpen}
        aria-controls={listboxId}
        aria-label={ariaLabel || label}
        aria-activedescendant={
          isOpen && highlightedIndex >= 0
            ? `${listboxId}-opt-${highlightedIndex}`
            : undefined
        }
        disabled={disabled}
        onClick={(event) => {
          event.stopPropagation();
          if (isOpen) closeMenu();
          else openMenu();
        }}
        onKeyDown={handleKeyDown}
        className={effectiveButtonClass}
      >
        <span className="flex min-w-0 flex-1 items-center gap-1.5 truncate">
          {displayColor && <OptionDot color={displayColor} size={7} />}
          {icon}
          <span className="truncate">{displayLabel}</span>
        </span>
        <svg
          viewBox="0 0 24 24"
          aria-hidden="true"
          fill="none"
          stroke="currentColor"
          strokeWidth={2.2}
          strokeLinecap="round"
          strokeLinejoin="round"
          className={`size-3 shrink-0 text-fg-faint transition-transform duration-150 ${
            isOpen ? "rotate-180 text-accent" : ""
          }`}
        >
          <path d="m6 9 6 6 6-6" />
        </svg>
      </button>

      {isOpen && (
        <div
          ref={listboxRef}
          id={listboxId}
          role="listbox"
          aria-label={ariaLabel || label}
          className={`absolute z-50 min-w-full max-h-60 w-max max-w-xs overflow-y-auto overscroll-contain rounded-lg border border-line bg-panel p-1 shadow-xl shadow-black/50 ${
            openUpward ? "bottom-full mb-1" : "top-full mt-1"
          }`}
        >
          {options.map((item, itemIdx) => {
            if (isGroup(item)) {
              return (
                <div key={item.group || itemIdx} className="py-1">
                  <div className="px-2 py-1 font-semibold text-[10px] uppercase tracking-wider text-fg-faint">
                    {item.group}
                  </div>
                  {item.options.map((opt) => {
                    const currentIndex = flatIndexCounter++;
                    const isSelected = opt.value === value;
                    const isHighlighted = currentIndex === highlightedIndex;

                    return (
                      <button
                        key={String(opt.value)}
                        type="button"
                        id={`${listboxId}-opt-${currentIndex}`}
                        role="option"
                        aria-selected={isSelected}
                        aria-disabled={opt.disabled}
                        data-picker-index={currentIndex}
                        disabled={opt.disabled}
                        onClick={() => selectOption(opt)}
                        onPointerEnter={() => setHighlightedIndex(currentIndex)}
                        className={`flex w-full cursor-pointer items-center justify-between gap-2 rounded-md px-2.5 py-1.5 text-left text-[12px] transition-colors ${
                          opt.disabled
                            ? "pointer-events-none opacity-40 text-fg-faint"
                            : isHighlighted
                              ? "bg-surface-strong text-fg-strong"
                              : isSelected
                                ? "bg-surface text-accent font-medium"
                                : "text-fg-muted hover:bg-surface hover:text-fg"
                        }`}
                      >
                        <span className="flex min-w-0 flex-1 items-center gap-2 truncate">
                          {opt.color && (
                            <OptionDot color={opt.color} size={7} />
                          )}
                          <span className="truncate">{opt.label}</span>
                        </span>
                        {isSelected && (
                          <svg
                            viewBox="0 0 24 24"
                            aria-hidden="true"
                            fill="none"
                            stroke="currentColor"
                            strokeWidth={2.5}
                            strokeLinecap="round"
                            strokeLinejoin="round"
                            className="size-3.5 shrink-0 text-accent"
                          >
                            <path d="M20 6 9 17l-5-5" />
                          </svg>
                        )}
                      </button>
                    );
                  })}
                </div>
              );
            }

            const currentIndex = flatIndexCounter++;
            const isSelected = item.value === value;
            const isHighlighted = currentIndex === highlightedIndex;

            return (
              <button
                key={String(item.value)}
                type="button"
                id={`${listboxId}-opt-${currentIndex}`}
                role="option"
                aria-selected={isSelected}
                aria-disabled={item.disabled}
                data-picker-index={currentIndex}
                disabled={item.disabled}
                onClick={() => selectOption(item)}
                onPointerEnter={() => setHighlightedIndex(currentIndex)}
                className={`flex w-full cursor-pointer items-center justify-between gap-2 rounded-md px-2.5 py-1.5 text-left text-[12px] transition-colors ${
                  item.disabled
                    ? "pointer-events-none opacity-40 text-fg-faint"
                    : isHighlighted
                      ? "bg-surface-strong text-fg-strong"
                      : isSelected
                        ? "bg-surface text-accent font-medium"
                        : "text-fg-muted hover:bg-surface hover:text-fg"
                }`}
              >
                <span className="flex min-w-0 flex-1 items-center gap-2 truncate">
                  {item.color && <OptionDot color={item.color} size={7} />}
                  <span className="truncate">{item.label}</span>
                </span>
                {isSelected && (
                  <svg
                    viewBox="0 0 24 24"
                    aria-hidden="true"
                    fill="none"
                    stroke="currentColor"
                    strokeWidth={2.5}
                    strokeLinecap="round"
                    strokeLinejoin="round"
                    className="size-3.5 shrink-0 text-accent"
                  >
                    <path d="M20 6 9 17l-5-5" />
                  </svg>
                )}
              </button>
            );
          })}
        </div>
      )}
    </div>
  );
}
