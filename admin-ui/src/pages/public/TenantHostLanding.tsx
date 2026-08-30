import { useQuery } from "@tanstack/react-query";
import { fetchPublicSite } from "../../shared/hostingApi";
import { PublicBoard } from "../board/PublicBoard";
import { PublicRoadmap } from "../roadmap/PublicRoadmap";

interface TenantHostLandingProps {
  /** Which public surface to show for this host's project. */
  surface: "board" | "roadmap";
  /** Rendered when this host is not tenant-bound (the admin host, or a
   *  deployment with no root domain configured). */
  fallback: () => JSX.Element | null;
}

// The landing for a tenant host (FR-FBR-32).
//
// DEC-FBR-13 puts each tenant's board at `{tenant}.feedbackmonk.com` — with no
// project id in the URL, because the host already identifies the tenant. This
// component is what closes that gap between "the subdomain resolves" and "the
// subdomain shows the board": it asks `/api/v1/public/site` who lives on this
// host, then hands the resolved project to the existing PublicBoard /
// PublicRoadmap components.
//
// WHY NOT A NEW ROUTE FAMILY: forking the public handlers for host-rooted URLs
// would double the surface that the moderation gate, the privacy shape and the
// rate-limit floor all have to cover. One discovery call and the already-hardened
// project-scoped endpoints is strictly less to get wrong.
//
// FALLBACK IS THE COMMON CASE, NOT THE ERROR CASE. On the admin host — and on
// any self-host deployment, where no root domain is configured — `/public/site`
// 404s, and this component renders the caller's fallback (the ordinary admin
// routing). That is why the 404 is handled as a route decision rather than
// surfaced as an error: nothing has gone wrong.
export function TenantHostLanding({ surface, fallback }: TenantHostLandingProps) {
  const query = useQuery({
    queryKey: ["public-site"],
    queryFn: fetchPublicSite,
    // The host→tenant mapping cannot change within a page view.
    staleTime: Infinity,
    retry: false,
  });

  if (query.isPending) {
    return (
      <main className="tenant-landing" aria-busy="true">
        <p className="muted">Loading…</p>
      </main>
    );
  }

  const site = query.data;
  if (!site) {
    // Not a tenant host (404), or the lookup failed. Either way this is not a
    // public surface — hand back to the admin app.
    return fallback();
  }

  // A tenant may own several projects but a host names only one address, so the
  // landing shows the first project. Multi-project tenants reach the others by
  // their existing `/public/projects/{id}/…` URLs, which keep working; a
  // per-project host shape would need a second DNS label per project and is not
  // what DEC-FBR-13 chose.
  const project = site.projects[0];
  if (!project) {
    return (
      <main className="tenant-landing">
        <h1>Nothing here yet</h1>
        <p className="muted">This site has no public projects.</p>
      </main>
    );
  }

  if (surface === "roadmap") {
    return <PublicRoadmap projectId={project.project_id} />;
  }

  if (!project.public_board_enabled) {
    // Distinguish "the board is switched off" from "the host is unknown". The
    // server would 404 the board read either way; saying which is true is the
    // difference between a visitor retrying and a visitor giving up.
    return (
      <main className="tenant-landing">
        <h1>Board not available</h1>
        <p className="muted">
          This project hasn’t published a public feedback board.
        </p>
      </main>
    );
  }

  return <PublicBoard projectId={project.project_id} />;
}
