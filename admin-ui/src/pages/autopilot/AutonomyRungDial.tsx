import {
  AUTONOMY_RUNG_DESCRIPTIONS,
  AUTONOMY_RUNG_LABELS,
  type AutonomyRung,
} from "../../shared/types.gen";

// The graduated autonomy-rung dial (FR-FBR-21). Sets how far the agent walks
// before it needs a signature, sent at work-order CREATE (C22). This is a
// SECURITY control, not UX sugar — so it (a) surfaces what each rung
// AUTHORIZES inline (blast radius visible at the point of choice) and (b)
// offers only Rungs 1–3: Rung 0 means "no work order is ever created", which
// is not a thing you select when you are about to create one. Rendered as a
// radiogroup so it is keyboard-operable and screen-reader-labelled.

const SELECTABLE_RUNGS: AutonomyRung[] = [1, 2, 3];

export function AutonomyRungDial({
  value,
  onChange,
  disabled,
  idPrefix,
}: {
  value: AutonomyRung;
  onChange: (rung: AutonomyRung) => void;
  disabled?: boolean;
  idPrefix: string;
}) {
  return (
    <fieldset className="ap-rung-dial" disabled={disabled}>
      <legend>Autonomy rung</legend>
      <p className="muted ap-rung-hint">
        How far the agent may walk before it needs your signature. You approve
        every work order regardless of rung — the rung governs what happens{" "}
        <em>after</em> approval.
      </p>
      <div
        role="radiogroup"
        aria-label="Autonomy rung"
        className="ap-rung-options"
      >
        {SELECTABLE_RUNGS.map((rung) => {
          const selected = value === rung;
          const id = `${idPrefix}-rung-${rung}`;
          return (
            <label
              key={rung}
              htmlFor={id}
              className={`ap-rung-option ${selected ? "ap-rung-option-selected" : ""}`}
            >
              <input
                type="radio"
                id={id}
                name={`${idPrefix}-rung`}
                value={rung}
                checked={selected}
                onChange={() => onChange(rung)}
                disabled={disabled}
              />
              <span className="ap-rung-option-label">
                {AUTONOMY_RUNG_LABELS[rung]}
              </span>
              <span className="ap-rung-option-desc muted">
                {AUTONOMY_RUNG_DESCRIPTIONS[rung]}
              </span>
            </label>
          );
        })}
      </div>
    </fieldset>
  );
}
