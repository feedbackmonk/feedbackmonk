import { useId, useState, type FormEvent } from "react";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import {
  registerRunnerToken,
  registerSigningKey,
} from "../../shared/ApiClient";
import { useToast } from "../../components/Toast";
import { useAdminProject } from "../autopilot/useAdminProject";
import { RunnerTokensList } from "./RunnerTokensList";
import { Trans } from "react-i18next";
import { useTranslation } from "../../i18n";

// /admin/settings/runner-tokens — FR-FBR-24 / Contract C25. The complete
// "enable a runner" surface for a project owner:
//   1. Register the PUBLIC half of a `runner`-class Ed25519 key (feedbackmonk
//      never holds the private key — DEC-FBR-04).
//   2. Mint a runner write-token client-side (`feedbackmonk-runner mint-token`),
//      then optionally register its {jti, label, expires_at} here for visibility.
//   3. List registered tokens + revoke a jti (the load-bearing lifecycle action).
//
// Structural security property surfaced in copy: a runner token authorizes ONLY
// runner transitions and can NEVER author `approved` (C22 inv. 2) — so even full
// token compromise cannot bypass the owner-approval gate. That is why automating
// runner-token lifecycle is safe.
export function RunnerTokens() {
  const { t } = useTranslation("admin");
  const project = useAdminProject();

  if (project.status === "pending") {
    return (
      <main className="runner-tokens-page" aria-busy="true">
        <header className="page-header">
          <h1>{t("admin.runnerTokens.title")}</h1>
        </header>
        <p className="muted">{t("admin.common.loading")}</p>
      </main>
    );
  }
  if (project.status === "error" || !project.projectId) {
    return (
      <main className="runner-tokens-page">
        <header className="page-header">
          <h1>{t("admin.runnerTokens.title")}</h1>
        </header>
        <div role="alert" className="error-block">
          {t("admin.common.noProjects")}
        </div>
      </main>
    );
  }
  return <RunnerTokensInner projectId={project.projectId} />;
}

function RunnerTokensInner({ projectId }: { projectId: string }) {
  const { t } = useTranslation("admin");
  return (
    <main className="runner-tokens-page" aria-labelledby="runner-tokens-title">
      <header className="page-header">
        <h1 id="runner-tokens-title">{t("admin.runnerTokens.title")}</h1>
      </header>

      <section
        className="runner-tokens-explainer"
        aria-label={t("admin.runnerTokens.explainerAria")}
      >
        <p>
          <Trans
            i18nKey="admin.runnerTokens.explainer"
            t={t}
            components={{
              strong1: <strong />,
              em: <em />,
              code: <code />,
              strong2: <strong />,
            }}
          />
        </p>
        <p className="runner-tokens-security">
          <Trans
            i18nKey="admin.runnerTokens.securityNote"
            t={t}
            components={{
              em: <em />,
              strong: <strong />,
            }}
          />
        </p>
      </section>

      <RegisterRunnerKeyForm projectId={projectId} />
      <RegisterRunnerTokenForm projectId={projectId} />
      <RunnerTokensList projectId={projectId} />
    </main>
  );
}

// ─── Register a runner-class signing key ────────────────────────────────────

function RegisterRunnerKeyForm({ projectId }: { projectId: string }) {
  const { t } = useTranslation("admin");
  const fieldId = useId();
  const { notify } = useToast();
  const [label, setLabel] = useState("");
  const [publicKey, setPublicKey] = useState("");
  const [inlineError, setInlineError] = useState<string | null>(null);

  const mutation = useMutation({
    mutationFn: () =>
      registerSigningKey(projectId, {
        public_key_base64: publicKey.trim(),
        label: label.trim(),
        key_class: "runner",
      }),
    onSuccess: (res) => {
      notify(
        t("admin.runnerTokens.keyForm.registered", { label: res.label }),
        "success",
      );
      setLabel("");
      setPublicKey("");
    },
    onError: () =>
      setInlineError(t("admin.runnerTokens.keyForm.registerFailed")),
  });

  function onSubmit(e: FormEvent) {
    e.preventDefault();
    setInlineError(null);
    if (!label.trim() || !publicKey.trim()) {
      setInlineError(t("admin.runnerTokens.keyForm.required"));
      return;
    }
    mutation.mutate();
  }

  return (
    <section
      className="runner-key-register"
      aria-labelledby={`${fieldId}-heading`}
    >
      <h2 id={`${fieldId}-heading`}>{t("admin.runnerTokens.keyForm.heading")}</h2>
      <p className="muted">
        <Trans
          i18nKey="admin.runnerTokens.keyForm.explainer"
          t={t}
          components={{
            strong: <strong />,
            code: <code />,
          }}
        />
      </p>
      <form onSubmit={onSubmit} className="runner-token-form">
        <label htmlFor={`${fieldId}-label`}>
          {t("admin.runnerTokens.keyForm.labelField")}
        </label>
        <input
          id={`${fieldId}-label`}
          type="text"
          value={label}
          onChange={(e) => setLabel(e.target.value)}
          maxLength={100}
          placeholder={t("admin.runnerTokens.labelPlaceholder")}
          autoComplete="off"
        />

        <label htmlFor={`${fieldId}-key`}>
          {t("admin.runnerTokens.keyForm.publicKeyLabel")}
        </label>
        <textarea
          id={`${fieldId}-key`}
          value={publicKey}
          onChange={(e) => setPublicKey(e.target.value)}
          rows={2}
          placeholder={t("admin.runnerTokens.keyForm.publicKeyPlaceholder")}
          autoComplete="off"
          spellCheck={false}
        />

        {inlineError ? (
          <p role="alert" className="error">
            {inlineError}
          </p>
        ) : null}

        <div className="runner-token-form-actions">
          <button type="submit" disabled={mutation.isPending}>
            {mutation.isPending
              ? t("admin.runnerTokens.keyForm.registering")
              : t("admin.runnerTokens.keyForm.submit")}
          </button>
        </div>
      </form>
    </section>
  );
}

// ─── Register an issued token (optional visibility bookkeeping) ──────────────

function RegisterRunnerTokenForm({ projectId }: { projectId: string }) {
  const { t } = useTranslation("admin");
  const fieldId = useId();
  const queryClient = useQueryClient();
  const { notify } = useToast();
  const [jti, setJti] = useState("");
  const [label, setLabel] = useState("");
  const [expiresAt, setExpiresAt] = useState("");
  const [inlineError, setInlineError] = useState<string | null>(null);

  const mutation = useMutation({
    mutationFn: () =>
      registerRunnerToken(projectId, {
        jti: jti.trim(),
        label: label.trim(),
        // datetime-local yields a local "YYYY-MM-DDTHH:mm"; widen to a real
        // instant so the server parses it as RFC3339. Empty ⇒ omit (optional).
        expires_at: expiresAt ? new Date(expiresAt).toISOString() : undefined,
      }),
    onSuccess: () => {
      notify(
        t("admin.runnerTokens.tokenForm.registered", { label: label.trim() }),
        "success",
      );
      setJti("");
      setLabel("");
      setExpiresAt("");
      queryClient.invalidateQueries({
        queryKey: ["runner-tokens", projectId],
      });
    },
    onError: () =>
      setInlineError(t("admin.runnerTokens.tokenForm.registerFailed")),
  });

  function onSubmit(e: FormEvent) {
    e.preventDefault();
    setInlineError(null);
    if (!jti.trim() || !label.trim()) {
      setInlineError(t("admin.runnerTokens.tokenForm.required"));
      return;
    }
    mutation.mutate();
  }

  return (
    <section
      className="runner-token-register"
      aria-labelledby={`${fieldId}-heading`}
    >
      <h2 id={`${fieldId}-heading`}>{t("admin.runnerTokens.tokenForm.heading")}</h2>
      <p className="muted">
        <Trans
          i18nKey="admin.runnerTokens.tokenForm.explainer"
          t={t}
          components={{
            code1: <code />,
            code2: <code />,
          }}
        />
      </p>
      <form onSubmit={onSubmit} className="runner-token-form">
        <label htmlFor={`${fieldId}-jti`}>
          {t("admin.runnerTokens.tokenForm.jtiLabel")}
        </label>
        <input
          id={`${fieldId}-jti`}
          type="text"
          value={jti}
          onChange={(e) => setJti(e.target.value)}
          maxLength={200}
          placeholder={t("admin.runnerTokens.tokenForm.jtiPlaceholder")}
          autoComplete="off"
          spellCheck={false}
        />

        <label htmlFor={`${fieldId}-label`}>
          {t("admin.runnerTokens.tokenForm.labelField")}
        </label>
        <input
          id={`${fieldId}-label`}
          type="text"
          value={label}
          onChange={(e) => setLabel(e.target.value)}
          maxLength={100}
          placeholder={t("admin.runnerTokens.labelPlaceholder")}
          autoComplete="off"
        />

        <label htmlFor={`${fieldId}-exp`}>
          {t("admin.runnerTokens.tokenForm.expiresLabel")}
        </label>
        <input
          id={`${fieldId}-exp`}
          type="datetime-local"
          value={expiresAt}
          onChange={(e) => setExpiresAt(e.target.value)}
        />

        {inlineError ? (
          <p role="alert" className="error">
            {inlineError}
          </p>
        ) : null}

        <div className="runner-token-form-actions">
          <button type="submit" disabled={mutation.isPending}>
            {mutation.isPending
              ? t("admin.runnerTokens.tokenForm.registering")
              : t("admin.runnerTokens.tokenForm.submit")}
          </button>
        </div>
      </form>
    </section>
  );
}
