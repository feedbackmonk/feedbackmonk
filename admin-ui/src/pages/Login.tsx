import { useState, type FormEvent } from "react";
import axios from "axios";
import { postLogin } from "../shared/ApiClient";
import { useRouter } from "../shared/router";
import { useTranslation } from "../i18n";

export function Login() {
  const { t } = useTranslation("admin");
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
        setError(t("admin.login.errors.invalidCredentials"));
      } else if (axios.isAxiosError(err) && err.response?.status === 403) {
        setError(t("admin.login.errors.tenantUnverified"));
      } else {
        setError(t("admin.login.errors.generic"));
      }
    } finally {
      setSubmitting(false);
    }
  }

  return (
    <main className="login-page">
      <form className="login-card" onSubmit={onSubmit} noValidate>
        <h1>{t("admin.login.title")}</h1>
        <p className="muted">{t("admin.login.subtitle")}</p>

        <label htmlFor="login-email">{t("admin.login.emailLabel")}</label>
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

        <label htmlFor="login-password">{t("admin.login.passwordLabel")}</label>
        <div style={{ position: "relative", display: "flex", alignItems: "center" }}>
          <input
            id="login-password"
            name="password"
            type={showPassword ? "text" : "password"}
            autoComplete="current-password"
            required
            value={password}
            onChange={(e) => setPassword(e.target.value)}
            disabled={submitting}
            style={{ width: "100%", paddingRight: "2.25rem" }}
          />
          <button
            type="button"
            className="login-password-toggle"
            onClick={() => setShowPassword((v) => !v)}
            disabled={submitting}
            aria-label={
              showPassword
                ? t("admin.login.hidePassword")
                : t("admin.login.showPassword")
            }
            aria-pressed={showPassword}
            title={
              showPassword
                ? t("admin.login.hidePassword")
                : t("admin.login.showPassword")
            }
            style={{
              position: "absolute",
              right: "0.5rem",
              display: "inline-flex",
              alignItems: "center",
              justifyContent: "center",
              width: "1.5rem",
              height: "1.5rem",
              padding: 0,
              border: "none",
              background: "transparent",
              color: "currentColor",
              opacity: 0.65,
              cursor: "pointer",
            }}
          >
            {showPassword ? (
              <svg
                width="18"
                height="18"
                viewBox="0 0 24 24"
                fill="none"
                stroke="currentColor"
                strokeWidth="2"
                strokeLinecap="round"
                strokeLinejoin="round"
                aria-hidden="true"
              >
                <path d="M17.94 17.94A10.07 10.07 0 0 1 12 20c-7 0-11-8-11-8a18.45 18.45 0 0 1 5.06-5.94M9.9 4.24A9.12 9.12 0 0 1 12 4c7 0 11 8 11 8a18.5 18.5 0 0 1-2.16 3.19m-6.72-1.07a3 3 0 1 1-4.24-4.24" />
                <line x1="1" y1="1" x2="23" y2="23" />
              </svg>
            ) : (
              <svg
                width="18"
                height="18"
                viewBox="0 0 24 24"
                fill="none"
                stroke="currentColor"
                strokeWidth="2"
                strokeLinecap="round"
                strokeLinejoin="round"
                aria-hidden="true"
              >
                <path d="M1 12s4-8 11-8 11 8 11 8-4 8-11 8-11-8-11-8z" />
                <circle cx="12" cy="12" r="3" />
              </svg>
            )}
          </button>
        </div>

        {error ? (
          <p className="error" role="alert">
            {error}
          </p>
        ) : null}

        <button type="submit" disabled={submitting || !email || !password}>
          {submitting ? t("admin.login.signingIn") : t("admin.login.signIn")}
        </button>
      </form>
    </main>
  );
}
