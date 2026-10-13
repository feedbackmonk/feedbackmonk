// The one-time sign-in handoff to the hosted board (FR-FBR-44, Contract C43).
//
// CONTRACT STUB — the `contracts` piece froze the route and props; the
// `public-ui` piece builds it: read `#token=` from the URL fragment, POST it
// once with `exchangeBoardSession`, remove the fragment from the address bar
// and history (`history.replaceState`), then go on to the board. The token
// never travels in a query string or path.
export interface SignInHandoffProps {
  projectId: string;
}

export function SignInHandoff(_props: SignInHandoffProps) {
  return null;
}
