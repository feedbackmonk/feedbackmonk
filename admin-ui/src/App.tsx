import { useRouter } from "./shared/router";
import { Login } from "./pages/Login";
import { FeedbackList } from "./pages/FeedbackList";
import { FeedbackDrawer } from "./pages/FeedbackDrawer";
import { AdminRoadmap } from "./pages/roadmap/AdminRoadmap";
import { PublicRoadmap } from "./pages/roadmap/PublicRoadmap";

// Routes:
//   /login                                   → Login
//   /feedback                                → FeedbackList
//   /feedback/FB-XXXXXX                      → FeedbackList + FeedbackDrawer overlay
//   /admin/roadmap                           → AdminRoadmap (server-side sole-project resolution)
//   /public/projects/:projectId/roadmap      → PublicRoadmap (no admin chrome; project-segmented per Contract C15)
//   anything else                            → redirect to /feedback (or /login when API 401s)
//
// `/admin/roadmap` deliberately omits the project segment to mirror the
// existing project-less admin URL convention (`/feedback`). Server resolves
// sole-project from AdminSession. Multi-project URL routing deferred to P3.
export function App() {
  const { pathname, navigate } = useRouter();

  if (pathname === "/login") {
    return <Login />;
  }

  // Public roadmap — no auth, no admin chrome. Project-segmented because
  // the public page is cross-tenant addressable (Contract C15 spec).
  const publicRoadmap = pathname.match(
    /^\/public\/projects\/([^/]+)\/roadmap\/?$/,
  );
  if (publicRoadmap) {
    return <PublicRoadmap projectId={decodeURIComponent(publicRoadmap[1])} />;
  }

  if (pathname === "/admin/roadmap" || pathname === "/admin/roadmap/") {
    return <AdminRoadmap />;
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
