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
  // Embedder opt-in to console-log capture (default OFF — privacy-by-default
  // per DEC-FBR-02). When true, the widget patches `console.*` from mount into
  // a bounded ring buffer; the captured text is only ever SENT if the end-user
  // leaves the "Include diagnostic logs" consent checkbox on. Logs are sent raw
  // and PII-scrubbed server-side (the single canonical `feedbackmonk-tracing`
  // chokepoint — never a second scrub path).
  captureConsole?: boolean;
}

// One entry of the attachment-upload response (GUIDE §6 frozen contract):
// `POST …/feedback/:fb/attachments` → 200 + `AttachmentResult[]`.
export interface AttachmentResult {
  attachment_id: string;
  url: string;
}

// A user-attached image staged for upload. `file` is the CURRENT blob — it is
// replaced in place when the user redacts the image via the canvas tool.
export interface AttachmentInput {
  file: Blob;
  name: string;
}

// Captured diagnostic logs sent as multipart text parts alongside `files[]`.
// Both optional; server scrubs PII before persist.
export interface CapturedLogs {
  service_log?: string;
  console_log?: string;
}
