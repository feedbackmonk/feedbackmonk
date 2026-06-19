import { useId, useState, type FormEvent } from "react";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import {
  registerRunnerToken,
  registerSigningKey,
} from "../../shared/ApiClient";
import { useToast } from "../../components/Toast";
import { useAdminProject } from "../autopilot/useAdminProject";
import { RunnerTokensList } from "./RunnerTokensList";

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
  const project = useAdminProject();

  if (project.status === "pending") {
    return (
      <main className="runner-tokens-page" aria-busy="true">
        <header className="page-header">
          <h1>Runner tokens</h1>
        </header>
        <p className="muted">Loading…</p>
      </main>
    );
  }
  if (project.status === "error" || !project.projectId) {
    return (
      <main className="runner-tokens-page">
        <header className="page-header">
          <h1>Runner tokens</h1>
        </header>
        <div role="alert" className="error-block">
          No projects configured.
        </div>
      </main>
    );
  }
  return <RunnerTokensInner projectId={project.projectId} />;
}

function RunnerTokensInner({ projectId }: { projectId: string }) {
  return (
    <main className="runner-tokens-page" aria-labelledby="runner-tokens-title">
      <header className="page-header">
        <h1 id="runner-tokens-title">Runner tokens</h1>
      </header>

      <section className="runner-tokens-explainer" aria-label="How runners authenticate">
        <p>
          A <strong>runner</strong> is an autonomous agent that polls for
          owner-approved work orders, runs them against your repository, and
          reports back. It authenticates with a short-lived{" "}
          <em>runner write-token</em> — a JWT you mint yourself from the private
          half of a registered <code>runner</code>-class key.{" "}
          <strong>feedbackmonk never holds your private key.</strong>
        </p>
        <p className="runner-tokens-security">
          A runner token can drive a <em>dispatched</em> order but can{" "}
          <strong>never approve one</strong> — approval is an admin-only action
          (Contract C22). So even a fully compromised runner token cannot bypass
          your approval gate, which is why issuing them is safe to automate.
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
      notify(`Runner key “${res.label}” registered.`, "success");
      setLabel("");
      setPublicKey("");
    },
    onError: () =>
      setInlineError(
        "Registration failed. The public key must be standard base64 of a " +
          "32-byte Ed25519 public key, and the label 1–100 characters.",
      ),
  });

  function onSubmit(e: FormEvent) {
    e.preventDefault();
    setInlineError(null);
    if (!label.trim() || !publicKey.trim()) {
      setInlineError("Both a label and a base64 public key are required.");
      return;
    }
    mutation.mutate();
  }

  return (
    <section
      className="runner-key-register"
      aria-labelledby={`${fieldId}-heading`}
    >
      <h2 id={`${fieldId}-heading`}>Register a runner key</h2>
      <p className="muted">
        Generate an Ed25519 keypair on the runner host and register the{" "}
        <strong>public</strong> half here as a <code>runner</code>-class key.
        Unlike an identity key, a runner key can only verify runner write-tokens
        — it can never mint an end-user identity.
      </p>
      <form onSubmit={onSubmit} className="runner-token-form">
        <label htmlFor={`${fieldId}-label`}>Label</label>
        <input
          id={`${fieldId}-label`}
          type="text"
          value={label}
          onChange={(e) => setLabel(e.target.value)}
          maxLength={100}
          placeholder="ci-runner"
          autoComplete="off"
        />

        <label htmlFor={`${fieldId}-key`}>Public key (base64)</label>
        <textarea
          id={`${fieldId}-key`}
          value={publicKey}
          onChange={(e) => setPublicKey(e.target.value)}
          rows={2}
          placeholder="base64 of the 32-byte raw Ed25519 public key"
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
            {mutation.isPending ? "Registering…" : "Register runner key"}
          </button>
        </div>
      </form>
    </section>
  );
}

// ─── Register an issued token (optional visibility bookkeeping) ──────────────

function RegisterRunnerTokenForm({ projectId }: { projectId: string }) {
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
      notify(`Token “${label.trim()}” registered.`, "success");
      setJti("");
      setLabel("");
      setExpiresAt("");
      queryClient.invalidateQueries({
        queryKey: ["runner-tokens", projectId],
      });
    },
    onError: () =>
      setInlineError(
        "Registration failed. Check the jti and label (1–200 / 1–100 chars).",
      ),
  });

  function onSubmit(e: FormEvent) {
    e.preventDefault();
    setInlineError(null);
    if (!jti.trim() || !label.trim()) {
      setInlineError("Both a token ID (jti) and a label are required.");
      return;
    }
    mutation.mutate();
  }

  return (
    <section
      className="runner-token-register"
      aria-labelledby={`${fieldId}-heading`}
    >
      <h2 id={`${fieldId}-heading`}>Register an issued token</h2>
      <p className="muted">
        Optional. After minting a token with{" "}
        <code>feedbackmonk-runner mint-token</code>, record its <code>jti</code>{" "}
        here so it appears in the list below and can be revoked. Registration is
        purely for visibility — a runner can authenticate whether or not its
        token is registered.
      </p>
      <form onSubmit={onSubmit} className="runner-token-form">
        <label htmlFor={`${fieldId}-jti`}>Token ID (jti)</label>
        <input
          id={`${fieldId}-jti`}
          type="text"
          value={jti}
          onChange={(e) => setJti(e.target.value)}
          maxLength={200}
          placeholder="the token's jti claim (a UUID)"
          autoComplete="off"
          spellCheck={false}
        />

        <label htmlFor={`${fieldId}-label`}>Label</label>
        <input
          id={`${fieldId}-label`}
          type="text"
          value={label}
          onChange={(e) => setLabel(e.target.value)}
          maxLength={100}
          placeholder="ci-runner"
          autoComplete="off"
        />

        <label htmlFor={`${fieldId}-exp`}>Expires (optional)</label>
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
            {mutation.isPending ? "Registering…" : "Register token"}
          </button>
        </div>
      </form>
    </section>
  );
}
