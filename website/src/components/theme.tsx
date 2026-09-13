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

function initialDark(): boolean {
  // `?theme=dark|light` pins the theme so screenshots and shared links are
  // deterministic; otherwise the saved choice wins, then the OS preference.
  const param = new URLSearchParams(location.search).get("theme");
  if (param === "dark") return true;
  if (param === "light") return false;
  const saved = localStorage.getItem("folio-theme");
  if (saved === "dark" || saved === "light") return saved === "dark";
  return window.matchMedia("(prefers-color-scheme: dark)").matches;
}

export function ThemeProvider({ children }: { children: ReactNode }) {
  const [dark, setDark] = useState(initialDark);

  useEffect(() => {
    document.documentElement.classList.toggle("dark", dark);
    localStorage.setItem("folio-theme", dark ? "dark" : "light");
  }, [dark]);

  const toggle = useCallback(() => setDark((value) => !value), []);

  return (
    <ThemeContext.Provider value={{ dark, toggle }}>
      {children}
    </ThemeContext.Provider>
  );
}

export function useTheme(): Theme {
  return useContext(ThemeContext);
}
