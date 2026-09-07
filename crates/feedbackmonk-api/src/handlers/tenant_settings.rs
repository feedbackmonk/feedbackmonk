//! Tenant-facing Language settings (FR-FBR-38, Contract C38).
//!
//! ```text
//! tenant_settings_router(state):
//!   GET /api/v1/admin/settings/locale   -- read the tenant's language settings
//!   PUT /api/v1/admin/settings/locale   -- set them (both fields optional)
//! ```
//!
//! Behind `AdminSession` and wrapped in `bind_admin_routes` like every other
//! admin surface, so it is unreachable on a tenant or custom host (DEC-FBR-13:
//! admin lives on exactly one origin). No CORS layer — never a browser embed.
//!
//! ## What the two fields mean
//!
//! - `locale` — the tenant's UI language. One value doing two jobs
//!   (DEC-FBR-IMPL-31): the admin console's language, and the fallback language
//!   for an email whose recipient carries no `submitter_locale` of their own.
//!   `null` means *never chosen* — the console follows the browser and emails
//!   fall to English. It is deliberately not the same as `"en"`, which means
//!   *chose English* and should survive the admin opening the console abroad.
//! - `translate_outbound` — RESERVED for FR-FBR-40 (Stage 2). Persisted here so
//!   the setting survives; consulted by no send path in this stage.
//!
//! ## PUT semantics: absent ≠ null
//!
//! Both fields are optional, and the distinction is load-bearing: an ABSENT
//! field leaves the stored value alone, a field explicitly set to `null` CLEARS
//! it. That is what lets the SPA send one field at a time from two independent
//! controls without either silently resetting the other. [`FieldUpdate`] names
//! the three states — `Unchanged` / `Clear` / `Set` — because they really are
//! three, and a bare `Option` can only carry two.
//!
//! ## Why the 400 body is `{"code": "invalid_locale"}` and not `ApiError`
//!
//! `ApiError::BadRequest` renders `{"error": "<prose>"}`. Contract C38 specifies
//! a machine-readable `code`, so this handler builds the response directly — the
//! same reason the public submit path returns a bare structured 402 rather than
//! routing through `ApiError`.

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::json;

use feedbackmonk_i18n::Locale;

use crate::auth::session::AdminSession;
use crate::error::ApiError;
use crate::state::AppState;

/// Contract C38 wire shape, used by both GET and the PUT echo.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct LocaleSettingsResponse {
    /// A C34 canonical code, or `null` when the tenant has never chosen one.
    pub locale: Option<String>,
    /// FR-FBR-40 (Stage 2) reserve. Always `false` until W-E wires it.
    pub translate_outbound: bool,
}

/// What a PUT says about one nullable field: nothing, "clear it", or a value.
///
/// Three states, so three variants. `Option<Option<T>>` would encode the same
/// thing and read as an accident.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum FieldUpdate<T> {
    /// The field was absent — leave the stored value alone.
    #[default]
    Unchanged,
    /// The field was explicitly `null` — clear the stored value.
    Clear,
    /// The field carried a value.
    Set(T),
}

impl<'de, T: Deserialize<'de>> Deserialize<'de> for FieldUpdate<T> {
    /// Only ever reached when the field is PRESENT — `#[serde(default)]` on the
    /// struct field supplies `Unchanged` when it is absent. So this maps
    /// `null` → `Clear` and everything else → `Set`.
    fn deserialize<D: Deserializer<'de>>(de: D) -> Result<Self, D::Error> {
        Ok(match Option::<T>::deserialize(de)? {
            None => FieldUpdate::Clear,
            Some(v) => FieldUpdate::Set(v),
        })
    }
}

/// PUT body. Both fields optional; see the module docs on absent-vs-null.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct LocaleSettingsRequest {
    #[serde(default)]
    pub locale: FieldUpdate<String>,
    #[serde(default)]
    pub translate_outbound: Option<bool>,
}

pub fn tenant_settings_router(state: AppState) -> Router {
    Router::new()
        .route(
            "/api/v1/admin/settings/locale",
            get(get_locale_settings).put(put_locale_settings),
        )
        .with_state(state)
}

async fn get_locale_settings(
    State(state): State<AppState>,
    session: AdminSession,
) -> Result<Json<LocaleSettingsResponse>, ApiError> {
    Ok(Json(read_settings(&state, &session).await?))
}

async fn put_locale_settings(
    State(state): State<AppState>,
    session: AdminSession,
    Json(req): Json<LocaleSettingsRequest>,
) -> Result<Response, ApiError> {
    // Validation lives HERE, not in the schema: the shipped-locale vocabulary is
    // additive data (`i18n/locales.json`, DEC-FBR-15), so pinning it into a DB
    // constraint would turn adding a language into a migration. Exact-match —
    // this is a stored canonical value, not a browser preference to resolve.
    let write_locale: Option<Option<&str>> = match &req.locale {
        FieldUpdate::Unchanged => None,
        FieldUpdate::Clear => Some(None),
        FieldUpdate::Set(code) => {
            let trimmed = code.trim();
            if trimmed.is_empty() {
                // The HTML-form spelling of "no selection" — a clear, not a
                // validation failure.
                Some(None)
            } else if Locale::parse(trimmed).is_some() {
                Some(Some(trimmed))
            } else {
                return Ok(invalid_locale_response());
            }
        }
    };
    if let Some(value) = write_locale {
        state.tenants.set_locale(&session.scope, value).await?;
    }

    if let Some(enabled) = req.translate_outbound {
        state
            .tenants
            .set_translate_outbound(&session.scope, enabled)
            .await?;
    }

    // Echo the STORED state, not the request: a caller that sent one field sees
    // what both fields now are, which is the only reading that is true after a
    // partial update.
    Ok(Json(read_settings(&state, &session).await?).into_response())
}

async fn read_settings(
    state: &AppState,
    session: &AdminSession,
) -> Result<LocaleSettingsResponse, ApiError> {
    Ok(LocaleSettingsResponse {
        locale: state.tenants.get_locale(&session.scope).await?,
        translate_outbound: state.tenants.get_translate_outbound(&session.scope).await?,
    })
}

/// Contract C38's `400 { "code": "invalid_locale" }`.
fn invalid_locale_response() -> Response {
    (
        StatusCode::BAD_REQUEST,
        Json(json!({ "code": "invalid_locale" })),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(body: &str) -> LocaleSettingsRequest {
        serde_json::from_str(body).expect("valid body")
    }

    #[test]
    fn absent_and_null_locale_are_different_requests() {
        // Absent: leave the stored value alone.
        assert_eq!(parse("{}").locale, FieldUpdate::Unchanged);
        // Explicit null: clear it.
        assert_eq!(parse(r#"{"locale":null}"#).locale, FieldUpdate::Clear);
        // A value: set it.
        assert_eq!(
            parse(r#"{"locale":"de"}"#).locale,
            FieldUpdate::Set("de".to_string())
        );
    }

    #[test]
    fn translate_outbound_is_independently_optional() {
        let r = parse(r#"{"translate_outbound":true}"#);
        assert_eq!(
            r.locale,
            FieldUpdate::Unchanged,
            "a translate_outbound-only PUT must not touch locale"
        );
        assert_eq!(r.translate_outbound, Some(true));

        let r = parse(r#"{"locale":"fr"}"#);
        assert_eq!(
            r.translate_outbound, None,
            "a locale-only PUT must not touch translate_outbound"
        );
    }

    #[test]
    fn response_serialises_to_the_c38_shape() {
        let json = serde_json::to_value(LocaleSettingsResponse {
            locale: Some("pt-BR".into()),
            translate_outbound: false,
        })
        .unwrap();
        assert_eq!(json["locale"], "pt-BR");
        assert_eq!(json["translate_outbound"], false);

        let cleared = serde_json::to_value(LocaleSettingsResponse {
            locale: None,
            translate_outbound: true,
        })
        .unwrap();
        assert!(cleared["locale"].is_null());
        assert_eq!(cleared["translate_outbound"], true);
    }

    #[test]
    fn invalid_locale_response_is_a_400_with_a_machine_code() {
        let resp = invalid_locale_response();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }
}
