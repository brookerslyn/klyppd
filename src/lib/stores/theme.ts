import { writable, get } from 'svelte/store';

export interface ThemeVars {
  '--bg-base': string;
  '--bg-deepest': string;
  '--bg-elev-1': string;
  '--bg-elev-2': string;
  '--text': string;
  '--text-dim': string;
  '--text-muted': string;
  '--text-faint': string;
  '--border': string;
  '--border-strong': string;
  '--accent': string;
  '--danger': string;
  '--font-mono': string;
  '--font-size': string;
  '--bg-opacity': string;
  '--blur-radius': string;
}

export const defaultTheme: ThemeVars = {
  '--bg-base': '#0a0e14',
  '--bg-deepest': '#080c12',
  '--bg-elev-1': '#111620',
  '--bg-elev-2': '#161b23',
  '--text': '#b8c4d0',
  '--text-dim': '#7a8899',
  '--text-muted': '#4a5568',
  '--text-faint': '#364050',
  '--border': '#1a2030',
  '--border-strong': '#232a35',
  '--accent': '#56b6c2',
  '--danger': '#e06c75',
  '--font-mono': "'JetBrains Mono', 'Fira Code', 'Cascadia Code', 'IBM Plex Mono', ui-monospace, monospace",
  '--font-size': '12px',
  '--bg-opacity': '1.0',
  '--blur-radius': '0px',
};

function createThemeStore() {
  // Load from localStorage or fall back to defaults
  const saved = typeof localStorage !== 'undefined' ? localStorage.getItem('klyppd-theme') : null;
  const initial: ThemeVars = saved ? { ...defaultTheme, ...JSON.parse(saved) } : { ...defaultTheme };

  const { subscribe, set, update } = writable<ThemeVars>(initial);

  function applyToDOM(vars: ThemeVars) {
    const root = document.documentElement;
    for (const [key, value] of Object.entries(vars)) {
      root.style.setProperty(key, value);
    }
    const opacity = parseFloat(vars['--bg-opacity']) || 1;
    root.style.setProperty('--bg-base-t', hexToRgba(vars['--bg-base'], opacity));
    root.style.setProperty('--bg-deepest-t', hexToRgba(vars['--bg-deepest'], opacity));
    root.style.setProperty('--bg-elev-1-t', hexToRgba(vars['--bg-elev-1'], opacity));
    root.style.setProperty('--bg-elev-2-t', hexToRgba(vars['--bg-elev-2'], opacity));
  }

  function persist(vars: ThemeVars) {
    try { localStorage.setItem('klyppd-theme', JSON.stringify(vars)); } catch {}
  }

  // Apply on init
  if (typeof document !== 'undefined') applyToDOM(initial);

  return {
    subscribe,
    set(vars: ThemeVars) {
      set(vars);
      applyToDOM(vars);
      persist(vars);
    },
    updateVar(key: keyof ThemeVars, value: string) {
      update(v => {
        const next = { ...v, [key]: value };
        applyToDOM(next);
        persist(next);
        return next;
      });
    },
    apply() {
      applyToDOM(get({ subscribe }));
    },
    reset() {
      const d = { ...defaultTheme };
      set(d);
      applyToDOM(d);
      persist(d);
    },
    exportCSS(): string {
      const vars = get({ subscribe });
      const lines = Object.entries(vars).map(([k, v]) => `  ${k}: ${v};`);
      return `:root {\n${lines.join('\n')}\n}`;
    },
    importCSS(css: string) {
      const vars = { ...get({ subscribe }) };
      const regex = /(--[\w-]+)\s*:\s*([^;]+)/g;
      let m: RegExpExecArray | null;
      while ((m = regex.exec(css)) !== null) {
        const key = m[1] as keyof ThemeVars;
        if (key in vars) {
          vars[key] = m[2].trim();
        }
      }
      set(vars);
      applyToDOM(vars);
      persist(vars);
    },
  };
}

export const theme = createThemeStore();

function hexToRgba(hex: string, alpha: number): string {
  hex = hex.replace('#', '');
  if (hex.length === 3) hex = hex.split('').map(c => c + c).join('');
  const r = parseInt(hex.substring(0, 2), 16);
  const g = parseInt(hex.substring(2, 4), 16);
  const b = parseInt(hex.substring(4, 6), 16);
  if (isNaN(r) || isNaN(g) || isNaN(b)) return hex;
  return `rgba(${r}, ${g}, ${b}, ${alpha})`;
}
