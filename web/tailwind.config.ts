import type { Config } from 'tailwindcss';
import forms from '@tailwindcss/forms';

const config: Config = {
  content: [
    './src/pages/**/*.{js,ts,jsx,tsx,mdx}',
    './src/components/**/*.{js,ts,jsx,tsx,mdx}',
    './src/features/**/*.{js,ts,jsx,tsx,mdx}',
    './src/app/**/*.{js,ts,jsx,tsx,mdx}',
  ],
  theme: {
    extend: {
      colors: {
        // Calm, trustworthy fintech palette
        primary: {
          50: '#f0f7ff',
          100: '#e0effe',
          200: '#bae0fd',
          300: '#7cc9fc',
          400: '#36b2f9',
          500: '#0b9aef',
          600: '#0672d0',
          700: '#0559a9',
          800: '#084a8d',
          900: '#0a3d73',
          950: '#071e3d',
        },
        // African fintech accent - warm terracotta
        accent: {
          50: '#fdf9f3',
          100: '#faf2e8',
          200: '#f4e0d0',
          300: '#eac7af',
          400: '#dea883',
          500: '#d48c63',
          600: '#c27551',
          700: '#a25f42',
          800: '#854f39',
          900: '#6f4332',
          950: '#3a2217',
        },
        // Neutral grays for trusted feel
        neutral: {
          50: '#f9fafb',
          100: '#f3f4f6',
          200: '#e5e7eb',
          300: '#d1d5db',
          400: '#9ca3af',
          500: '#6b7280',
          600: '#4b5563',
          700: '#374151',
          800: '#1f2937',
          900: '#111827',
          950: '#030712',
        },
        // Status colors
        success: {
          50: '#f0fdf4',
          600: '#16a34a',
          700: '#15803d',
        },
        warning: {
          50: '#fffbeb',
          600: '#d97706',
          700: '#b45309',
        },
        error: {
          50: '#fef2f2',
          600: '#dc2626',
          700: '#b91c1c',
        },
      },
      fontFamily: {
        sans: ['ui-sans-serif', 'system-ui', '-apple-system', 'BlinkMacSystemFont', '"Segoe UI"', 'sans-serif'],
        mono: ['ui-monospace', 'SFMono-Regular', '"SF Mono"', 'Monaco', '"Cascadia Code"', 'monospace'],
      },
      fontSize: {
        // Money-focused typography
        'money-lg': ['2.5rem', { lineHeight: '1.1', fontVariantNumeric: 'tabular-nums' }],
        'money-md': ['1.875rem', { lineHeight: '1.2', fontVariantNumeric: 'tabular-nums' }],
        'money-sm': ['1.25rem', { lineHeight: '1.3', fontVariantNumeric: 'tabular-nums' }],
      },
      spacing: {
        'safe': 'max(1rem, env(safe-area-inset-bottom))',
      },
      borderRadius: {
        lg: '0.5rem',
        xl: '0.75rem',
        '2xl': '1rem',
      },
      keyframes: {
        'fade-in': {
          '0%': { opacity: '0' },
          '100%': { opacity: '1' },
        },
        'slide-up': {
          '0%': { transform: 'translateY(1rem)', opacity: '0' },
          '100%': { transform: 'translateY(0)', opacity: '1' },
        },
      },
      animation: {
        'fade-in': 'fade-in 0.3s ease-out',
        'slide-up': 'slide-up 0.3s ease-out',
      },
    },
  },
  plugins: [forms],
  darkMode: 'class',
};
export default config;
