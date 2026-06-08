import { useState, type FormEvent } from "react";
import axios from "axios";
import { postLogin } from "../shared/ApiClient";
import { useRouter } from "../shared/router";

export function Login() {
  const { search, navigate } = useRouter();
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [showPassword, setShowPassword] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [submitting, setSubmitting] = useState(false);

  const nextParam = new URLSearchParams(search).get("next");

  async function onSubmit(e: FormEvent) {
    e.preventDefault();
    if (submitting) return;
    setError(null);
    setSubmitting(true);
    try {
      await postLogin({ email, password });
      navigate(nextParam && nextParam.startsWith("/") ? nextParam : "/feedback", {
        replace: true,
      });
    } catch (err) {
      if (axios.isAxiosError(err) && err.response?.status === 401) {
        setError("Invalid email or password.");
      } else if (axios.isAxiosError(err) && err.response?.status === 403) {
        setError("Tenant not yet verified. Check your inbox.");
      } else {
        setError("Login failed. Please try again.");
      }
    } finally {
      setSubmitting(false);
    }
  }

  return (
    <main className="login-page">
      <form className="login-card" onSubmit={onSubmit} noValidate>
        <h1>feedbackmonk Admin</h1>
        <p className="muted">Sign in to triage feedback.</p>

        <label htmlFor="login-email">Email</label>
        <input
          id="login-email"
          name="email"
          type="email"
          autoComplete="email"
          required
          value={email}
          onChange={(e) => setEmail(e.target.value)}
          disabled={submitting}
        />

        <label htmlFor="login-password">Password</label>
        <div style={{ display: "flex", gap: "0.5rem", alignItems: "stretch" }}>
          <input
            id="login-password"
            name="password"
            type={showPassword ? "text" : "password"}
            autoComplete="current-password"
            required
            value={password}
            onChange={(e) => setPassword(e.target.value)}
            disabled={submitting}
            style={{ flex: 1, minWidth: 0 }}
          />
          <button
            type="button"
            className="login-password-toggle"
            onClick={() => setShowPassword((v) => !v)}
            disabled={submitting}
            aria-label={showPassword ? "Hide password" : "Show password"}
            aria-pressed={showPassword}
            tabIndex={-1}
            style={{ flexShrink: 0 }}
          >
            {showPassword ? "Hide" : "Show"}
          </button>
        </div>

        {error ? (
          <p className="error" role="alert">
            {error}
          </p>
        ) : null}

        <button type="submit" disabled={submitting || !email || !password}>
          {submitting ? "Signing in…" : "Sign in"}
        </button>
      </form>
    </main>
  );
}
