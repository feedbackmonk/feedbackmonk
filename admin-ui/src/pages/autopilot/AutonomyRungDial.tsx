import { type AutonomyRung } from "../../shared/types.gen";
import { Trans } from "react-i18next";
import { useTranslation } from "../../i18n";
import { useAdminLabels } from "../../i18n/useAdminLabels";

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
  const { t } = useTranslation("admin");
  const adminLabels = useAdminLabels();
  return (
    <fieldset className="ap-rung-dial" disabled={disabled}>
      <legend>{t("admin.autonomyRungDial.legend")}</legend>
      <p className="muted ap-rung-hint">
        <Trans
          i18nKey="admin.autonomyRungDial.hint"
          t={t}
          components={{ em: <em /> }}
        />
      </p>
      <div
        role="radiogroup"
        aria-label={t("admin.autonomyRungDial.legend")}
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
                {adminLabels.autonomyRungLabel(rung)}
              </span>
              <span className="ap-rung-option-desc muted">
                {adminLabels.autonomyRungDescription(rung)}
              </span>
            </label>
          );
        })}
      </div>
    </fieldset>
  );
}
