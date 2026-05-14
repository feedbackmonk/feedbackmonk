import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useState,
  type ReactNode,
} from "react";

// Minimal history-API router. Two-route system + dynamic drawer; pulling in
// react-router-dom would be premature abstraction at this scope. If the
// admin UI grows beyond ~5 routes, swap in then.

interface RouteState {
  pathname: string;
  search: string;
}

interface RouterContextValue extends RouteState {
  navigate: (to: string, opts?: { replace?: boolean }) => void;
}

const RouterContext = createContext<RouterContextValue | null>(null);

function readLocation(): RouteState {
  return {
    pathname: window.location.pathname || "/",
    search: window.location.search || "",
  };
}

export function Router({ children }: { children: ReactNode }) {
  const [state, setState] = useState<RouteState>(() => readLocation());

  useEffect(() => {
    const onPop = () => setState(readLocation());
    window.addEventListener("popstate", onPop);
    return () => window.removeEventListener("popstate", onPop);
  }, []);

  const navigate = useCallback(
    (to: string, opts: { replace?: boolean } = {}) => {
      const url = new URL(to, window.location.origin);
      if (opts.replace) {
        window.history.replaceState(null, "", url);
      } else {
        window.history.pushState(null, "", url);
      }
      setState({ pathname: url.pathname, search: url.search });
    },
    [],
  );

  const value = useMemo<RouterContextValue>(
    () => ({ ...state, navigate }),
    [state, navigate],
  );

  return (
    <RouterContext.Provider value={value}>{children}</RouterContext.Provider>
  );
}

export function useRouter(): RouterContextValue {
  const ctx = useContext(RouterContext);
  if (!ctx) {
    throw new Error("useRouter must be used inside <Router>");
  }
  return ctx;
}

export function useSearchParams(): [
  URLSearchParams,
  (next: URLSearchParams, opts?: { replace?: boolean }) => void,
] {
  const { pathname, search, navigate } = useRouter();
  const params = useMemo(() => new URLSearchParams(search), [search]);
  const setParams = useCallback(
    (next: URLSearchParams, opts?: { replace?: boolean }) => {
      const s = next.toString();
      navigate(`${pathname}${s ? `?${s}` : ""}`, opts);
    },
    [pathname, navigate],
  );
  return [params, setParams];
}

export function Link({
  to,
  children,
  ...rest
}: {
  to: string;
  children: ReactNode;
} & Omit<React.AnchorHTMLAttributes<HTMLAnchorElement>, "href">) {
  const { navigate } = useRouter();
  return (
    <a
      href={to}
      onClick={(e) => {
        if (
          e.defaultPrevented ||
          e.button !== 0 ||
          e.metaKey ||
          e.ctrlKey ||
          e.shiftKey ||
          e.altKey
        ) {
          return;
        }
        e.preventDefault();
        navigate(to);
      }}
      {...rest}
    >
      {children}
    </a>
  );
}
