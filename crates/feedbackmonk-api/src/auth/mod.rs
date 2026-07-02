//! Authentication helpers -- password hashing (argon2) and signed-cookie
//! admin sessions (HMAC-SHA256 over `tenant_id || issued_at || session_epoch`).

pub mod ops;
pub mod password;
pub mod session;

pub use ops::OpsAuth;
pub use session::{
    clear_session_cookie, issue_session_cookie, AdminSession, SESSION_COOKIE_NAME,
};
