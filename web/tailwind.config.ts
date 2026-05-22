import type { Config } from 'tailwindcss'

const config = {
  darkMode: ['class'],
  content: ['./index.html', './admin.html', './login.html', './registration-paused.html', './src/**/*.{ts,tsx}'],
  theme: {
    container: {
      center: true,
      padding: '1rem',
      screens: {
        '2xl': '1440px',
      },
    },
    extend: {
      colors: {
        clay: {
          canvas: 'hsl(var(--background) / <alpha-value>)',
          foreground: 'hsl(var(--foreground) / <alpha-value>)',
          muted: 'hsl(var(--muted-foreground) / <alpha-value>)',
          violet: 'hsl(var(--primary) / <alpha-value>)',
          pink: 'hsl(var(--secondary) / <alpha-value>)',
          sky: 'hsl(var(--accent) / <alpha-value>)',
          emerald: 'hsl(var(--success) / <alpha-value>)',
          amber: 'hsl(var(--warning) / <alpha-value>)',
        },
        border: 'hsl(var(--border) / <alpha-value>)',
        input: 'hsl(var(--input) / <alpha-value>)',
        ring: 'hsl(var(--ring) / <alpha-value>)',
        background: 'hsl(var(--background) / <alpha-value>)',
        foreground: 'hsl(var(--foreground) / <alpha-value>)',
        primary: {
          DEFAULT: 'hsl(var(--primary) / <alpha-value>)',
          foreground: 'hsl(var(--primary-foreground) / <alpha-value>)',
        },
        secondary: {
          DEFAULT: 'hsl(var(--secondary) / <alpha-value>)',
          foreground: 'hsl(var(--secondary-foreground) / <alpha-value>)',
        },
        destructive: {
          DEFAULT: 'hsl(var(--destructive) / <alpha-value>)',
          foreground: 'hsl(var(--destructive-foreground) / <alpha-value>)',
        },
        muted: {
          DEFAULT: 'hsl(var(--muted) / <alpha-value>)',
          foreground: 'hsl(var(--muted-foreground) / <alpha-value>)',
        },
        accent: {
          DEFAULT: 'hsl(var(--accent) / <alpha-value>)',
          foreground: 'hsl(var(--accent-foreground) / <alpha-value>)',
        },
        popover: {
          DEFAULT: 'hsl(var(--popover) / <alpha-value>)',
          foreground: 'hsl(var(--popover-foreground) / <alpha-value>)',
        },
        card: {
          DEFAULT: 'hsl(var(--card) / <alpha-value>)',
          foreground: 'hsl(var(--card-foreground) / <alpha-value>)',
        },
        warning: {
          DEFAULT: 'hsl(var(--warning) / <alpha-value>)',
          foreground: 'hsl(var(--warning-foreground) / <alpha-value>)',
        },
        success: {
          DEFAULT: 'hsl(var(--success) / <alpha-value>)',
          foreground: 'hsl(var(--success-foreground) / <alpha-value>)',
        },
        info: {
          DEFAULT: 'hsl(var(--info) / <alpha-value>)',
          foreground: 'hsl(var(--info-foreground) / <alpha-value>)',
        },
        // Compatibility aliases for existing utility classes used by current pages.
        'base-100': 'hsl(var(--base-100) / <alpha-value>)',
        'base-200': 'hsl(var(--base-200) / <alpha-value>)',
        'base-300': 'hsl(var(--base-300) / <alpha-value>)',
        'base-content': 'hsl(var(--base-content) / <alpha-value>)',
      },
      borderRadius: {
        lg: 'var(--radius)',
        md: 'calc(var(--radius) - 2px)',
        sm: 'calc(var(--radius) - 4px)',
        box: 'var(--radius-xl)',
      },
      boxShadow: {
        claySurface: 'var(--shadow-clay-surface)',
        clayCard: 'var(--shadow-clay-card)',
        clayButton: 'var(--shadow-clay-button)',
        clayButtonHover: 'var(--shadow-clay-button-hover)',
        clayPressed: 'var(--shadow-clay-pressed)',
      },
      fontFamily: {
        display: ['Nunito', 'Noto Sans SC', 'sans-serif'],
        sans: ['DM Sans', 'Noto Sans SC', 'ui-sans-serif', 'system-ui', 'sans-serif'],
        mono: ['JetBrains Mono', 'SFMono-Regular', 'Menlo', 'Monaco', 'Consolas', 'monospace'],
      },
      keyframes: {
        'clay-float': {
          '0%, 100%': { transform: 'translateY(0) rotate(0deg)' },
          '50%': { transform: 'translateY(-18px) rotate(2deg)' },
        },
        'clay-breathe': {
          '0%, 100%': { transform: 'scale(1)' },
          '50%': { transform: 'scale(1.018)' },
        },
      },
      animation: {
        'clay-float': 'clay-float 12s ease-in-out infinite',
        'clay-breathe': 'clay-breathe 6s ease-in-out infinite',
      },
    },
  },
  plugins: [],
} satisfies Config

export default config
