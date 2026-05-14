import { useRouter } from "./shared/router";
import { Login } from "./pages/Login";
import { FeedbackList } from "./pages/FeedbackList";
import { FeedbackDrawer } from "./pages/FeedbackDrawer";

// Routes:
//   /login                         → Login
//   /feedback                      → FeedbackList
//   /feedback/FB-XXXXXX            → FeedbackList + FeedbackDrawer overlay
//   anything else                  → redirect to /feedback (or /login when API 401s)
export function App() {
  const { pathname, navigate } = useRouter();

  if (pathname === "/login") {
    return <Login />;
  }

  // Match /feedback or /feedback/{feedbackId}
  const feedbackMatch = pathname.match(/^\/feedback(?:\/([^/]+))?$/);
  if (feedbackMatch) {
    const feedbackId = feedbackMatch[1];
    return (
      <>
        <FeedbackList />
        {feedbackId ? (
          <FeedbackDrawer
            feedbackId={decodeURIComponent(feedbackId)}
            onClose={() => navigate("/feedback")}
          />
        ) : null}
      </>
    );
  }

  // Default redirect.
  if (pathname !== "/feedback") {
    queueMicrotask(() => navigate("/feedback", { replace: true }));
  }
  return null;
}
