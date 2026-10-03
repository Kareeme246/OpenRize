import { useState } from "react";
import { SegmentedControl } from "../../components/SegmentedControl";
import type { ThreadsLayout } from "./ThreadsView";

const KEY = "openrize.workflow.layout";
const OPTIONS = [
  { value: "lanes", label: "Lanes" },
  { value: "timeline", label: "Timeline" },
] satisfies { value: ThreadsLayout; label: string }[];

export function useWorkflowLayout() {
  const [layout, setLayout] = useState<ThreadsLayout>(() => {
    try {
      return window.localStorage.getItem(KEY) === "timeline"
        ? "timeline"
        : "lanes";
    } catch {
      return "lanes";
    }
  });
  const choose = (value: ThreadsLayout): void => {
    setLayout(value);
    try {
      window.localStorage.setItem(KEY, value);
    } catch {
      /* Viewer preference is best effort. */
    }
  };
  return { layout, choose };
}

export function WorkflowLayoutControl({
  name,
  layout,
  onChange,
}: {
  name: string;
  layout: ThreadsLayout;
  onChange: (value: ThreadsLayout) => void;
}) {
  return (
    <SegmentedControl
      name={name}
      value={layout}
      options={OPTIONS}
      onChange={onChange}
    />
  );
}
