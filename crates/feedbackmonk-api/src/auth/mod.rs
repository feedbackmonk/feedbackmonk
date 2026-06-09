//! Authentication helpers -- password hashing (argon2) and signed-cookie
//! admin sessions (HMAC-SHA256 over `tenant_id || issued_at`).

pub mod ops;
pub mod password;
pub mod session;

pub use ops::OpsAuth;
pub use session::{issue_session_cookie, AdminSession, SESSION_COOKIE_NAME};
