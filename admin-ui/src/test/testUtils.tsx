import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, type RenderOptions } from "@testing-library/react";
import { type ReactElement, type ReactNode } from "react";
import { Router } from "../shared/router";

export function makeClient(): QueryClient {
  return new QueryClient({
    defaultOptions: {
      queries: { retry: false, gcTime: 0, staleTime: 0 },
      mutations: { retry: false },
    },
  });
}

interface RenderWithClientOptions extends RenderOptions {
  client?: QueryClient;
  withRouter?: boolean;
  initialPath?: string;
}

export function renderWithClient(
  ui: ReactElement,
  opts: RenderWithClientOptions = {},
) {
  const {
    client = makeClient(),
    withRouter = false,
    initialPath,
    ...rest
  } = opts;
  if (initialPath && typeof window !== "undefined") {
    window.history.replaceState(null, "", initialPath);
  }
  const Wrapper = ({ children }: { children: ReactNode }) =>
    withRouter ? (
      <QueryClientProvider client={client}>
        <Router>{children}</Router>
      </QueryClientProvider>
    ) : (
      <QueryClientProvider client={client}>{children}</QueryClientProvider>
    );
  return { client, ...render(ui, { wrapper: Wrapper, ...rest }) };
}
