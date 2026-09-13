import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useState,
  type ReactNode,
} from "react";

type Theme = { dark: boolean; toggle: () => void };

const ThemeContext = createContext<Theme>({ dark: false, toggle: () => {} });

export function ThemeProvider({ children }: { children: ReactNode }) {
  // The prerendered HTML is always light; the real theme resolves after
  // hydration, so server and client render the same first pass.
  const [dark, setDark] = useState(false);

  useEffect(() => {
    // `?theme=dark|light` pins the theme so screenshots and shared links are
    // deterministic; otherwise the saved choice wins, then the OS preference.
    const param = new URLSearchParams(location.search).get("theme");
    const saved = localStorage.getItem("folio-theme");
    let next: boolean;
    if (param === "dark") next = true;
    else if (param === "light") next = false;
    else if (saved === "dark" || saved === "light") next = saved === "dark";
    else next = window.matchMedia("(prefers-color-scheme: dark)").matches;
    setDark(next);
  }, []);

  useEffect(() => {
    document.documentElement.classList.toggle("dark", dark);
  }, [dark]);

  const toggle = useCallback(() => {
    // Only a user action persists the choice — otherwise the first load
    // would pin light-theme visitors against their OS preference.
    setDark((value) => {
      const next = !value;
      localStorage.setItem("folio-theme", next ? "dark" : "light");
      return next;
    });
  }, []);

  return (
    <ThemeContext.Provider value={{ dark, toggle }}>
      {children}
    </ThemeContext.Provider>
  );
}

export function useTheme(): Theme {
  return useContext(ThemeContext);
}
