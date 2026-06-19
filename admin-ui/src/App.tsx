import { useRouter } from "./shared/router";
import { Login } from "./pages/Login";
import { FeedbackList } from "./pages/FeedbackList";
import { FeedbackDrawer } from "./pages/FeedbackDrawer";
import { AdminRoadmap } from "./pages/roadmap/AdminRoadmap";
import { PublicRoadmap } from "./pages/roadmap/PublicRoadmap";
import { TierSettings } from "./pages/settings/TierSettings";
import { RunnerTokens } from "./pages/settings/RunnerTokens";
import { AutopilotDigest } from "./pages/autopilot/AutopilotDigest";
import { ClusterDetail } from "./pages/autopilot/ClusterDetail";
import { WorkOrderList } from "./pages/autopilot/WorkOrderList";
import { WorkOrderDetail } from "./pages/autopilot/WorkOrderDetail";

// Routes:
//   /login                                   → Login
//   /feedback                                → FeedbackList
//   /feedback/FB-XXXXXX                      → FeedbackList + FeedbackDrawer overlay
//   /admin/roadmap                           → AdminRoadmap (server-side sole-project resolution)
//   /admin/settings/tier                     → TierSettings (P3 Stage 2 — plan & usage)
//   /admin/settings/runner-tokens            → RunnerTokens (P5b — runner key + token lifecycle)
//   /admin/autopilot                         → AutopilotDigest (P5a — digest + cluster list)
//   /admin/autopilot/clusters/:clusterId     → ClusterDetail (members + rec cards)
//   /admin/autopilot/work-orders             → WorkOrderList
//   /admin/autopilot/work-orders/:id         → WorkOrderDetail (state + ledger + owner actions)
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

  if (
    pathname === "/admin/settings/tier" ||
    pathname === "/admin/settings/tier/"
  ) {
    return <TierSettings />;
  }

  if (
    pathname === "/admin/settings/runner-tokens" ||
    pathname === "/admin/settings/runner-tokens/"
  ) {
    return <RunnerTokens />;
  }

  // P5a autopilot surface (FR-FBR-21). Project-less admin URLs (sole-project
  // resolution via fetchAdminProjects), mirroring the /admin/roadmap
  // convention. Order matters: match the deeper routes before the index.
  const workOrderDetail = pathname.match(
    /^\/admin\/autopilot\/work-orders\/([^/]+)\/?$/,
  );
  if (workOrderDetail) {
    return (
      <WorkOrderDetail workOrderId={decodeURIComponent(workOrderDetail[1])} />
    );
  }
  if (
    pathname === "/admin/autopilot/work-orders" ||
    pathname === "/admin/autopilot/work-orders/"
  ) {
    return <WorkOrderList />;
  }
  const clusterDetail = pathname.match(
    /^\/admin\/autopilot\/clusters\/([^/]+)\/?$/,
  );
  if (clusterDetail) {
    return <ClusterDetail clusterId={decodeURIComponent(clusterDetail[1])} />;
  }
  if (pathname === "/admin/autopilot" || pathname === "/admin/autopilot/") {
    return <AutopilotDigest />;
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
