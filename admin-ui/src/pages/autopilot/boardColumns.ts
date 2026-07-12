import {
  WORK_ORDER_STATE_LABELS,
  type WorkOrderState,
} from "../../shared/types.gen";

// C31 §7 — Kanban grouping for the read-only work-order board. Every one of the
// 10 `WorkOrderState` values maps to EXACTLY ONE of these 6 columns. The four
// execution states (dispatched/claimed/building/verifying — the `is_execution_state`
// set) collapse into a single "In flight" column; the two hold states
// (failed/cancelled) into "Halted".
//
// The `Record<WorkOrderState, …>` below is the exhaustiveness guard: adding a new
// state to the `WorkOrderState` union makes this Record incomplete and fails
// `tsc`, so a new state can never silently vanish from the board (the vitest in
// `__tests__/Board.test.tsx` is the runtime witness of the same invariant).

export type BoardColumnId =
  | "draft"
  | "approved"
  | "in-flight"
  | "reported"
  | "done"
  | "halted";

export interface BoardColumn {
  id: BoardColumnId;
  label: string;
}

// Display order, left→right. Mirrors the lifecycle flow.
export const BOARD_COLUMNS: readonly BoardColumn[] = [
  { id: "draft", label: "Draft" },
  { id: "approved", label: "Approved" },
  { id: "in-flight", label: "In flight" },
  { id: "reported", label: "Reported" },
  { id: "done", label: "Done" },
  { id: "halted", label: "Halted" },
] as const;

// The single source of truth for state→column. Exhaustive by construction.
const COLUMN_BY_STATE: Record<WorkOrderState, BoardColumnId> = {
  draft: "draft",
  approved: "approved",
  dispatched: "in-flight",
  claimed: "in-flight",
  building: "in-flight",
  verifying: "in-flight",
  reported: "reported",
  completed: "done",
  failed: "halted",
  cancelled: "halted",
};

export function columnForState(state: WorkOrderState): BoardColumnId {
  return COLUMN_BY_STATE[state];
}

// Every work-order state known at runtime, from the canonical label Record in
// types.gen.ts (the same Record `tsc` forces to stay exhaustive). Used by the
// board test to prove the mapping covers all states.
export function allWorkOrderStates(): WorkOrderState[] {
  return Object.keys(WORK_ORDER_STATE_LABELS) as WorkOrderState[];
}

// Group work orders into their columns, preserving input order within each
// column (callers pass newest-first). Generic over any object carrying `state`.
export function groupByColumn<T extends { state: WorkOrderState }>(
  orders: readonly T[],
): Record<BoardColumnId, T[]> {
  const groups: Record<BoardColumnId, T[]> = {
    draft: [],
    approved: [],
    "in-flight": [],
    reported: [],
    done: [],
    halted: [],
  };
  for (const o of orders) {
    groups[columnForState(o.state)].push(o);
  }
  return groups;
}
