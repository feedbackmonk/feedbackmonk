// Type mirrors of Contract C12 (widget runtime config) and the existing P0
// submission endpoint. These shapes are authored by Worker A (this file) on
// both ends of the wire — the widget consumes them, and the Rust handler at
// crates/feedbackmonk-api/src/handlers/widget_config.rs emits them.
//
// Pre-authorized widening per GUIDE.md §8: new OPTIONAL fields may be added
// without contract-freeze ratification. Removals or renames require LD halt.

export type WidgetAuthMode = "auth" | "anonymous";
export type WidgetSubmissionKind = "bug" | "feature" | "question" | "other";

export interface WidgetBrand {
  primary_color: string;
  logo_url: string | null;
  footer_text: string | null;
}

export interface WidgetConfig {
  project_id: string;
  tenant_id: string;
  display_name: string;
  brand: WidgetBrand;
  auth_modes: WidgetAuthMode[];
  submission_kinds: WidgetSubmissionKind[];
  max_body_chars: number;
}

export interface SubmitFeedbackRequest {
  kind: WidgetSubmissionKind;
  subject: string;
  body: string;
  email?: string;
}

export interface SubmitFeedbackResponse {
  feedback_id: string;
  submitted_at: string;
}

export interface ApiError {
  code: string;
  message: string;
}

export interface MountOptions {
  jwt?: string;
  projectId?: string;
  apiBase?: string;
}
